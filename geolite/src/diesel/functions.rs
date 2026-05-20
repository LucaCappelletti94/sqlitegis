//! Diesel SQL function definitions for spatial operations.
//!
//! Most declarations in this module are generated from the canonical function
//! catalog via `cargo run -p xtask -- gen-function-surfaces`.
//!
//! Import the functions you need and use them directly in Diesel query builder
//! expressions.
//!
//! # Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::prelude::*;
//! use diesel::sqlite::Sqlite;
//! use diesel::NullableExpressionMethods;
//! use geolite::diesel::functions::*;
//!
//! diesel::table! {
//!     features (id) {
//!         id -> Integer,
//!         geom -> Nullable<geolite::diesel::Geometry>,
//!     }
//! }
//!
//! let query = features::table
//!     .filter(st_dwithin(features::geom, st_point(13.4, 52.5).nullable(), 1000.0).eq(true))
//!     .select(features::geom);
//! let sql = debug_query::<Sqlite, _>(&query).to_string().to_lowercase();
//! assert!(sql.contains("st_dwithin"));
//! # }
//! ```
//!
//! The relate aliases are available in free-function form:
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::dsl::select;
//! use diesel::sqlite::Sqlite;
//! use geolite::diesel::functions::*;
//!
//! let a = st_geomfromtext("POINT(1 1)");
//! let b = st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))");
//! let pattern = "T*****FF*";
//!
//! // Alias for ST_Relate(a, b, pattern)
//! let via_geoms = select(st_relate_match_geoms(a, b, pattern));
//! let geoms_sql = debug_query::<Sqlite, _>(&via_geoms).to_string().to_lowercase();
//! assert!(geoms_sql.contains("st_relate"));
//!
//! // Alias for ST_RelateMatch(matrix, pattern)
//! let via_matrix = select(st_relate_match("T********", pattern));
//! let matrix_sql = debug_query::<Sqlite, _>(&via_matrix).to_string().to_lowercase();
//! assert!(matrix_sql.contains("st_relatematch"));
//! # }
//! ```
//!
//! # Spatial index lifecycle is raw SQL only
//!
//! `CreateSpatialIndex` and `DropSpatialIndex` are intentionally **not**
//! declared as typed Diesel functions in this module.
//!
//! Manage index lifecycle with `diesel::sql_query(...)` (or SQL migrations),
//! which mirrors the PostGIS workflow where index lifecycle is DDL/SQL-driven.

use crate::diesel::types::Geometry;
use diesel::sql_types::{Binary, Double, Integer, Nullable, Text};

include!("generated/functions.rs");
