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

The five curved-earth areal workloads run over a clipped copy of `regions` holding 49821 of the 50000 polygons. The generated squares reach the south pole, where sqlitegis refuses a vertex outside `[-90, 90]`, and 36 of them straddle the equator, where SpatiaLite's ellipsoid area bails out with `ptarray_area_spheroid: cannot handle ptarray that crosses equator`. Clipping happens once at setup so it stays outside the timed query, and both libraries measure the same rows. `ST_LengthSpheroid` runs over the exterior rings of that same clipped set.

| Workload | sqlitegis | SpatiaLite | Ratio |
| --- | ---: | ---: | --- |
| `ST_Intersects` bulk, unindexed | `5.53 ms` | `8.95 ms` | `sqlitegis 1.62x` |
| `ST_Intersects` window, R-tree-prefiltered | `10.69 us` | `13.10 us` | `sqlitegis 1.23x` |
| `ST_Contains` bulk, unindexed | `5.63 ms` | `8.97 ms` | `sqlitegis 1.59x` |
| `ST_Contains` window, R-tree-prefiltered | `10.83 us` | `13.07 us` | `sqlitegis 1.21x` |
| `ST_Covers` bulk, unindexed | `5.48 ms` | `8.48 ms` | `sqlitegis 1.55x` |
| `ST_Touches` bulk, unindexed | `6.97 ms` | `10.81 ms` | `sqlitegis 1.55x` |
| `ST_Overlaps` bulk, unindexed | `7.12 ms` | `10.88 ms` | `sqlitegis 1.53x` |
| `ST_Equals` bulk, unindexed | `7.02 ms` | `11.74 ms` | `sqlitegis 1.67x` |
| `ST_DWithin` bulk, unindexed | `31.10 ms` | `39.07 ms` | `sqlitegis 1.26x` |
| `ST_DistanceSphere` bulk | `33.14 ms` | `251.17 ms` | `sqlitegis 7.58x` |
| `ST_DistanceSpheroid` bulk | `90.19 ms` | `317.87 ms` | `sqlitegis 3.52x` |
| `ST_DWithinSphere` bulk, unindexed | `32.77 ms` | `252.27 ms` | `sqlitegis 7.70x` |
| `ST_DWithinSpheroid` bulk, unindexed | `88.86 ms` | `308.07 ms` | `sqlitegis 3.47x` |
| `ST_LengthSpheroid` bulk | `126.50 ms` | `309.29 ms` | `sqlitegis 2.44x` |
| `ST_AreaSphere` bulk | `140.49 ms` | `304.15 ms` | `sqlitegis 2.16x` |
| `ST_AreaSpheroid` bulk | `159.42 ms` | `306.42 ms` | `sqlitegis 1.92x` |
| `ST_PerimeterSphere` bulk | `47.38 ms` | `208.56 ms` | `sqlitegis 4.40x` |
| `ST_PerimeterSpheroid` bulk | `159.01 ms` | `307.23 ms` | `sqlitegis 1.93x` |
| `ST_Distance` planar bulk | `31.89 ms` | `35.33 ms` | `sqlitegis 1.11x` |
| `ST_AsText` scalar throughput | `29.14 ms` | `47.27 ms` | `sqlitegis 1.62x` |
| `ST_AsGeoJSON` serialize throughput | `31.95 ms` | `61.35 ms` | `sqlitegis 1.92x` |
| `ST_AsBinary` serialize throughput | `28.30 ms` | `6.69 ms` | `SpatiaLite 4.23x` |
| `ST_GeomFromText` parse throughput | `1.58 ms` | `3.08 ms` | `sqlitegis 1.95x` |
| `ST_GeomFromWKB` parse throughput | `1.62 ms` | `3.23 ms` | `sqlitegis 2.00x` |
| `ST_Buffer` scalar throughput | `327.63 ms` | `699.69 ms` | `sqlitegis 2.14x` |
| `ST_Buffer` + `ST_Intersection` bulk | `37.60 ms` | `28.00 ms` | `SpatiaLite 1.34x` |
| `ST_Centroid` scalar throughput | `55.60 ms` | `36.28 ms` | `SpatiaLite 1.53x` |
| `ST_Envelope` scalar throughput | `117.07 ms` | `33.37 ms` | `SpatiaLite 3.51x` |
| `ST_Area` sum | `41.78 ms` | `19.91 ms` | `SpatiaLite 2.10x` |
| `ST_Perimeter` sum | `42.04 ms` | `18.88 ms` | `SpatiaLite 2.23x` |
| `ST_X` sum | `15.99 ms` | `4.42 ms` | `SpatiaLite 3.62x` |
| `ST_Y` sum | `15.88 ms` | `4.34 ms` | `SpatiaLite 3.66x` |
| `ST_Difference` disjoint bulk | `261.53 ms` | `218.40 ms` | `SpatiaLite 1.20x` |
| `ST_Difference` overlapping bulk | `171.82 ms` | `319.79 ms` | `sqlitegis 1.86x` |
| `ST_Union` disjoint bulk | `101.66 ms` | `81.37 ms` | `SpatiaLite 1.25x` |
| `ST_SymDifference` disjoint bulk | `101.74 ms` | `80.54 ms` | `SpatiaLite 1.26x` |

sqlitegis is ahead on 25 of the 36 workloads and behind on 11. The headline patterns:

