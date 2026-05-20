use thiserror::Error;

use crate::core::ewkb::geometry_type_name;

#[derive(Debug, Error)]
pub enum GeoLiteError {
    #[error("invalid EWKB: {0}")]
    InvalidEwkb(String),

    #[error("geozero error: {0}")]
    Geozero(#[from] geozero::error::GeozeroError),

    #[error("geometry is not a {expected}; got {actual}")]
    WrongType {
        expected: &'static str,
        actual: &'static str,
    },

    #[error("unsupported coordinate dimensions: {dimensions} (operation would drop coordinates)")]
    UnsupportedDimensions { dimensions: &'static str },

    #[error("index out of bounds: {index} (len {len})")]
    OutOfBounds { index: i32, len: usize },

    #[error("{0}")]
    InvalidInput(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GeoLiteError>;

impl GeoLiteError {
    /// Construct a `WrongType` error from an `expected` label and the actual
    /// geometry that was supplied. Centralises the `geometry_type_name`
    /// lookup so call sites don't have to repeat the boilerplate.
    pub fn wrong_type(expected: &'static str, got: &geo::Geometry<f64>) -> Self {
        Self::WrongType {
            expected,
            actual: geometry_type_name(got),
        }
    }
}
