use thiserror::Error;

use crate::core::ewkb::geometry_type_name;

/// Errors returned by SQLiteGIS's core, SQLite, and Diesel layers.
///
/// A typical match handling looks like:
///
/// ```
/// use sqlitegis::SqliteGisError;
/// use sqlitegis::core::functions::constructors::st_point;
/// use sqlitegis::core::functions::measurement::st_distance;
///
/// let a = st_point(0.0, 0.0, None).unwrap();
/// let b = st_point(3.0, 4.0, None).unwrap();
/// match st_distance(&a, &b) {
///     Ok(d) => assert!((d - 5.0).abs() < 1e-10),
///     Err(SqliteGisError::InvalidEwkb(_msg)) => {
///         // malformed BLOB on input
///     }
///     Err(SqliteGisError::WrongType { expected, actual }) => {
///         eprintln!("expected {expected}, got {actual}");
///     }
///     Err(SqliteGisError::InvalidInput(_msg)) => {
///         // semantically rejected (e.g. non-finite coordinate)
///     }
///     Err(_other) => {}
/// }
/// ```
#[derive(Debug, Error)]
pub enum SqliteGisError {
    /// The supplied bytes did not parse as a valid EWKB BLOB.
    #[error("invalid EWKB: {0}")]
    InvalidEwkb(String),

    /// A nested geozero codec error bubbled up from WKT, GeoJSON, or WKB I/O.
    #[error("geozero error: {0}")]
    Geozero(#[from] geozero::error::GeozeroError),

    /// The geometry's concrete type did not match what the operation needed.
    #[error("geometry is not a {expected}; got {actual}")]
    WrongType {
        /// Human readable name of the geometry type the caller expected.
        expected: &'static str,
        /// Human readable name of the geometry type that was supplied.
        actual: &'static str,
    },

    /// The operation cannot run on a geometry with the given coordinate
    /// dimensionality without silently dropping coordinates.
    #[error("unsupported coordinate dimensions: {dimensions} (operation would drop coordinates)")]
    UnsupportedDimensions {
        /// Label of the unsupported dimensional layout (e.g. `"XYZM"`).
        dimensions: &'static str,
    },

    /// A 1-based index argument was outside the addressable range.
    #[error("index out of bounds: {index} (len {len})")]
    OutOfBounds {
        /// The offending index as provided by the caller.
        index: i32,
        /// Length of the collection the index was checked against.
        len: usize,
    },

    /// The argument was syntactically valid but semantically rejected
    /// (e.g. a non-finite coordinate, an unknown SRID, an empty WKT string).
    #[error("{0}")]
    InvalidInput(String),

    /// An underlying `std::io::Error` from a reader or writer.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias used by every fallible function in the crate.
pub type Result<T> = std::result::Result<T, SqliteGisError>;

impl SqliteGisError {
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
