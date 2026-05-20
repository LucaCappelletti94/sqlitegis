//! Convenience re-exports for geolite-diesel.
//!
//! ```rust
//! use geolite::diesel::prelude::*;
//! use diesel::NullableExpressionMethods;
//!
//! // Type-check a common expression-method entrypoint from the prelude.
//! let expr = st_point(13.4, 52.5).nullable().st_astext();
//! let _ = expr;
//! ```

pub use crate::diesel::expression_methods::GeometryExpressionMethods;
pub use crate::diesel::functions::*;
pub use crate::diesel::types::{Geography, Geometry};
