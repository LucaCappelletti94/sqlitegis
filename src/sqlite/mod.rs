//! SQLite integration.
//!
//! Two independent paths exist, gated by separate Cargo features:
//!
//! - `sqlite` enables the in-process registration API on a raw `*mut
//!   sqlite3` handle via direct `libsqlite3-sys` calls. Used by Diesel.
//! - `sqlite-extension` enables the `sqlite3_sqlitegis_init` C entry point
//!   for `SELECT load_extension(...)`. Built on `sqlite-loadable`, so the
//!   produced cdylib routes every SQLite call through the host's
//!   `sqlite3_api_routines` table at runtime and has no link-time
//!   dependency on a specific libsqlite3.
//!
//! Enable both when one binary needs both paths (the in-process Diesel
//! integration test plus the cdylib it loads).

#[cfg(feature = "sqlite")]
mod ffi;
#[cfg(feature = "sqlite")]
mod sqlite_compat;

#[cfg(feature = "sqlite")]
pub use ffi::{register_functions, register_on_every_new_connection};

// The loadable module must be reachable from the crate root for the cdylib
// linker to keep the `#[no_mangle] sqlite3_sqlitegis_init` symbol in the
// exported table; a `pub` re-export of the entry point is the simplest way.
#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
pub mod loadable;
#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
pub use loadable::sqlite3_sqlitegis_init;
