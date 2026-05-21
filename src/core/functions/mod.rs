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
