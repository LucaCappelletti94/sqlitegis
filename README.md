# SQLiteGIS

[![CI](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sqlitegis/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sqlitegis)
[![crates.io](https://img.shields.io/crates/v/sqlitegis.svg)](https://crates.io/crates/sqlitegis)
[![docs.rs](https://img.shields.io/docsrs/sqlitegis)](https://docs.rs/sqlitegis)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](https://github.com/LucaCappelletti94/sqlitegis)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/LucaCappelletti94/sqlitegis/blob/main/LICENSE)

[PostGIS](https://postgis.net/)-style spatial functions for [SQLite](https://www.sqlite.org/) in pure [Rust](https://www.rust-lang.org/), primarily a [Diesel](https://diesel.rs/) ORM integration. Geometries travel as [EWKB](https://en.wikipedia.org/wiki/Well-known_text_representation_of_geometry#Well-known_binary) BLOBs, matching the PostGIS wire format so queries port between SQLite and PostGIS without rewriting. The same functions are also exposed as a SQLite loadable extension (native and [WebAssembly](https://webassembly.org/)) for non-Rust consumers like the SQLite CLI.

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

See [BENCHMARKS.md](https://github.com/LucaCappelletti94/sqlitegis/blob/main/BENCHMARKS.md) for the full R-tree and SpatiaLite comparison reports. Headline: on a 50k-row dataset across 31 head-to-head workloads, sqlitegis wins 20 (geodesic family 3.7x to 8.6x faster, binary predicates 1.2x to 1.7x via an MBR-only fastpath, I/O parse paths 2x faster) and loses 9 (`ST_Envelope`, `ST_AsBinary`, and the per-row scalar accessors `ST_X`/`ST_Y`/`ST_Area`/`ST_Perimeter` go through full EWKB decode where SpatiaLite has thin-C-wrapper shortcuts).

## Contributing

See [CONTRIBUTING.md](https://github.com/LucaCappelletti94/sqlitegis/blob/main/CONTRIBUTING.md).

## License

MIT OR Apache-2.0
