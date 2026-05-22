#![cfg(not(target_arch = "wasm32"))]
//! In-process integration tests for the SQLite registration API.
//!
//! Exercises `sqlitegis::sqlite::register_functions` against a raw
//! `*mut sqlite3` opened via `libsqlite3-sys::sqlite3_open`. The
//! runtime-load test (which validated the cdylib produced by
//! `--features sqlite-extension`) was removed in the move to
//! `sqlite-loadable`: the cdylib now links its own SQLite via
//! `sqlite3ext-sys`, which collides with `libsqlite3-sys` when both
//! end up in the same test binary. The equivalent assertion now lives
//! in the Python wheel CI smoke-test that loads the produced cdylib
//! into a Python whose libsqlite3 differs from the cargo-time link.

use libsqlite3_sys::*;
use std::ffi::{CStr, CString};

include!("sqlite_test_db_macro.rs");
define_test_db!(TestDb);
type ActiveTestDb = TestDb;

include!("support/shared_cases.rs");
define_shared_cases!(test);
