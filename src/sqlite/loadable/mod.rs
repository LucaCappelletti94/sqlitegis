//! SQLite loadable-extension entry point built on
//! [`sqlite-loadable`](https://crates.io/crates/sqlite-loadable).
//!
//! Every SQLite call from the callbacks in this module goes through the
//! host's `sqlite3_api_routines` table that `sqlite-loadable`'s
//! `#[sqlite_entrypoint]` macro captures at load time. The produced cdylib
//! therefore has no link-time dependency on a specific libsqlite3 and works
//! against whichever libsqlite3 the host (Python, the SQLite CLI, etc.)
//! has loaded.
//!
//! Compare to [`crate::sqlite::ffi`] which is the in-process direct
//! `libsqlite3-sys` path used by Diesel. The two paths ship parallel
//! callback implementations: same `crate::core::functions::*` logic,
//! different FFI plumbing.
//!
//! Submodules:
//!
//! - [`args`]: arg-extraction and result-setting helpers wrapping
//!   `sqlite_loadable::api`.
//! - [`scalar`]: scalar-callback macros and one `fn *_cb` per SQL function.
//! - [`spatial_index`]: `CreateSpatialIndex` / `DropSpatialIndex` DDL
//!   callbacks plus the `sl_*` SQL execution helpers that drive them
//!   through `sqlite3ext_prepare_v2` / `_step` / `_finalize`.

mod args;
mod scalar;
mod spatial_index;

use sqlite_loadable::prelude::*;
use sqlite_loadable::scalar::define_scalar_function;
use sqlite_loadable::Result;

use scalar::{
    st_area_cb, st_asbinary_cb, st_asewkb_cb, st_asewkt_cb, st_asgeojson_cb, st_astext_cb,
    st_azimuth_cb, st_buffer_cb, st_centroid_cb, st_closestpoint_cb, st_collect_cb, st_contains_cb,
    st_coorddim_cb, st_coveredby_cb, st_covers_cb, st_crosses_cb, st_difference_cb,
    st_dimension_cb, st_disjoint_cb, st_distance_cb, st_distancesphere_cb, st_distancespheroid_cb,
    st_dwithin_cb, st_dwithinsphere_cb, st_dwithinspheroid_cb, st_endpoint_cb, st_envelope_cb,
    st_equals_cb, st_exteriorring_cb, st_geometryn_cb, st_geometrytype_cb, st_geomfromewkb_cb,
    st_geomfromgeojson, st_geomfromtext_1, st_geomfromtext_2, st_geomfromwkb_1, st_geomfromwkb_2,
    st_hausdorffdistance_cb, st_interiorringn_cb, st_intersection_cb, st_intersects_cb,
    st_isempty_cb, st_isvalid_cb, st_isvalidreason_cb, st_length2d_cb, st_length_cb,
    st_lengthsphere_cb, st_makeenvelope_4_cb, st_makeenvelope_5_cb, st_makeline_cb,
    st_makepolygon_cb, st_memsize_cb, st_ndims_cb, st_npoints_cb, st_numgeometries_cb,
    st_numinteriorrings_cb, st_numpoints_cb, st_numrings_cb, st_overlaps_cb, st_perimeter_cb,
    st_point_2_cb, st_point_3_cb, st_pointn_cb, st_pointonsurface_cb, st_project_cb,
    st_relate_bool_cb, st_relate_cb, st_relatematch_cb, st_setsrid_cb, st_srid_cb,
    st_startpoint_cb, st_symdifference_cb, st_touches_cb, st_union_cb, st_within_cb, st_x_cb,
    st_xmax_cb, st_xmin_cb, st_y_cb, st_ymax_cb, st_ymin_cb, st_z_cb, st_zmflag_cb,
};
use spatial_index::{create_spatial_index_cb, drop_spatial_index_cb};

