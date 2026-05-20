//! # geolite
//!
//! PostGIS-style spatial functions for SQLite in pure Rust, plus a
//! first-class Diesel ORM integration.
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
//! - `diesel` adds backend-agnostic types ([`Geometry`], [`Geography`])
//!   plus [`GeometryExpressionMethods`].
//! - `diesel-sqlite` / `diesel-postgres` add the backend-specific impls.

pub mod core;
pub use core::error::{GeoLiteError, Result};

#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "diesel")]
pub mod diesel;

#[cfg(feature = "diesel")]
pub use diesel::prelude;

#[cfg(feature = "diesel")]
pub use diesel::{Geography, Geometry, GeometryExpressionMethods};
