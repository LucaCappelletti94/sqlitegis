//! Target-conditional re-exports that unify `libsqlite3-sys` (native) and
//! `sqlite-wasm-rs` (wasm32) behind a single import path.

#[cfg(not(target_arch = "wasm32"))]
pub use libsqlite3_sys::*;

#[cfg(target_arch = "wasm32")]
pub use sqlite_wasm_rs::*;

/// The `SQLITE_TRANSIENT` destructor sentinel (`(sqlite3_destructor_type)-1`).
///
/// Passing it to `sqlite3_result_blob/_text` tells SQLite to copy the buffer
/// immediately, so the Rust-owned source can be freed as soon as the setter
/// returns. `libsqlite3-sys` does not expose a typed constant for it, so we
/// build it from the documented integer value.
#[cfg(not(target_arch = "wasm32"))]
pub fn sqlite_transient() -> sqlite3_destructor_type {
    // SAFETY: SQLITE_TRANSIENT is defined by SQLite as the function-pointer
    // value -1. `sqlite3_destructor_type` is `Option<unsafe extern "C" fn(...)>`,
    // a nullable function pointer whose niche makes any non-zero bit pattern a
    // valid `Some`, so transmuting the all-ones `isize` is sound and matches
    // the C macro the SQLite API expects here.
    unsafe { std::mem::transmute(-1_isize) }
}

#[cfg(target_arch = "wasm32")]
pub fn sqlite_transient() -> sqlite3_destructor_type {
    sqlite_wasm_rs::SQLITE_TRANSIENT()
}
