//! SQLite integration. Available under `feature = "sqlite"` for in-process
//! registration against a raw `*mut sqlite3`, and under
//! `feature = "sqlite-extension"` for the `#[no_mangle]` C entry points
//! that make the cdylib loadable via SQLite's `load_extension`.

mod ffi;
mod sqlite_compat;

pub use ffi::register_functions;
