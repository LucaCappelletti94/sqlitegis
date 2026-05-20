#![cfg(all(feature = "diesel-sqlite", target_arch = "wasm32"))]
//! WASM integration tests for the Diesel integration.
//!
//! Same test logic as the native `sqlite_integration.rs`, but uses
//! `sqlite-wasm-rs` for auto-extension and `wasm_bindgen_test` as the
//! test runner. Skips timing-based performance tests.

use std::sync::Once;

use diesel::prelude::*;

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

// Auto-extension registration

static INIT: Once = Once::new();

/// Entry point called by SQLite for each new connection.
unsafe extern "C" fn sqlitegis_init(
    db: *mut sqlite_wasm_rs::sqlite3,
    _pz_err_msg: *mut *mut std::ffi::c_char,
    _p_api: *const sqlite_wasm_rs::sqlite3_api_routines,
) -> std::ffi::c_int {
    sqlitegis::sqlite::register_functions(db)
}

fn conn() -> SqliteConnection {
    INIT.call_once(|| unsafe {
        sqlite_wasm_rs::sqlite3_auto_extension(Some(sqlitegis_init));
    });
    SqliteConnection::establish(":memory:").unwrap()
}

// Shared test definitions

include!("diesel_test_helpers.rs");
define_diesel_sqlite_tests!(wasm_bindgen_test);
