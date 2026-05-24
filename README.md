# SQLiteGIS

[![CI](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sqlitegis/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sqlitegis)
[![crates.io](https://img.shields.io/crates/v/sqlitegis.svg)](https://crates.io/crates/sqlitegis)
[![docs.rs](https://img.shields.io/docsrs/sqlitegis)](https://docs.rs/sqlitegis)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](https://github.com/LucaCappelletti94/sqlitegis)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/LucaCappelletti94/sqlitegis/blob/main/LICENSE)

[PostGIS](https://postgis.net/)-style spatial functions for [SQLite](https://www.sqlite.org/) in pure [Rust](https://www.rust-lang.org/), primarily a [Diesel](https://diesel.rs/) ORM integration. Geometries travel as [EWKB](https://en.wikipedia.org/wiki/Well-known_text_representation_of_geometry#Well-known_binary) BLOBs, matching the PostGIS wire format so queries port between SQLite and PostGIS without rewriting. The same functions are also exposed as a SQLite loadable extension (native and [WebAssembly](https://webassembly.org/)) for non-Rust consumers like the SQLite CLI or [Datasette](https://datasette.io/).

## Quick start (Diesel)

```rust
use diesel::prelude::*;
use sqlitegis::diesel::functions::st_point;
use sqlitegis::diesel::prelude::*;

// Register the spatial functions on every new SqliteConnection.
sqlitegis::sqlite::register_on_every_new_connection();

diesel::table! {
    features (id) {
        id -> Integer,
        geom -> Nullable<sqlitegis::diesel::Geometry>,
    }
}

let mut conn = SqliteConnection::establish(":memory:").unwrap();
let nearby = features::table
    .filter(features::geom.st_dwithin(st_point(13.4, 52.5).nullable(), 1000.0).eq(true))
    .select(features::geom.st_astext());
```

`CreateSpatialIndex` and `DropSpatialIndex` are DDL helpers without typed wrappers, called through `diesel::sql_query`. [R-tree](https://en.wikipedia.org/wiki/R-tree)-backed queries run 50 to 60x faster than the non-indexed equivalents (see Benchmarks).

## Without Diesel: pure-Rust geometry

If you only need the geometry algebra without SQL, the core functions are callable from regular Rust without any database at all.

```rust
use sqlitegis::core::functions::constructors::st_point;
use sqlitegis::core::functions::measurement::st_distance;

let a = st_point(0.0, 0.0, None).unwrap();
let b = st_point(3.0, 4.0, None).unwrap();
assert!((st_distance(&a, &b).unwrap() - 5.0).abs() < 1e-10);
```

## As a SQLite loadable extension

For non-Rust consumers (SQLite CLI, Datasette, the WebAssembly browser path) the same functions are available as a `load_extension`-style cdylib. Build it yourself with the `sqlite-extension` feature.

```sh
cargo build --release -p sqlitegis --features sqlite-extension
```

```sql
SELECT load_extension('./target/release/libsqlitegis');
SELECT ST_AsText(ST_Buffer(ST_Point(0, 0), 1.0));
SELECT ST_Distance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'));
```

## Notes

Geodesic functions (`ST_DistanceSphere`, `ST_DistanceSpheroid`, `ST_LengthSphere`, `ST_Azimuth`, `ST_Project`, `ST_DWithinSphere`, `ST_DWithinSpheroid`) require `SRID=4326` non-empty Point inputs and reject anything else. `ST_GeomFromGeoJSON` defaults to `SRID=4326`. `ST_DWithin*` predicates require a finite, non-negative distance.

## Benchmarks

[Criterion](https://github.com/bheisler/criterion.rs) central estimates on the included R-tree workloads:

| Scenario | Indexed | Non-indexed | Speedup |
| --- | ---: | ---: | ---: |
| `intersects_window` | `178 us` | `9.81 ms` | `~55x` |
| `knn` | `89 us` | `5.66 ms` | `~64x` |

### vs SpatiaLite

[SpatiaLite](https://www.gaia-gis.it/fossil/libspatialite/index) is the long-established C extension that adds PostGIS-style spatial functions to SQLite, built on top of [GEOS](https://libgeos.org/) (the C++ port of the JTS computational-geometry suite) and [PROJ](https://proj.org/) (the standard coordinate-reprojection library). It is the closest existing analogue to sqlitegis and the natural baseline to measure against.

SpatiaLite would have been the obvious choice except for two practical issues. Its C/C++/GEOS/PROJ build chain is a recurring source of friction (a substantial native build with several optional system libraries), and it does not run cleanly on WebAssembly or edge devices. A pure Rust crate compiles to those targets without extra tooling and pulls no transitive C dependencies. That is the gap sqlitegis fills.

`benches/spatialite_vs_sqlitegis.rs` (gated behind the `bench-spatialite` Cargo feature, requires `libsqlite3-mod-spatialite` installed system-wide) runs the same 50k WGS84-point dataset through both libraries on the same in-process libsqlite3, so the comparison isolates per-callback cost from engine differences.

| Workload | sqlitegis | SpatiaLite | Ratio |
| --- | ---: | ---: | --- |
| `ST_Intersects` bulk, unindexed | `5.79 ms` | `9.63 ms` | `sqlitegis 1.66x` |
| `ST_Intersects` window, R-tree-prefiltered | `10.60 us` | `12.85 us` | `sqlitegis 1.21x` |
| `ST_DistanceSphere` bulk | `31.25 ms` | `255.74 ms` | `sqlitegis 8.18x` |
| `ST_AsText` scalar throughput | `28.67 ms` | `50.56 ms` | `sqlitegis 1.76x` |
| `ST_Buffer` + `ST_Intersection` bulk | `208.95 ms` | `29.16 ms` | `SpatiaLite 7.16x` |

sqlitegis leads four of five workloads. The `ST_Buffer` + `ST_Intersection` gap is the GEOS edge. The `geo` crate's offset-curve and boolean ops are correct but slower than GEOS's polygon-clipping. The geodesic margin comes from SpatiaLite's 3-arg `ST_Distance(g1, g2, use_ellipsoid)` paying ellipsoid setup cost on the sphere branch too, while `ST_DistanceSphere` is a direct Haversine on `f64`.

Reproduce with:

```sh
cargo bench --features "bench-spatialite sqlite bundled-sqlite" --bench spatialite_vs_sqlitegis -- --warm-up-time 2 --measurement-time 6
```

## Contributing

See [CONTRIBUTING.md](https://github.com/LucaCappelletti94/sqlitegis/blob/main/CONTRIBUTING.md).

## License

MIT OR Apache-2.0
