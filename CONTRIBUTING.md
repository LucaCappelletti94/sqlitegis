# Contributing to SQLiteGIS

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

## Supply-chain checks

CI runs two dependency gates that you can reproduce locally. They need network access (to fetch the RustSec advisory database) so they are kept out of the default `prek` suite.

```sh
# RustSec advisories only (CVEs, unsound, unmaintained):
cargo audit

# Full policy: advisories + license allow-list + banned/duplicate crates + sources.
cargo deny check --all-features
```

The cargo-deny policy lives in [`deny.toml`](deny.toml). The license allow-list is intentionally tight (only licenses actually present in the tree), so a dependency update that pulls a new license fails the check and forces a deliberate review.

## Miri (undefined-behavior checks)

CI runs [Miri](https://github.com/rust-lang/miri) over the untrusted-input boundary: the EWKB parser and the accessor/I/O/constructor modules that decode and serialize blobs. Miri cannot interpret the SQLite C library or the inline assembly in `i_overlay` (the geometry-algorithm crate), so it is scoped to those pure byte-level modules with `--no-default-features`.

```sh
rustup +nightly component add miri
MIRIFLAGS=-Zmiri-strict-provenance cargo +nightly miri test --no-default-features --lib core::ewkb
```

## Building the loadable SQLite extension

```sh
cargo build --release -p sqlitegis --features sqlite-extension
```

This produces `target/release/libsqlitegis.{so,dylib,dll}` with `sqlite3_sqlitegis_init` exported. Without the `sqlite-extension` feature the symbol is intentionally NOT exported, so downstream Rust binaries that depend on `sqlitegis` with `features = ["sqlite"]` for in-process registration cannot leak the FFI entry point.

## Documentation

```sh
cargo doc -p sqlitegis --all-features --no-deps --open
```

## Benchmarks

```sh
cargo bench -p sqlitegis --features diesel-sqlite --benches
```

Criterion writes HTML reports under `target/criterion/`. Numbers vary by host and load. See the README for a recent baseline.

## Adding a new spatial function

The catalog in [`src/core/function_catalog.rs`](src/core/function_catalog.rs) is the single source of truth. To add a new function:

1. Add an entry to `SQLITE_DETERMINISTIC_FUNCTIONS` (or `SQLITE_DIRECT_ONLY_FUNCTIONS` for DDL-like helpers).
2. Implement the core function in `src/core/functions/`.
3. Add the SQLite callback wrapper to `src/sqlite/ffi.rs` and the corresponding entry in `src/sqlite/deterministic_callbacks.rs` (or `direct_only_callbacks.rs`).
4. Add the matching `define_sql_function!` block in `src/diesel/functions.rs`.
5. If the first argument is `Nullable<Geometry>`, add the method wrapper in `src/diesel/expression_methods.rs`.

Three parity nets keep these in sync. Failing any of them is a sign that one of the steps above was missed:

- Compile-time `assert_catalog_callback_parity` in `src/sqlite/ffi.rs` checks the SQLite callback arrays against the catalog 1-for-1.
- Three runtime parity tests in `tests/diesel_expression_methods.rs`:
  - `diesel_functions_and_methods_surface_parity`
  - `diesel_sql_functions_are_backed_by_sqlite_catalog`
  - `catalog_functions_are_covered_by_diesel_declarations`

## License

By contributing you agree your contributions will be licensed under the same terms as the project (MIT OR Apache-2.0).
