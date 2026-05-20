//! Target-conditional re-exports that unify `libsqlite3-sys` (native) and
//! `sqlite-wasm-rs` (wasm32) behind a single import path.

#[cfg(not(target_arch = "wasm32"))]
pub use libsqlite3_sys::*;

#[cfg(target_arch = "wasm32")]
pub use sqlite_wasm_rs::*;

#[cfg(not(target_arch = "wasm32"))]
pub fn sqlite_transient() -> sqlite3_destructor_type {
    unsafe { std::mem::transmute(-1_isize) }
}

#[cfg(target_arch = "wasm32")]
pub fn sqlite_transient() -> sqlite3_destructor_type {
    sqlite_wasm_rs::SQLITE_TRANSIENT()
}
