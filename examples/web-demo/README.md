# geolite web demo

A single-page Dioxus app that runs PostGIS-style spatial SQL entirely in the browser, with geolite's functions registered as a SQLite loadable extension via [sqlite-wasm-rs](https://crates.io/crates/sqlite-wasm-rs). Everything (the schema, the dataset, the queries) lives in the page. Nothing hits a server.

## What it shows

- The full geolite SQL surface (`ST_DistanceSphere`, `ST_DWithinSphere`, `ST_Intersects`, `ST_MakeEnvelope`, `ST_AsText`, `CreateSpatialIndex`, and so on) running on top of SQLite compiled to WASM.
- Diesel queries (`diesel::sql_query`, `LoadConnection::load`, typed `QueryableByName` rows) against that same connection.
- 68k cities from the GeoNames `cities5000` dataset loaded into an in-memory SQLite database and indexed via geolite's R-tree shadow table.
- A canvas map of every city, with rows from the last query highlighted in orange and your browser-reported position as a green disc.

## Prerequisites

- Rust toolchain (matches the main workspace). Add the WASM target once with `rustup target add wasm32-unknown-unknown`.
- The Dioxus CLI:
  ```sh
  cargo install dioxus-cli --locked
  ```

## Dev run

```sh
cd examples/web-demo
dx serve --platform web
```

Open the URL printed in the terminal (typically `http://localhost:8080`). The first load fetches and inserts the dataset, which takes a few seconds and shows a progress counter.

## Static bundle (for deploying to GitHub Pages, Netlify, etc.)

```sh
dx bundle --platform web --release
```

Output lands in `dist/`. Serve it with any static file server. The page is self-contained.

## How it's wired

- `src/db.rs`. `Once::call_once` plus `sqlite_wasm_rs::sqlite3_auto_extension` registers `geolite_sqlite::register_functions` for every new connection, then opens a single `SqliteConnection::establish(":memory:")` shared across the page.
- `src/loader.rs`. Fetches `public/cities5000.tsv` via gloo-net, batched INSERTs (1k rows per batch) wrapped in one transaction. Geometry is produced server-side via `ST_Point(lon, lat, 4326)` so the browser never builds EWKB itself.
- `src/runner.rs`. Executes user-typed SQL through Diesel's `LoadConnection`, iterates the resulting cursor with `Row::field_count` and `SqliteValue::value_type` to read any column shape into displayable strings.
- `src/viz.rs`. Equirectangular projection on a `<canvas>`. Subscribes to the city set, the highlighted set, and the user position. Redraws on any change.

## Data

The bundled `public/cities5000.tsv` is a slim projection of [GeoNames `cities5000`](https://download.geonames.org/export/dump/) with name, country code, latitude, longitude, and population. GeoNames data is licensed under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

## Caveats

- The DB is in-memory. Refresh the page and the data reloads from scratch. Persistence via OPFS would require [`sqlite-wasm-vfs`](https://crates.io/crates/sqlite-wasm-vfs) and is out of scope here.
- Geolocation falls back to Berlin if the browser denies or the API is unavailable.
- This crate is intentionally **not** a member of the main workspace. Dioxus drags in hundreds of transitive crates that would slow down workspace precommit and `cargo audit`. The geolite path deps still point at the live source under `../`.
