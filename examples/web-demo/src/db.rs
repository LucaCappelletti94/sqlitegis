//! In-memory SQLite connection wired with the geolite extension.
//!
//! Mirrors the pattern used in `geolite-diesel/tests/wasm_integration.rs`:
//! register `geolite_init` once via `sqlite3_auto_extension`, then every
//! `SqliteConnection::establish(":memory:")` call gets geolite's functions.

use std::cell::RefCell;
use std::sync::Once;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;

static INIT: Once = Once::new();

unsafe extern "C" fn geolite_init(
    db: *mut sqlite_wasm_rs::sqlite3,
    _pz_err_msg: *mut *mut std::ffi::c_char,
    _p_api: *const sqlite_wasm_rs::sqlite3_api_routines,
) -> std::ffi::c_int {
    geolite_sqlite::register_functions(db)
}

thread_local! {
    static CONN: RefCell<Option<SqliteConnection>> = const { RefCell::new(None) };
}

fn ensure_auto_extension() {
    INIT.call_once(|| unsafe {
        sqlite_wasm_rs::sqlite3_auto_extension(Some(geolite_init));
    });
}

/// Open a fresh in-memory database, registering geolite via auto-extension.
/// Replaces any previously open connection.
pub fn reopen() -> Result<(), String> {
    ensure_auto_extension();
    let conn = SqliteConnection::establish(":memory:").map_err(|e| e.to_string())?;
    CONN.with(|cell| *cell.borrow_mut() = Some(conn));
    Ok(())
}

/// Borrow the live connection for one operation.
///
/// Panics if `reopen` hasn't been called first.
pub fn with_conn<R>(f: impl FnOnce(&mut SqliteConnection) -> R) -> R {
    CONN.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let conn = borrow
            .as_mut()
            .expect("db::reopen must be called before with_conn");
        f(conn)
    })
}

/// Execute a free-form SQL script (multiple statements, separated by `;`).
pub fn run_script(sql: &str) -> Result<(), String> {
    with_conn(|c| c.batch_execute(sql).map_err(|e| e.to_string()))
}