/// C ABI entry point invoked by SQLite when this cdylib is loaded with
/// `SELECT load_extension('libsqlitegis')`. The `#[sqlite_entrypoint]`
/// attribute expands this into a `#[no_mangle]` extern "C" wrapper that
/// captures the host's `sqlite3_api_routines` pointer before delegating to
/// this body, and walks the catalog registering every scalar function.
#[sqlite_entrypoint]
pub fn sqlite3_sqlitegis_init(db: *mut sqlite3) -> Result<()> {
    let det = FunctionFlags::UTF8 | FunctionFlags::DETERMINISTIC;
    let direct = FunctionFlags::UTF8 | FunctionFlags::DIRECTONLY;

    // I/O
    define_scalar_function(db, "ST_GeomFromText", 1, st_geomfromtext_1, det)?;
    define_scalar_function(db, "ST_GeomFromText", 2, st_geomfromtext_2, det)?;
    define_scalar_function(db, "ST_GeomFromWKB", 1, st_geomfromwkb_1, det)?;
    define_scalar_function(db, "ST_GeomFromWKB", 2, st_geomfromwkb_2, det)?;
    define_scalar_function(db, "ST_GeomFromEWKB", 1, st_geomfromewkb_cb, det)?;
    define_scalar_function(db, "ST_GeomFromGeoJSON", 1, st_geomfromgeojson, det)?;
    define_scalar_function(db, "ST_AsText", 1, st_astext_cb, det)?;
    define_scalar_function(db, "ST_AsEWKT", 1, st_asewkt_cb, det)?;
    define_scalar_function(db, "ST_AsBinary", 1, st_asbinary_cb, det)?;
    define_scalar_function(db, "ST_AsEWKB", 1, st_asewkb_cb, det)?;
    define_scalar_function(db, "ST_AsGeoJSON", 1, st_asgeojson_cb, det)?;

    // Constructors
    define_scalar_function(db, "ST_Point", 2, st_point_2_cb, det)?;
    define_scalar_function(db, "ST_Point", 3, st_point_3_cb, det)?;
    define_scalar_function(db, "ST_MakePoint", 2, st_point_2_cb, det)?;
    define_scalar_function(db, "ST_MakePoint", 3, st_point_3_cb, det)?;
    define_scalar_function(db, "ST_MakeLine", 2, st_makeline_cb, det)?;
    define_scalar_function(db, "ST_MakePolygon", 1, st_makepolygon_cb, det)?;
    define_scalar_function(db, "ST_Collect", 2, st_collect_cb, det)?;
    define_scalar_function(db, "ST_MakeEnvelope", 4, st_makeenvelope_4_cb, det)?;
    define_scalar_function(db, "ST_MakeEnvelope", 5, st_makeenvelope_5_cb, det)?;

    // Accessors
    define_scalar_function(db, "ST_SRID", 1, st_srid_cb, det)?;
    define_scalar_function(db, "ST_SetSRID", 2, st_setsrid_cb, det)?;
    define_scalar_function(db, "ST_GeometryType", 1, st_geometrytype_cb, det)?;
    define_scalar_function(db, "ST_X", 1, st_x_cb, det)?;
    define_scalar_function(db, "ST_Y", 1, st_y_cb, det)?;
    define_scalar_function(db, "ST_Z", 1, st_z_cb, det)?;
    define_scalar_function(db, "ST_IsEmpty", 1, st_isempty_cb, det)?;
    define_scalar_function(db, "ST_NDims", 1, st_ndims_cb, det)?;
    define_scalar_function(db, "ST_CoordDim", 1, st_coorddim_cb, det)?;
    define_scalar_function(db, "ST_Zmflag", 1, st_zmflag_cb, det)?;
    define_scalar_function(db, "ST_MemSize", 1, st_memsize_cb, det)?;
    define_scalar_function(db, "ST_IsValid", 1, st_isvalid_cb, det)?;
    define_scalar_function(db, "ST_IsValidReason", 1, st_isvalidreason_cb, det)?;
    define_scalar_function(db, "ST_NumPoints", 1, st_numpoints_cb, det)?;
    define_scalar_function(db, "ST_NPoints", 1, st_npoints_cb, det)?;
    define_scalar_function(db, "ST_NumGeometries", 1, st_numgeometries_cb, det)?;
    define_scalar_function(db, "ST_NumInteriorRings", 1, st_numinteriorrings_cb, det)?;
    define_scalar_function(db, "ST_NumInteriorRing", 1, st_numinteriorrings_cb, det)?;
    define_scalar_function(db, "ST_NumRings", 1, st_numrings_cb, det)?;
    define_scalar_function(db, "ST_Dimension", 1, st_dimension_cb, det)?;
    define_scalar_function(db, "ST_Envelope", 1, st_envelope_cb, det)?;
    define_scalar_function(db, "ST_PointN", 2, st_pointn_cb, det)?;
    define_scalar_function(db, "ST_StartPoint", 1, st_startpoint_cb, det)?;
    define_scalar_function(db, "ST_EndPoint", 1, st_endpoint_cb, det)?;
    define_scalar_function(db, "ST_ExteriorRing", 1, st_exteriorring_cb, det)?;
    define_scalar_function(db, "ST_InteriorRingN", 2, st_interiorringn_cb, det)?;
    define_scalar_function(db, "ST_GeometryN", 2, st_geometryn_cb, det)?;
    define_scalar_function(db, "ST_XMin", 1, st_xmin_cb, det)?;
    define_scalar_function(db, "ST_XMax", 1, st_xmax_cb, det)?;
    define_scalar_function(db, "ST_YMin", 1, st_ymin_cb, det)?;
    define_scalar_function(db, "ST_YMax", 1, st_ymax_cb, det)?;

    // Measurement
    define_scalar_function(db, "ST_Area", 1, st_area_cb, det)?;
    define_scalar_function(db, "ST_Length", 1, st_length_cb, det)?;
    define_scalar_function(db, "ST_Length2D", 1, st_length2d_cb, det)?;
    define_scalar_function(db, "ST_Perimeter", 1, st_perimeter_cb, det)?;
    define_scalar_function(db, "ST_Distance", 2, st_distance_cb, det)?;
    define_scalar_function(db, "ST_Centroid", 1, st_centroid_cb, det)?;
    define_scalar_function(db, "ST_PointOnSurface", 1, st_pointonsurface_cb, det)?;
    define_scalar_function(db, "ST_HausdorffDistance", 2, st_hausdorffdistance_cb, det)?;

    // Geodesic
    define_scalar_function(db, "ST_DistanceSphere", 2, st_distancesphere_cb, det)?;
    define_scalar_function(db, "ST_DistanceSpheroid", 2, st_distancespheroid_cb, det)?;
    define_scalar_function(db, "ST_LengthSphere", 1, st_lengthsphere_cb, det)?;
    define_scalar_function(db, "ST_Azimuth", 2, st_azimuth_cb, det)?;
    define_scalar_function(db, "ST_Project", 3, st_project_cb, det)?;

    // Operations
    define_scalar_function(db, "ST_Union", 2, st_union_cb, det)?;
    define_scalar_function(db, "ST_Intersection", 2, st_intersection_cb, det)?;
    define_scalar_function(db, "ST_Difference", 2, st_difference_cb, det)?;
    define_scalar_function(db, "ST_SymDifference", 2, st_symdifference_cb, det)?;
    define_scalar_function(db, "ST_Buffer", 2, st_buffer_cb, det)?;
    define_scalar_function(db, "ST_ClosestPoint", 2, st_closestpoint_cb, det)?;

    // Predicates
    define_scalar_function(db, "ST_Intersects", 2, st_intersects_cb, det)?;
    define_scalar_function(db, "ST_Contains", 2, st_contains_cb, det)?;
    define_scalar_function(db, "ST_Within", 2, st_within_cb, det)?;
    define_scalar_function(db, "ST_Disjoint", 2, st_disjoint_cb, det)?;
    define_scalar_function(db, "ST_DWithin", 3, st_dwithin_cb, det)?;
    define_scalar_function(db, "ST_DWithinSphere", 3, st_dwithinsphere_cb, det)?;
    define_scalar_function(db, "ST_DWithinSpheroid", 3, st_dwithinspheroid_cb, det)?;
    define_scalar_function(db, "ST_Covers", 2, st_covers_cb, det)?;
    define_scalar_function(db, "ST_CoveredBy", 2, st_coveredby_cb, det)?;
    define_scalar_function(db, "ST_Equals", 2, st_equals_cb, det)?;
    define_scalar_function(db, "ST_Touches", 2, st_touches_cb, det)?;
    define_scalar_function(db, "ST_Crosses", 2, st_crosses_cb, det)?;
    define_scalar_function(db, "ST_Overlaps", 2, st_overlaps_cb, det)?;
    define_scalar_function(db, "ST_Relate", 2, st_relate_cb, det)?;
    define_scalar_function(db, "ST_Relate", 3, st_relate_bool_cb, det)?;
    define_scalar_function(db, "ST_RelateMatch", 2, st_relatematch_cb, det)?;

    // Spatial index DDL helpers (SQLITE_DIRECTONLY: rejected from triggers/views).
    define_scalar_function(db, "CreateSpatialIndex", 2, create_spatial_index_cb, direct)?;
    define_scalar_function(db, "DropSpatialIndex", 2, drop_spatial_index_cb, direct)?;

    Ok(())
}
