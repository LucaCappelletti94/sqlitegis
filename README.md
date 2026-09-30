# SQLiteGIS

[![CI](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml/badge.svg)](https://github.com/LucaCappelletti94/sqlitegis/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/LucaCappelletti94/sqlitegis/graph/badge.svg)](https://codecov.io/gh/LucaCappelletti94/sqlitegis)
[![crates.io](https://img.shields.io/crates/v/sqlitegis.svg)](https://crates.io/crates/sqlitegis)
[![docs.rs](https://img.shields.io/docsrs/sqlitegis)](https://docs.rs/sqlitegis)
[![MSRV](https://img.shields.io/badge/MSRV-1.88-blue)](https://github.com/LucaCappelletti94/sqlitegis)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/LucaCappelletti94/sqlitegis/blob/main/LICENSE)

[PostGIS](https://postgis.net/)-style spatial functions for [SQLite](https://www.sqlite.org/) in pure [Rust](https://www.rust-lang.org/), primarily a [Diesel](https://diesel.rs/) ORM integration. Geometries travel as [EWKB](https://en.wikipedia.org/wiki/Well-known_text_representation_of_geometry#Well-known_binary) BLOBs, matching the PostGIS wire format so queries port between SQLite and PostGIS without rewriting. The same functions are also exposed as a SQLite loadable extension (native and [WebAssembly](https://webassembly.org/)) for non-Rust consumers like the SQLite CLI. Try the [live demo](https://sqlitegis.luca.phd/) to run spatial SQL against 68k cities entirely in the browser.

## Quick start (Diesel)

A bare dependency is geometry only. The Diesel layer needs the `diesel-sqlite` feature (or `diesel-postgres`), and in-process registration alone needs `sqlite`.

```sh
cargo add sqlitegis --features diesel-sqlite
```

```rust
# #[cfg(not(feature = "diesel-sqlite"))]
# fn main() {}
# #[cfg(feature = "diesel-sqlite")]
# fn main() {
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
# }
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

Geodesic functions require `SRID=4326` and reject any other shape. `ST_DistanceSphere`, `ST_DistanceSpheroid`, `ST_DWithinSphere`, `ST_DWithinSpheroid`, `ST_Azimuth` and `ST_Project` take non-empty Points. `ST_LengthSphere` and `ST_LengthSpheroid` take a LineString or MultiLineString. `ST_AreaSphere`, `ST_AreaSpheroid`, `ST_PerimeterSphere` and `ST_PerimeterSpheroid` take a Polygon or MultiPolygon. The `Spheroid` variants measure on the WGS84 ellipsoid (Karney) and agree with PostGIS `geography` to the last few digits, the `Sphere` variants on a sphere of the mean earth radius. `ST_GeomFromGeoJSON` defaults to `SRID=4326`. `ST_DWithin*` predicates require a finite, non-negative distance.

Every measure and transform that depends on the shape of the earth comes in three forms: a plain name that works in the units of the CRS, a `Sphere` suffix that measures in metres on a sphere of the mean earth radius, and a `Spheroid` suffix that measures in metres on the WGS84 ellipsoid. That covers `ST_Length`, `ST_Area`, `ST_Perimeter`, `ST_Segmentize`, `ST_LineInterpolatePoint`, `ST_LineInterpolatePoints` and `ST_LineSubstring`. The linear-referencing three take a single LineString and a fraction in `[0, 1]`, and `ST_LineSubstring` collapses to a Point when both fractions are equal, matching PostGIS. `ST_Segmentize*` takes a LineString, MultiLineString, Polygon or MultiPolygon. `ST_Length2DSpheroid` is an alias of `ST_LengthSpheroid`, since this crate is 2D throughout.

## Benchmarks

See [BENCHMARKS.md](https://github.com/LucaCappelletti94/sqlitegis/blob/main/BENCHMARKS.md) for the full R-tree and SpatiaLite comparison reports. Headline: on a 50k-row dataset across 36 head-to-head workloads, sqlitegis wins 25 (curved-earth family 1.9x to 7.7x faster, binary predicates 1.2x to 1.7x via an MBR-only fastpath, I/O parse paths 2x faster) and loses 11 (`ST_Envelope`, `ST_AsBinary`, and the per-row scalar accessors `ST_X`/`ST_Y`/`ST_Area`/`ST_Perimeter` go through full EWKB decode where SpatiaLite has thin-C-wrapper shortcuts).

## Contributing

See [CONTRIBUTING.md](https://github.com/LucaCappelletti94/sqlitegis/blob/main/CONTRIBUTING.md).

## License

MIT OR Apache-2.0
