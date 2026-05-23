# Origin of the 6.5x SpatiaLite-vs-sqlitegis unindexed scan gap

## Context

The `benches/spatialite_vs_sqlitegis.rs` head-to-head bench (50k random WGS84 points, query: `WHERE ST_Intersects(geom, ST_GeomFromText('POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))', 4326))`) shows SpatiaLite at 8.7 ms / 50k rows = **0.17 us/row** versus sqlitegis at 57.0 ms / 50k rows = **1.14 us/row**. The previous hypothesis (missing bbox quick-reject) only saved ~3% when added to `st_intersects`, so the dominant cost lives somewhere earlier. This document captures what the per-row cost actually consists of on each side, from a direct read of both codebases.

## What sqlitegis does per row (confirmed)

Cost ledger from `src/sqlite/loadable/scalar.rs:641` (`st_intersects_cb`) down to `src/core/functions/predicates.rs:31` (`st_intersects`):

| Step | Per-row cost | File |
|---|---|---|
| Blob pointer extraction (zero-copy) | < 0.01 us | `src/sqlite/loadable/args.rs` |
| Parse left blob header | < 0.05 us | `src/core/ewkb.rs:300` |
| **`Ewkb(left).to_geo()` heap-allocs full `geo::Geometry`** | ~0.4 - 0.6 us | `src/core/ewkb.rs:305` |
| Parse right blob header | < 0.05 us | same |
| **`Ewkb(right).to_geo()` heap-allocs full `geo::Geometry`** | ~0.4 - 0.6 us | same, executed 50k times on the *same constant polygon bytes* |
| `bounding_rect()` + bbox intersect (already-parsed) | < 0.01 us | `src/core/functions/predicates.rs:36-39` |
| `geo::Intersects::intersects` on positive rows only | ~0.1 - 0.2 us | `src/core/functions/predicates.rs:42` |
| **Total** | **~1.14 us** | |

Critical facts:

1. **Zero use of `sqlite3_set_auxdata` / `sqlite3_get_auxdata` anywhere in the crate.** Confirmed via grep: the right-hand-side constant polygon blob is fully re-parsed on every one of the 50,000 calls.
2. **No MBR-only fastpath in the EWKB parser.** `bounding_rect()` is called only after the full `geo::Geometry` has been heap-allocated. There is no path that reads "just the bbox" from raw EWKB bytes.

## What SpatiaLite does per row (confirmed against upstream C source)

Two distinct optimisations, both source-cited:

### 1. Inline MBR in every geometry blob

SpatiaLite blobs carry a precomputed minimum bounding rectangle as bytes 6-37 of the blob payload (`gg_relations.c`, `tinyPoint2Geom` and friends):

```
byte 0      GAIA_MARK_START      sentinel
byte 1      endian byte
bytes 2-5   SRID (i32)
bytes 6-13  MinX (f64, little-endian)
bytes 14-21 MinY (f64, little-endian)
bytes 22-29 MaxX (f64, little-endian)
bytes 30-37 MaxY (f64, little-endian)
byte 38     GAIA_MARK_MBR        sentinel
... type, geometry payload follows ...
```

Reading the MBR is therefore an O(1), 32-byte load from raw bytes with **no allocation and no geometry materialisation**. For our bench's 99.99%-negative-row distribution, the MBR-vs-MBR reject completes in tens of nanoseconds per row.

EWKB has no such header. The corresponding sqlitegis path has to decode the full geometry before it can compute `bounding_rect()`.

### 2. Per-connection prepared-geometry cache (not SQLite auxdata)

SpatiaLite's `ST_Intersects` SQL callback is `fnct_Intersects` in `spatialite.c`, which delegates to `gaiaGeomCollPreparedIntersects` in `gg_relations.c`. The cache logic is in `evalGeosCacheItem`:

```c
/* the first 46 bytes of the BLOB contain the MBR,
   the SRID and the Type; so are assumed to represent
   a valid signature */
if (memcmp(blob, p->gaiaBlob, 46) == 0)
    return 1;
```

The `splite_internal_cache` struct holds two `cacheItem` slots; consecutive calls with identical-prefix blobs (RHS constant in our bench) hit the cache via a 46-byte memcmp, skip `gaiaToGeos_r()` entirely, and reuse a pre-prepared `GEOSPreparedGeometry`. They **do not use `sqlite3_set_auxdata`**; they use `sqlite3_user_data(context)` to fetch the per-connection cache instead.

