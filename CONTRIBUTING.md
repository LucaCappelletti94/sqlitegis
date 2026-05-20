# Contributing to geolite

## Local checks

Local checks are driven by [`prek`](https://github.com/j178/prek), a Rust reimplementation of the `pre-commit` framework. Configuration lives in `prek.toml` at the repo root.

```sh
# Once, per checkout:
prek install

# Run the same checks CI runs (fmt + clippy + workspace tests + doctests):
prek run --all-files

# Run the expensive hooks (Postgres via testcontainers, WASM target):
prek run --stage manual --all-files
```

`prek install` writes a Git `pre-commit` hook that runs the default suite on every commit. The `manual`-stage hooks are gated out of the default run because they require Docker (for the Postgres testcontainer) and a `wasm32-unknown-unknown` target.

## Building the loadable SQLite extension

```sh
cargo build --release -p geolite --features sqlite-extension
```

This produces `target/release/libgeolite.{so,dylib,dll}` with `sqlite3_geolite_init` exported. Without the `sqlite-extension` feature the symbol is intentionally NOT exported, so downstream Rust binaries that depend on `geolite` with `features = ["sqlite"]` for in-process registration cannot leak the FFI entry point.

## Documentation

```sh
cargo doc -p geolite --all-features --no-deps --open
```

## Benchmarks

```sh
cargo bench -p geolite --features diesel-sqlite --benches
```

Criterion writes HTML reports under `target/criterion/`. Numbers vary by host and load; see the README for a recent baseline.

## Adding a new spatial function

The catalog in [`geolite/src/core/function_catalog.rs`](geolite/src/core/function_catalog.rs) is the single source of truth. To add a new function:

1. Add an entry to `SQLITE_DETERMINISTIC_FUNCTIONS` (or `SQLITE_DIRECT_ONLY_FUNCTIONS` for DDL-like helpers).
2. Implement the core function in `geolite/src/core/functions/`.
3. Add the SQLite callback wrapper to `geolite/src/sqlite/ffi.rs` and the corresponding entry in `geolite/src/sqlite/deterministic_callbacks.rs` (or `direct_only_callbacks.rs`).
4. Add the matching `define_sql_function!` block in `geolite/src/diesel/functions.rs`.
5. If the first argument is `Nullable<Geometry>`, add the method wrapper in `geolite/src/diesel/expression_methods.rs`.

Three parity nets keep these in sync; failing any of them is a sign that one of the steps above was missed:

- Compile-time `assert_catalog_callback_parity` in `geolite/src/sqlite/ffi.rs` checks the SQLite callback arrays against the catalog 1-for-1.
- Three runtime parity tests in `geolite/tests/diesel_expression_methods.rs`:
  - `diesel_functions_and_methods_surface_parity`
  - `diesel_sql_functions_are_backed_by_sqlite_catalog`
  - `catalog_functions_are_covered_by_diesel_declarations`

## License

By contributing you agree your contributions will be licensed under the same terms as the project (MIT OR Apache-2.0).
