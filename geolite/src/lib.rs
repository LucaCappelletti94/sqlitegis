#![doc = include_str!("../../README.md")]
//! # Crate layout
//!
//! Modules are gated behind features so consumers only pay for what they
//! ask for. See the `[features]` table in Cargo.toml for the full list;
//! in short:
//!
//! - `core` is always available (pure-Rust geometry, EWKB I/O, function
//!   catalog, no SQLite or Diesel deps).
//! - `sqlite` adds [`crate::sqlite::register_functions`] for in-process
//!   registration against a `*mut sqlite3` connection.
//! - `sqlite-extension` further adds the `#[no_mangle]` C entry points so
//!   the cdylib build is loadable via SQLite's `load_extension`.
//! - `diesel` adds backend-agnostic types
//!   ([`Geometry`](crate::diesel::Geometry),
//!   [`Geography`](crate::diesel::Geography)) plus
//!   [`GeometryExpressionMethods`](crate::diesel::GeometryExpressionMethods).
//! - `diesel-sqlite` / `diesel-postgres` add the backend-specific impls.
//!
//! Diesel users typically import via the prelude:
//! `use geolite::prelude::*;` (re-exported from
//! [`crate::diesel::prelude`]).

pub mod core;
pub use core::error::{GeoLiteError, Result};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "diesel")]
pub mod diesel;

#[cfg(feature = "diesel")]
pub use diesel::prelude;