For the bench workload this means the constant polygon is converted to GEOS form exactly once, not 50,000 times.

## Why the gap is 6.5x

Combining the two optimisations on the 99.99%-negative-row workload:

- SpatiaLite per row: ~10 ns memcmp prefix check + ~20 ns MBR decode/compare from inline header = ~30 ns. Then exit. Bench observed: 174 ns/row including SQLite virtual-machine and dispatch overhead.
- sqlitegis per row: ~0.4-0.6 us decode left point + ~0.4-0.6 us decode right polygon (redundant, same bytes 50k times) + ~0.01 us bbox compare = ~1.0 us. Bench observed: 1.14 us/row.

Ratio of the geometry-handling cost alone: roughly 30x. The 6.5x bench ratio is what's left after SQLite's per-call overhead amortises the rest. **Most of the gap is the absence of an inline MBR in our blob format, with the redundant RHS re-parse as a meaningful second contributor.**

The reason the bbox-prefilter we added in `st_intersects` only saved 3% is now obvious: it still costs the full geometry decode of both sides before it can read the bbox. The MBR comparison itself is cheap; the parse it depends on is not.

## Candidate remedies (for discussion, not implementation)

Three orthogonal fixes, ranked by impact on the unindexed scan:

A. **MBR-only fastpath in the EWKB parser.** Walk the EWKB bytes to compute the bounding box without heap-allocating a `geo::Geometry`. For a `POINT` blob this is trivial: read 2 doubles (or 3 for XYZ). For `LINESTRING` / `POLYGON` it's O(n) but still allocation-free. Wire it into `st_intersects` as the first check before falling through to the full parse only when the bbox actually overlaps. Closes the dominant cost contributor on the 99.99% negative rows. Non-breaking: the EWKB blob layout does not change.

B. **`sqlite3_set_auxdata` caching of the parsed RHS geometry.** When the same `sqlite3_value*` pointer is passed across consecutive callback invocations, attach the parsed `geo::Geometry` to the value via `sqlite3_set_auxdata`, with a destructor that drops the boxed geometry when SQLite releases the value. Requires plumbing in both `src/sqlite/ffi.rs` and `src/sqlite/loadable/scalar.rs`. Closes the redundant-decode contributor (one decode per query, not per row).

C. **Add an MBR header to our blob format.** Mirror SpatiaLite's design: bytes 6-37 of every EWKB blob become the inline MBR, written at construction time, read in O(1) at predicate time. Maximally fast but **breaking** for any persisted databases produced by sqlitegis 0.1.x. Would need a migration story and a feature flag.

D (longer term). **R-tree-indexed queries already win.** The same bench's R-tree-indexed `ST_Intersects` path has sqlitegis ahead of SpatiaLite (9.19 us vs 11.66 us, 1.27x). If the project's positioning is "use the R-tree, it's there for a reason," the unindexed-scan gap is academic. This is a legitimate "do nothing" option if the architecture treats unindexed scans as out-of-scope.

## Files to read for the discussion

To make the discussion concrete, the relevant code lives at:

- `src/sqlite/loadable/scalar.rs:641`: `st_intersects_cb` callback
- `src/core/functions/predicates.rs:31-42`: `st_intersects` (now has the bbox prefilter we just added)
- `src/core/ewkb.rs:299-317`: `parse_ewkb` / `parse_ewkb_pair`, the parse path
- `src/sqlite/ffi.rs` and `src/sqlite/loadable/scalar.rs`: both FFI paths would need to be touched for any of options A, B, or C

## Verification (when a fix lands)

Re-run `cargo bench --features bench-spatialite --bench spatialite_vs_sqlitegis -- --warm-up-time 2 --measurement-time 6` and compare the **Unindexed ST_Intersects bulk** group's median time against the current 57.0 ms baseline. Option A is expected to drop it to ~5-10 ms (closing or reversing the gap). Option B alone would drop it to ~30-35 ms (~half the cost, since the LHS still needs to be parsed). Option A+B would compound. Option C would close the gap entirely but is the most invasive.

Correctness regression check: `cargo test --workspace --features sqlite` (currently 119 passing) plus `bindings/python/tests` (currently 37 passing, 100% coverage) must remain green for any candidate.
