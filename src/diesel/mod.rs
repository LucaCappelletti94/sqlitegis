//! Diesel ORM integration. Backend-agnostic types and the
//! [`GeometryExpressionMethods`] trait live here. Enable `diesel-sqlite` or
//! `diesel-postgres` to compile the backend-specific impls.

pub mod expression_methods;
pub mod functions;
pub mod prelude;
pub mod query_helpers;
pub mod query_patterns;
pub mod types;

// #[doc(inline)] tells rustdoc to render each re-exported item AS IF it
// were defined here, so direct URLs like
// `crate::diesel::trait.GeometryExpressionMethods.html` resolve instead
// of 404'ing as plain `pub use` re-exports would.
#[doc(inline)]
pub use expression_methods::GeometryExpressionMethods;
#[doc(inline)]
pub use query_helpers::{
    dwithin_sphere_indexed_sql, dwithin_sphere_indexed_sql_string, intersects_window_indexed_sql,
    intersects_window_indexed_sql_string, nearest_sphere_indexed_sql,
    nearest_sphere_indexed_sql_string, radius_bbox, RadiusBbox,
};
#[doc(inline)]
pub use types::{Geography, GeographyValue, Geometry, GeometryValue};
