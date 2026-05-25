# sqlitegis benchmarks

All measurements were captured on the same machine with [Criterion](https://github.com/bheisler/criterion.rs) using its default 100-sample protocol. Numbers are central estimates. The deltas reported as "sqlitegis Nx" or "SpatiaLite Nx" are ratios of the two libraries' medians on the same query.

## R-tree workloads

Indexed vs non-indexed scans on the in-tree spatial-index bench. Confirms the R-tree path pays off for typical "find features in a window" or KNN queries.

| Scenario | Indexed | Non-indexed | Speedup |
| --- | ---: | ---: | ---: |
| `intersects_window` | `178 us` | `9.81 ms` | `~55x` |
| `knn` | `89 us` | `5.66 ms` | `~64x` |

Reproduce with:

```sh
cargo bench --features diesel-sqlite --bench spatial_index
```

## vs SpatiaLite

[SpatiaLite](https://www.gaia-gis.it/fossil/libspatialite/index) is the long-established C extension that adds PostGIS-style spatial functions to SQLite, built on top of [GEOS](https://libgeos.org/) (the C++ port of the JTS computational-geometry suite) and [PROJ](https://proj.org/) (the standard coordinate-reprojection library). It is the closest existing analogue to sqlitegis and the natural baseline to measure against.

SpatiaLite would have been the obvious choice except for two practical issues. Its C/C++/GEOS/PROJ build chain is a recurring source of friction (a substantial native build with several optional system libraries), and it does not run cleanly on WebAssembly or edge devices. A pure Rust crate compiles to those targets without extra tooling and pulls no transitive C dependencies. That is the gap sqlitegis fills.

`benches/spatialite_vs_sqlitegis.rs` (gated behind the `bench-spatialite` Cargo feature, requires `libsqlite3-mod-spatialite` installed system-wide) runs both libraries on the same in-process libsqlite3, so the comparison isolates per-callback cost from engine differences. The dataset is 50k random WGS84 points in `places` and 50k random 0.1-degree axis-aligned polygons in `regions`. Both libraries see identical bytes.

| Workload | sqlitegis | SpatiaLite | Ratio |
| --- | ---: | ---: | --- |
| `ST_Intersects` bulk, unindexed | `5.78 ms` | `9.63 ms` | `sqlitegis 1.66x` |
| `ST_Intersects` window, R-tree-prefiltered | `10.62 us` | `12.96 us` | `sqlitegis 1.22x` |
| `ST_Contains` bulk, unindexed | `5.87 ms` | `9.83 ms` | `sqlitegis 1.67x` |
| `ST_Contains` window, R-tree-prefiltered | `10.40 us` | `12.99 us` | `sqlitegis 1.25x` |
| `ST_Covers` bulk, unindexed | `5.82 ms` | `9.45 ms` | `sqlitegis 1.62x` |
| `ST_Touches` bulk, unindexed | `9.22 ms` | `12.68 ms` | `sqlitegis 1.37x` |
| `ST_Overlaps` bulk, unindexed | `8.96 ms` | `12.75 ms` | `sqlitegis 1.42x` |
| `ST_Equals` bulk, unindexed | `9.31 ms` | `12.88 ms` | `sqlitegis 1.38x` |
| `ST_DWithin` bulk, unindexed | `28.03 ms` | `34.41 ms` | `sqlitegis 1.23x` |
| `ST_DistanceSphere` bulk | `30.02 ms` | `257.99 ms` | `sqlitegis 8.59x` |
| `ST_DistanceSpheroid` bulk | `86.63 ms` | `318.77 ms` | `sqlitegis 3.68x` |
| `ST_DWithinSphere` bulk, unindexed | `30.94 ms` | `255.73 ms` | `sqlitegis 8.27x` |
| `ST_DWithinSpheroid` bulk, unindexed | `85.35 ms` | `315.55 ms` | `sqlitegis 3.70x` |
| `ST_Distance` planar bulk | `28.61 ms` | `34.43 ms` | `sqlitegis 1.20x` |
| `ST_AsText` scalar throughput | `27.96 ms` | `48.97 ms` | `sqlitegis 1.75x` |
| `ST_AsGeoJSON` serialize throughput | `31.44 ms` | `65.64 ms` | `sqlitegis 2.09x` |
| `ST_AsBinary` serialize throughput | `26.84 ms` | `7.25 ms` | `SpatiaLite 3.70x` |
| `ST_GeomFromText` parse throughput | `1.44 ms` | `2.99 ms` | `sqlitegis 2.08x` |
| `ST_GeomFromWKB` parse throughput | `1.50 ms` | `2.97 ms` | `sqlitegis 1.98x` |
| `ST_Buffer` scalar throughput | `311.08 ms` | `714.05 ms` | `sqlitegis 2.30x` |
| `ST_Buffer` + `ST_Intersection` bulk | `36.27 ms` | `28.46 ms` | `SpatiaLite 1.27x` |
| `ST_Centroid` scalar throughput | `55.27 ms` | `38.06 ms` | `SpatiaLite 1.45x` |
| `ST_Envelope` scalar throughput | `113.79 ms` | `32.59 ms` | `SpatiaLite 3.49x` |
| `ST_Area` sum | `40.18 ms` | `18.82 ms` | `SpatiaLite 2.13x` |
| `ST_Perimeter` sum | `40.65 ms` | `18.98 ms` | `SpatiaLite 2.14x` |
| `ST_X` sum | `14.63 ms` | `4.91 ms` | `SpatiaLite 2.98x` |
| `ST_Y` sum | `14.59 ms` | `5.02 ms` | `SpatiaLite 2.90x` |
| `ST_Difference` disjoint bulk | `250.00 ms` | `218.54 ms` | `SpatiaLite 1.14x` |
| `ST_Difference` overlapping bulk | `162.62 ms` | `327.18 ms` | `sqlitegis 2.01x` |
| `ST_Union` disjoint bulk | `88.08 ms` | `84.08 ms` | `SpatiaLite 1.05x` |
| `ST_SymDifference` disjoint bulk | `87.26 ms` | `83.69 ms` | `SpatiaLite 1.04x` |

sqlitegis is ahead on 20 of the 31 workloads, within run-to-run noise on two more (`ST_Union` and `ST_SymDifference` disjoint), and behind on nine. The headline patterns:

**Predicate wins.** The binary predicates (`ST_Intersects`, `ST_Contains`, `ST_Covers`, `ST_Touches`, `ST_Overlaps`, `ST_Equals`) win 1.2x to 1.7x via an MBR-only fastpath that walks the EWKB bytes for the bounding rectangle and short-circuits the full geometric test when bboxes cannot satisfy the predicate. On filter-heavy "find features in a window" workloads the negative-row path stops paying for a full decode and runs in ~60 ns instead of a few microseconds per row.

**Geodesic family wins.** All five geodesic workloads (`ST_DistanceSphere`, `ST_DistanceSpheroid`, `ST_DWithinSphere`, `ST_DWithinSpheroid`, planar `ST_Distance`) put sqlitegis ahead, ranging from 1.2x to 8.6x. SpatiaLite's 3-arg `ST_Distance(g1, g2, use_ellipsoid)` pays PROJ-based ellipsoid setup cost even on the sphere branch. sqlitegis uses direct Haversine on `f64` lat/lon pairs for the sphere variant and `geographiclib-rs` for the ellipsoid variant.

**Set-op wire-level fastpath.** `ST_Union` and `ST_SymDifference` on disjoint inputs splice the two input EWKB blobs into a `MultiPolygon` result without decoding either side. That closes the SpatiaLite gap from ~2.5x to within run-to-run noise. `ST_Difference` overlapping unexpectedly wins 2x even on the BooleanOps slow path. `ST_Difference` disjoint still loses by 1.14x. Extending the splice trick to "return A unchanged" would close it.

**I/O wins, with one exception.** `ST_GeomFromText` and `ST_GeomFromWKB` parse 2x faster, `ST_AsText` serialises 1.75x faster, `ST_AsGeoJSON` 2x faster. The exception is `ST_AsBinary` at 3.70x slower: today it round-trips through `geo::Geometry` plus geozero serializer even though the conversion from EWKB to ISO WKB is byte-level trivial for XY inputs (strip SRID flag from type word, strip the SRID bytes, copy the rest). Identified optimisation candidate.

**Remaining GEOS-favored gaps.** `ST_Centroid` and `ST_Buffer + ST_Intersection` lose by under 1.5x where decades of GEOS optimisation show up. `ST_Envelope` loses 3.49x for the same reason `ST_AsBinary` does: today goes through full decode + bounding rect + serialize, when `extract_mbr` already walks the EWKB and an MBR-fastpath would build the 5-vertex result polygon by hand. Identified optimisation candidate.

**Surprise scalar losses.** `ST_X`, `ST_Y`, `ST_Area`, `ST_Perimeter` lose ~2-3x on what should be near-trivial header walks. SpatiaLite likely binds these as thin C wrappers that read a few EWKB bytes directly. sqlitegis goes through geozero's full decode path. A header-walk-only fastpath for these (in the spirit of `extract_mbr`) is plausible follow-up work.

## SpatiaLite naming quirks worth knowing

While porting bench queries between the two libraries, the following function-name differences mattered. None of them are sqlitegis bugs. Documented here for anyone porting queries.

- `ST_DistanceSphere(g1, g2)` (PostGIS / sqlitegis) is `ST_Distance(g1, g2, 0)` in SpatiaLite 5.1.0.
- `ST_DistanceSpheroid(g1, g2)` is `ST_Distance(g1, g2, 1)` in SpatiaLite 5.1.0.
- `ST_MakeEnvelope(xmin, ymin, xmax, ymax, srid)` is not present in SpatiaLite 5.1.0. Bench code constructs the envelope as a `POLYGON` WKT literal instead.
- `ST_DWithin(g1, g2, dist)` is not present in SpatiaLite 5.1.0. Bench code rewrites it as `ST_Distance(g1, g2) <= dist`. The same rewrite applies to `ST_DWithinSphere` (`ST_Distance(g1, g2, 0) <= dist`) and `ST_DWithinSpheroid` (`ST_Distance(g1, g2, 1) <= dist`).
- `ST_AsGeoJSON(g)` (PostGIS / sqlitegis) is `AsGeoJSON(g)` (no `ST_` prefix) in SpatiaLite 5.1.0.
- `GreatCircleDistance` was present in SpatiaLite 4.x but removed in 5.x.

## Reproducing

`libsqlite3-mod-spatialite` must be installed system-wide so the SQLite loader can find `mod_spatialite`. Then:

```sh
cargo bench --features "bench-spatialite sqlite bundled-sqlite" --bench spatialite_vs_sqlitegis -- --warm-up-time 2 --measurement-time 6
```

CI does not run this bench. SpatiaLite is not a default CI dep, and the bench is feature-gated off so the rest of the matrix stays unaffected.
