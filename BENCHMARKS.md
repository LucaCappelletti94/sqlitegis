# sqlitegis benchmarks

All measurements were captured on the same machine with [Criterion](https://github.com/bheisler/criterion.rs) using its default 100-sample protocol. Numbers are central estimates; the deltas reported as "sqlitegis Nx" or "SpatiaLite Nx" are ratios of the two libraries' medians on the same query.

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

`benches/spatialite_vs_sqlitegis.rs` (gated behind the `bench-spatialite` Cargo feature, requires `libsqlite3-mod-spatialite` installed system-wide) runs both libraries on the same in-process libsqlite3, so the comparison isolates per-callback cost from engine differences. The dataset is 50k random WGS84 points in `places` and 50k random 0.1-degree axis-aligned polygons in `regions`; both libraries see identical bytes.

| Workload | sqlitegis | SpatiaLite | Ratio |
| --- | ---: | ---: | --- |
| `ST_Intersects` bulk, unindexed | `5.65 ms` | `9.73 ms` | `sqlitegis 1.72x` |
| `ST_Intersects` window, R-tree-prefiltered | `10.31 us` | `12.89 us` | `sqlitegis 1.25x` |
| `ST_Contains` bulk, unindexed | `5.68 ms` | `9.44 ms` | `sqlitegis 1.66x` |
| `ST_Contains` window, R-tree-prefiltered | `10.34 us` | `12.70 us` | `sqlitegis 1.23x` |
| `ST_Covers` bulk, unindexed | `5.79 ms` | `8.91 ms` | `sqlitegis 1.54x` |
| `ST_Touches` bulk, unindexed | `10.12 ms` | `12.83 ms` | `sqlitegis 1.27x` |
| `ST_Overlaps` bulk, unindexed | `10.82 ms` | `13.06 ms` | `sqlitegis 1.21x` |
| `ST_Equals` bulk, unindexed | `10.59 ms` | `12.79 ms` | `sqlitegis 1.21x` |
| `ST_DWithin` bulk, unindexed | `28.73 ms` | `39.63 ms` | `sqlitegis 1.38x` |
| `ST_DistanceSphere` bulk | `30.14 ms` | `254.72 ms` | `sqlitegis 8.45x` |
| `ST_AsText` scalar throughput | `28.28 ms` | `49.99 ms` | `sqlitegis 1.77x` |
| `ST_Buffer` scalar throughput | `329.56 ms` | `694.51 ms` | `sqlitegis 2.11x` |
| `ST_Centroid` scalar throughput | `56.10 ms` | `38.16 ms` | `SpatiaLite 1.47x` |
| `ST_Buffer` + `ST_Intersection` bulk | `36.85 ms` | `28.54 ms` | `SpatiaLite 1.29x` |
| `ST_Difference` disjoint bulk | `254.51 ms` | `224.24 ms` | `SpatiaLite 1.10x` |
| `ST_Union` disjoint bulk | `226.19 ms` | `86.10 ms` | `SpatiaLite 2.63x` |
| `ST_SymDifference` disjoint bulk | `224.51 ms` | `89.68 ms` | `SpatiaLite 2.51x` |

sqlitegis is ahead on 12 of the 17 workloads. The wins on the binary predicates (`ST_Intersects`, `ST_Contains`, `ST_Covers`, `ST_Touches`, `ST_Overlaps`, `ST_Equals`) come from an MBR-only fastpath that walks the EWKB bytes for the bounding rectangle and short-circuits the full geometric test when bboxes cannot satisfy the predicate. On filter-heavy "find features in a window" workloads (the vast majority of real PostGIS queries) the negative-row path stops paying for a full decode and runs in ~60 ns instead of a few microseconds per row.

The geodesic margin comes from SpatiaLite's 3-arg `ST_Distance(g1, g2, use_ellipsoid)` paying ellipsoid setup cost even on the sphere branch, while `ST_DistanceSphere` is a direct Haversine on `f64` lat/lon pairs.

The remaining gaps are GEOS-favored. `ST_Centroid` and `ST_Buffer + ST_Intersection` lose by under 1.5x on workloads where decades of GEOS optimisation show up. `ST_Union` and `ST_SymDifference` still lose by ~2.5x on the disjoint-bbox bench even though we already short-circuit the BooleanOps sweep when bboxes don't overlap: the residual cost is the per-row decode and serialize of the constant LHS, which a future `sqlite3_set_auxdata` cache could amortise across rows.

## SpatiaLite naming quirks worth knowing

While porting bench queries between the two libraries, the following function-name differences mattered. None of them are sqlitegis bugs; documented here for anyone porting queries.

- `ST_DistanceSphere(g1, g2)` (PostGIS / sqlitegis) is `ST_Distance(g1, g2, 0)` in SpatiaLite 5.1.0.
- `ST_DistanceSpheroid(g1, g2)` is `ST_Distance(g1, g2, 1)` in SpatiaLite 5.1.0.
- `ST_MakeEnvelope(xmin, ymin, xmax, ymax, srid)` is not present in SpatiaLite 5.1.0; bench code constructs the envelope as a `POLYGON` WKT literal instead.
- `ST_DWithin(g1, g2, dist)` is not present in SpatiaLite 5.1.0; bench code rewrites it as `ST_Distance(g1, g2) <= dist`.
- `GreatCircleDistance` was present in SpatiaLite 4.x but removed in 5.x.

## Reproducing

`libsqlite3-mod-spatialite` must be installed system-wide so the SQLite loader can find `mod_spatialite`. Then:

```sh
cargo bench --features "bench-spatialite sqlite bundled-sqlite" --bench spatialite_vs_sqlitegis -- --warm-up-time 2 --measurement-time 6
```

CI does not run this bench. SpatiaLite is not a default CI dep, and the bench is feature-gated off so the rest of the matrix stays unaffected.
