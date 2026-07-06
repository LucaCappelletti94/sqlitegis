//! Pure-Rust spatial functions, partitioned by intent so docs.rs readers
//! land on the right submodule without scanning a flat list of 60+ names.
//!
//! - [`crate::core::functions::constructors`] -- build EWKB geometries from
//!   primitive scalars (`st_point`, `st_makeenvelope`, `st_makeline`, ...).
//! - [`crate::core::functions::accessors`] -- read metadata off an existing
//!   geometry blob (`st_x`, `st_y`, `st_srid`, `st_geometry_type`, ...).
//! - [`crate::core::functions::io`] -- parse and serialise WKT, GeoJSON, and
//!   EWKB (`geom_from_text`, `st_astext`, `st_geomfromgeojson`,
//!   `st_asgeojson`).
//! - [`crate::core::functions::measurement`] -- numeric measurements
//!   (`st_distance`, `st_area`, `st_length`, `st_distance_sphere`,
//!   `st_distance_spheroid`, ...).
//! - [`crate::core::functions::operations`] -- derive new geometries from
//!   existing ones (`st_buffer`, `st_union`, `st_intersection`,
//!   `st_difference`, ...).
//! - [`crate::core::functions::predicates`] -- boolean spatial relationships
//!   (`st_intersects`, `st_within`, `st_contains`, `st_dwithin`,
//!   `st_relate`, ...).
//!
//! Every function in these submodules takes EWKB BLOB slices on input
//! and returns either an EWKB `Vec<u8>` or a primitive scalar, with no
//! SQLite or Diesel coupling. The SQLite and Diesel layers wrap these
//! into their respective surfaces from the [catalog].
//!
//! [catalog]: crate::core::function_catalog

pub mod accessors;
pub mod constructors;
pub(crate) mod emptiness;
pub mod io;
pub mod measurement;
pub mod operations;
pub mod predicates;

/// Turn a panic from `geo` or `i_overlay` (both `assert!` on degenerate finite
/// geometry) into an error, so a hostile blob cannot abort the process and the
/// never-panic contract holds for direct callers past the FFI `xfunc_guard`.
// TODO(georust/geo#1552, #1554, #1555, #1556 plus the i_overlay aborts): drop
// once all are fixed and released.
pub(crate) fn catch_geo<T>(
    label: &str,
    f: impl FnOnce() -> crate::core::error::Result<T>,
) -> crate::core::error::Result<T> {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(_) => Err(crate::core::error::SqliteGisError::InvalidInput(format!(
            "{label}: operation failed on degenerate or invalid geometry"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::SqliteGisError;

    #[test]
    fn catch_geo_catches_panic_and_returns_error() {
        let result: Result<i32, SqliteGisError> = catch_geo("test_label", || {
            panic!("simulated geo panic");
        });
        assert!(result.is_err());
        let err = result.unwrap_err();
        match err {
            SqliteGisError::InvalidInput(msg) => {
                assert!(msg.contains("test_label"));
                assert!(msg.contains("operation failed on degenerate or invalid geometry"));
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn catch_geo_passes_through_ok_result() {
        let result = catch_geo("test", || Ok(42));
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn catch_geo_passes_through_err_result() {
        let result: Result<i32, SqliteGisError> = catch_geo("test", || {
            Err(SqliteGisError::InvalidInput("inner error".into()))
        });
        assert!(result.is_err());
    }
}