**Predicate wins.** The binary predicates (`ST_Intersects`, `ST_Contains`, `ST_Covers`, `ST_Touches`, `ST_Overlaps`, `ST_Equals`) win 1.2x to 1.7x via an MBR-only fastpath that walks the EWKB bytes for the bounding rectangle and short-circuits the full geometric test when bboxes cannot satisfy the predicate. On filter-heavy "find features in a window" workloads the negative-row path stops paying for a full decode and runs in ~60 ns instead of a few microseconds per row.

**Curved-earth wins.** All ten curved-earth workloads put sqlitegis ahead, from 1.9x to 7.7x. SpatiaLite's 3-arg `ST_Distance(g1, g2, use_ellipsoid)` pays PROJ-based ellipsoid setup cost even on the sphere branch. sqlitegis uses Haversine on `f64` lat/lon pairs for the sphere variants and `geographiclib-rs` for the ellipsoid ones. `ST_PerimeterSphere` is the widest of the areal measures at 4.40x because a Haversine ring walk is arithmetic only, where the three that solve geodesics land near 2x.

**Set-op wire-level fastpath.** `ST_Union` and `ST_SymDifference` on disjoint inputs splice the two input EWKB blobs into a `MultiPolygon` result without decoding either side, which cuts the SpatiaLite gap to 1.25x. `ST_Difference` overlapping wins 1.86x even on the BooleanOps slow path. `ST_Difference` disjoint loses by 1.20x. Extending the splice trick to "return A unchanged" would close it.

**I/O wins, with one exception.** `ST_GeomFromText` and `ST_GeomFromWKB` parse ~2x faster, `ST_AsText` serialises 1.62x faster, `ST_AsGeoJSON` 1.92x faster. The exception is `ST_AsBinary` at 4.23x slower: today it round-trips through `geo::Geometry` plus geozero serializer even though the conversion from EWKB to ISO WKB is byte-level trivial for XY inputs (strip SRID flag from type word, strip the SRID bytes, copy the rest). Identified optimisation candidate.

**Remaining GEOS-favored gaps.** `ST_Centroid` and `ST_Buffer + ST_Intersection` lose by under 1.6x where decades of GEOS optimisation show up. `ST_Envelope` loses 3.51x for the same reason `ST_AsBinary` does: today goes through full decode + bounding rect + serialize, when `extract_mbr` already walks the EWKB and an MBR-fastpath would build the 5-vertex result polygon by hand. Identified optimisation candidate.

**Surprise scalar losses.** `ST_X`, `ST_Y`, `ST_Area`, `ST_Perimeter` lose 2x to 3.7x on what should be near-trivial header walks. SpatiaLite likely binds these as thin C wrappers that read a few EWKB bytes directly. sqlitegis goes through geozero's full decode path. A header-walk-only fastpath for these (in the spirit of `extract_mbr`) is plausible follow-up work.

## SpatiaLite naming quirks worth knowing

While porting bench queries between the two libraries, the following function-name differences mattered. None of them are sqlitegis bugs. Documented here for anyone porting queries.

- `ST_DistanceSphere(g1, g2)` (PostGIS / sqlitegis) is `ST_Distance(g1, g2, 0)` in SpatiaLite 5.1.0.
- `ST_DistanceSpheroid(g1, g2)` is `ST_Distance(g1, g2, 1)` in SpatiaLite 5.1.0.
- `ST_MakeEnvelope(xmin, ymin, xmax, ymax, srid)` is not present in SpatiaLite 5.1.0. Bench code constructs the envelope as a `POLYGON` WKT literal instead.
- `ST_DWithin(g1, g2, dist)` is not present in SpatiaLite 5.1.0. Bench code rewrites it as `ST_Distance(g1, g2) <= dist`. The same rewrite applies to `ST_DWithinSphere` (`ST_Distance(g1, g2, 0) <= dist`) and `ST_DWithinSpheroid` (`ST_Distance(g1, g2, 1) <= dist`).
- `ST_AsGeoJSON(g)` (PostGIS / sqlitegis) is `AsGeoJSON(g)` (no `ST_` prefix) in SpatiaLite 5.1.0.
- `GreatCircleDistance` was present in SpatiaLite 4.x but removed in 5.x.
- `ST_LengthSpheroid(g)` (sqlitegis) is `GeodesicLength(g)` in SpatiaLite 5.1.0, and `ST_LengthSphere(g)` is `GreatCircleLength(g)`. The two libraries agree to the last digit on the ellipsoid and differ in the eighth significant figure on the sphere, where SpatiaLite uses a slightly different mean radius.
- `ST_AreaSpheroid(g)` and `ST_AreaSphere(g)` are `ST_Area(g, 1)` and `ST_Area(g, 0)` in SpatiaLite 5.1.0. `ST_PerimeterSpheroid(g)` and `ST_PerimeterSphere(g)` are `ST_Perimeter(g, 1)` and `ST_Perimeter(g, 0)`.

## Reproducing

`libsqlite3-mod-spatialite` must be installed system-wide so the SQLite loader can find `mod_spatialite`. Then:

```sh
cargo bench --features "bench-spatialite sqlite bundled-sqlite" --bench spatialite_vs_sqlitegis -- --warm-up-time 2 --measurement-time 6
```

CI does not run this bench. SpatiaLite is not a default CI dep, and the bench is feature-gated off so the rest of the matrix stays unaffected.
