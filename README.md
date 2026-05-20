# geolite

[![CI](https://github.com/LucaCappelletti94/geolite/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/geolite/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/geolite/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/geolite)
[![MSRV](https://img.shields.io/badge/MSRV-1.86-blue)](https://github.com/LucaCappelletti94/geolite)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/LucaCappelletti94/geolite/blob/main/LICENSE)

PostGIS-style spatial functions for SQLite in pure Rust. Ships as a SQLite loadable extension (native + WASM) and as a Diesel ORM integration. Geometries are stored as EWKB BLOBs, matching the PostGIS wire format. Everything lives in a single crate behind feature flags.

## Features

| Feature | What it adds |
| --- | --- |
| `default = ["diesel-sqlite"]` | Diesel + SQLite, the most common usage |
| `sqlite` | In-process registration API (`register_functions`). Pulls libsqlite3-sys on native, sqlite-wasm-rs on wasm32 |
| `sqlite-extension` | Adds the `#[no_mangle]` C entry point so the cdylib build is loadable via SQLite's `load_extension` |
| `diesel` | Backend-agnostic Diesel types (`Geometry`, `Geography`) and the `GeometryExpressionMethods` trait |
| `diesel-sqlite` | Implies `diesel` and `sqlite`; adds Diesel's SQLite backend |
| `diesel-postgres` | Implies `diesel`; adds Diesel's Postgres backend for use against PostGIS |

```toml
[dependencies]
geolite = "0.2"                                       # default: diesel-sqlite
geolite = { version = "0.2", default-features = false } # pure-Rust geometry only
geolite = { version = "0.2", features = ["sqlite-extension"] } # for the loadable .so
```

## SQLite extension

```sh
cargo build --release -p geolite --features sqlite-extension
```

```sql
SELECT load_extension('./target/release/libgeolite');
SELECT ST_AsText(ST_Buffer(ST_Point(0, 0), 1.0));
SELECT ST_Distance(ST_GeomFromText('POINT(0 0)'), ST_GeomFromText('POINT(3 4)'));
```

## Pure-Rust geometry

```rust
use geolite::core::functions::constructors::st_point;
use geolite::core::functions::measurement::st_distance;

let a = st_point(0.0, 0.0, None).unwrap();
let b = st_point(3.0, 4.0, None).unwrap();
assert!((st_distance(&a, &b).unwrap() - 5.0).abs() < 1e-10);
```

## Diesel integration

```rust
# #[cfg(feature = "diesel-sqlite")]
# {
use diesel::debug_query;
use diesel::prelude::*;
use diesel::sqlite::Sqlite;
use geolite::diesel::functions::st_point;
use geolite::diesel::prelude::*;

diesel::table! {
    features (id) {
        id -> Integer,
        geom -> Nullable<geolite::diesel::Geometry>,
    }
}

let query = features::table
    .filter(
        features::geom
            .st_dwithin(st_point(13.4, 52.5).nullable(), 1000.0)
            .eq(true),
    )
    .select(features::geom.st_astext());

let sql = debug_query::<Sqlite, _>(&query).to_string().to_lowercase();
assert!(sql.contains("st_dwithin"));
# }
```

`CreateSpatialIndex` and `DropSpatialIndex` are called via raw SQL (`diesel::sql_query`). They are DDL helpers and don't have typed wrappers. Both lifecycle calls fail closed when the `geolite_spatial_index_catalog` ownership table is out of sync with live R-tree objects. Prefer SQL migrations for setup and teardown. Indexed queries are roughly 50 to 60x faster than non-indexed in our benches. Run `cargo bench -p geolite --features diesel-sqlite` to measure locally.

## Geographic functions

Geodesic and spherical functions (`ST_DistanceSphere`, `ST_DistanceSpheroid`, `ST_LengthSphere`, `ST_Azimuth`, `ST_Project`, `ST_DWithinSphere`, `ST_DWithinSpheroid`) require `SRID=4326` non-empty Point inputs. Everything else is rejected with an explicit error. `ST_GeomFromGeoJSON` defaults to `SRID=4326` when none is given. Wrap in `ST_SetSRID` to override. `ST_DWithin*` predicates require a finite, non-negative distance.

## Benchmarks

Criterion central estimates, measured 2026-03-05:

| Scenario | Indexed (ORM + R-tree join) | Non-indexed (ORM) | Approx speedup |
| --- | ---: | ---: | ---: |
| `intersects_window` | `156.43 us` | `9.3577 ms` | `~59.8x` |
| `knn` | `84.271 us` | `5.4050 ms` | `~64.1x` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the local-checks setup (`prek`), how to add a new spatial function, and how to run the benchmark and doc commands locally.

## License

MIT OR Apache-2.0
