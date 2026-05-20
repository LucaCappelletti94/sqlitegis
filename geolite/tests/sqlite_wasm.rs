#![cfg(target_arch = "wasm32")]
//! Headless WASM integration tests for geolite-sqlite.

use sqlite_wasm_rs::*;
use std::ffi::{CStr, CString};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

include!("sqlite_test_db_macro.rs");
define_test_db!(WasmTestDb);
type ActiveTestDb = WasmTestDb;

include!("support/shared_cases.rs");
define_shared_cases!(wasm_bindgen_test);
