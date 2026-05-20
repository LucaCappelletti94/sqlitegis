//! Diesel ORM integration. Backend-agnostic types and the
//! [`GeometryExpressionMethods`] trait live here. Enable `diesel-sqlite` or
//! `diesel-postgres` to compile the backend-specific impls.

pub mod expression_methods;
pub mod functions;
pub mod prelude;
pub mod query_helpers;
pub mod query_patterns;
pub mod types;

pub use expression_methods::GeometryExpressionMethods;
pub use query_helpers::{
    dwithin_sphere_indexed_sql, dwithin_sphere_indexed_sql_string, radius_bbox, RadiusBbox,
};
pub use types::{Geography, Geometry};
