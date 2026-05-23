//! Scalar callback definitions for the loadable extension.
//!
//! Layout:
//!
//! - First, a small family of `macro_rules!` that emit a `fn name(ctx,
//!   values) -> Result<()>` per call shape (one blob, two blobs, blob plus
//!   integer, etc.). Each macro inlines the NULL handling and the error
//!   conversion so the per-function lines below stay short.
//! - Then, hand-written callbacks for shapes the macros do not cover
//!   (variadic SRID arguments, mixed text/blob args, NULL-required text).
//! - Last, the macro invocations that generate one `fn *_cb` per SQL
//!   function. The names match the ones in the catalog so the entry point
//!   in `mod.rs` can refer to them directly.

use sqlite_loadable::prelude::*;
use sqlite_loadable::{api, errors::Error, Result};

use crate::core::functions::accessors::*;
use crate::core::functions::constructors::*;
use crate::core::functions::io::*;
use crate::core::functions::measurement::*;
use crate::core::functions::operations::*;
use crate::core::functions::predicates::*;

use super::args::{
    any_null, arg_blob_or_null, arg_double, arg_i32, arg_text_or_null, mk_err, set_blob_vec,
    set_bool, set_f64, set_i32, set_i64, set_text_str,
};

// -------------------------------------------------------------------------
// Macros that emit a sqlite-loadable scalar callback per SQL function
// -------------------------------------------------------------------------

/// 1 blob -> Result<T>, with a custom setter. NULL input -> NULL output.
macro_rules! sl_blob {
    ($name:ident, $label:expr, $core_fn:expr, $set:ident) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(b) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            match $core_fn(b) {
                Ok(v) => $set(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// 2 blobs -> Result<T>, with a custom setter. Either NULL input -> NULL output.
macro_rules! sl_blob2 {
    ($name:ident, $label:expr, $core_fn:expr, $set:ident) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(a) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(b) = arg_blob_or_null(values, 1) else {
                api::result_null(ctx);
                return Ok(());
            };
            match $core_fn(a, b) {
                Ok(v) => $set(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// 1 blob -> Result<Option<f64>>, where None maps to SQL NULL.
macro_rules! sl_blob_opt_f64 {
    ($name:ident, $label:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(b) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            match $core_fn(b) {
                Ok(Some(v)) => set_f64(ctx, v)?,
                Ok(None) => api::result_null(ctx),
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// blob + i32 -> Result<Vec<u8>>.
macro_rules! sl_blob_i32_blob {
    ($name:ident, $label:expr, $arg_name:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(b) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(n) = arg_i32(values, 1) else {
                return Err(Error::new_message(format!(
                    "{}: {} cannot be NULL",
                    $label, $arg_name,
                )));
            };
            match ($core_fn)(b, n) {
                Ok(v) => set_blob_vec(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// blob + f64 -> Result<Vec<u8>>.
macro_rules! sl_blob_f64_blob {
    ($name:ident, $label:expr, $arg_name:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(b) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(d) = arg_double(values, 1) else {
                return Err(Error::new_message(format!(
                    "{}: {} cannot be NULL",
                    $label, $arg_name,
                )));
            };
            match ($core_fn)(b, d) {
                Ok(v) => set_blob_vec(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// blob + f64 + f64 -> Result<Vec<u8>>.
macro_rules! sl_blob_f64_f64_blob {
    ($name:ident, $label:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(b) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(d1) = arg_double(values, 1) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(d2) = arg_double(values, 2) else {
                api::result_null(ctx);
                return Ok(());
            };
            match ($core_fn)(b, d1, d2) {
                Ok(v) => set_blob_vec(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// 2 blobs + f64 -> Result<bool>.
macro_rules! sl_blob2_f64_bool {
    ($name:ident, $label:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(a) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(b) = arg_blob_or_null(values, 1) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(d) = arg_double(values, 2) else {
                return Err(Error::new_message(format!(
                    "{}: distance cannot be NULL",
                    $label,
                )));
            };
            match ($core_fn)(a, b, d) {
                Ok(v) => set_bool(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// 2 blobs + text -> Result<bool>.
macro_rules! sl_blob2_text_bool {
    ($name:ident, $label:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(a) = arg_blob_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(b) = arg_blob_or_null(values, 1) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(t) = arg_text_or_null(values, 2) else {
                return Err(Error::new_message(format!(
                    "{}: pattern cannot be NULL",
                    $label,
                )));
            };
            let pattern = t?;
            match ($core_fn)(a, b, pattern) {
                Ok(v) => set_bool(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

/// 2 text -> Result<bool>. Currently unused but kept for symmetry with
/// `xfunc_text2_bool` in `ffi.rs`; will be needed by the next batch of
/// text-input predicates.
#[allow(unused_macros)]
macro_rules! sl_text2_bool {
    ($name:ident, $label:expr, $core_fn:expr) => {
        pub(super) fn $name(
            ctx: *mut sqlite3_context,
            values: &[*mut sqlite3_value],
        ) -> Result<()> {
            let Some(a) = arg_text_or_null(values, 0) else {
                api::result_null(ctx);
                return Ok(());
            };
            let Some(b) = arg_text_or_null(values, 1) else {
                api::result_null(ctx);
                return Ok(());
            };
            let pa = a?;
            let pb = b?;
            match ($core_fn)(pa, pb) {
                Ok(v) => set_bool(ctx, v)?,
                Err(e) => return Err(mk_err($label, e)),
            }
            Ok(())
        }
    };
}

// -------------------------------------------------------------------------
// Hand-written callbacks for shapes the macros above do not cover.
// -------------------------------------------------------------------------

// ST_GeomFromText(wkt) and ST_GeomFromText(wkt, srid)
pub(super) fn st_geomfromtext_1(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(t) = arg_text_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let wkt = t?;
    match geom_from_text(wkt, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_GeomFromText", e)),
    }
}

pub(super) fn st_geomfromtext_2(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(t) = arg_text_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let wkt = t?;
    let srid = arg_i32(values, 1);
    match geom_from_text(wkt, srid) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_GeomFromText", e)),
    }
}

// ST_GeomFromWKB(blob) and ST_GeomFromWKB(blob, srid)
pub(super) fn st_geomfromwkb_1(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(b) = arg_blob_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    match geom_from_wkb(b, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_GeomFromWKB", e)),
    }
}

pub(super) fn st_geomfromwkb_2(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(b) = arg_blob_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let srid = arg_i32(values, 1);
    match geom_from_wkb(b, srid) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_GeomFromWKB", e)),
    }
}

// ST_GeomFromGeoJSON(text)
pub(super) fn st_geomfromgeojson(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(t) = arg_text_or_null(values, 0) else {
        return Err(Error::new_message(
            "ST_GeomFromGeoJSON: json cannot be NULL",
        ));
    };
    let json = t?;
    match geom_from_geojson(json, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_GeomFromGeoJSON", e)),
    }
}

// ST_Point(x, y) and ST_Point(x, y, srid)
pub(super) fn st_point_2_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    if any_null(&values[..2]) {
        api::result_null(ctx);
        return Ok(());
    }
    let x = api::value_double(&values[0]);
    let y = api::value_double(&values[1]);
    match st_point(x, y, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_Point", e)),
    }
}

pub(super) fn st_point_3_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    if any_null(values) {
        api::result_null(ctx);
        return Ok(());
    }
    let x = api::value_double(&values[0]);
    let y = api::value_double(&values[1]);
    let srid = Some(api::value_int(&values[2]));
    match st_point(x, y, srid) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_Point", e)),
    }
}

// ST_MakeEnvelope(xmin, ymin, xmax, ymax) and (..., srid)
pub(super) fn st_makeenvelope_4_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    if any_null(&values[..4]) {
        api::result_null(ctx);
        return Ok(());
    }
    let xmin = api::value_double(&values[0]);
    let ymin = api::value_double(&values[1]);
    let xmax = api::value_double(&values[2]);
    let ymax = api::value_double(&values[3]);
    match st_make_envelope(xmin, ymin, xmax, ymax, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_MakeEnvelope", e)),
    }
}

pub(super) fn st_makeenvelope_5_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    if any_null(values) {
        api::result_null(ctx);
        return Ok(());
    }
    let xmin = api::value_double(&values[0]);
    let ymin = api::value_double(&values[1]);
    let xmax = api::value_double(&values[2]);
    let ymax = api::value_double(&values[3]);
    let srid = Some(api::value_int(&values[4]));
    match st_make_envelope(xmin, ymin, xmax, ymax, srid) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_MakeEnvelope", e)),
    }
}

// ST_PointN(geom, n) - core fn takes (blob, i32, Option<srid>); pass None.
pub(super) fn st_pointn_cb(ctx: *mut sqlite3_context, values: &[*mut sqlite3_value]) -> Result<()> {
    let Some(b) = arg_blob_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let Some(n) = arg_i32(values, 1) else {
        return Err(Error::new_message("ST_PointN: n cannot be NULL"));
    };
    match st_point_n(b, n, None) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_PointN", e)),
    }
}

// ST_SetSRID(geom, srid)
pub(super) fn st_setsrid_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(b) = arg_blob_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let Some(srid) = arg_i32(values, 1) else {
        return Err(Error::new_message("ST_SetSRID: srid cannot be NULL"));
    };
    match st_set_srid(b, srid) {
        Ok(v) => set_blob_vec(ctx, v),
        Err(e) => Err(mk_err("ST_SetSRID", e)),
    }
}

// ST_RelateMatch(matrix, pattern) - both text.
pub(super) fn st_relatematch_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let Some(a) = arg_text_or_null(values, 0) else {
        api::result_null(ctx);
        return Ok(());
    };
    let Some(b) = arg_text_or_null(values, 1) else {
        api::result_null(ctx);
        return Ok(());
    };
    let matrix = a?;
    let pattern = b?;
    match st_relate_match(matrix, pattern) {
        Ok(v) => set_bool(ctx, v),
        Err(e) => Err(mk_err("ST_RelateMatch", e)),
    }
}

// -------------------------------------------------------------------------
// Deterministic scalar callbacks generated via the macros above
// -------------------------------------------------------------------------

// I/O serialisers
sl_blob!(st_astext_cb, "ST_AsText", as_text, set_text_str);
sl_blob!(st_asewkt_cb, "ST_AsEWKT", as_ewkt, set_text_str);
sl_blob!(st_asbinary_cb, "ST_AsBinary", as_binary, set_blob_vec);
sl_blob!(st_asewkb_cb, "ST_AsEWKB", as_ewkb, set_blob_vec);
sl_blob!(st_asgeojson_cb, "ST_AsGeoJSON", as_geojson, set_text_str);
sl_blob!(
    st_geomfromewkb_cb,
    "ST_GeomFromEWKB",
    geom_from_ewkb,
    set_blob_vec
);

// Constructor wrappers
sl_blob2!(st_makeline_cb, "ST_MakeLine", st_make_line, set_blob_vec);
sl_blob!(
    st_makepolygon_cb,
    "ST_MakePolygon",
    st_make_polygon,
    set_blob_vec
);
sl_blob2!(st_collect_cb, "ST_Collect", st_collect, set_blob_vec);

// Accessors
sl_blob!(st_srid_cb, "ST_SRID", st_srid, set_i32);
sl_blob!(
    st_geometrytype_cb,
    "ST_GeometryType",
    st_geometry_type,
    set_text_str
);
sl_blob!(st_ndims_cb, "ST_NDims", st_ndims, set_i32);
sl_blob!(st_coorddim_cb, "ST_CoordDim", st_coord_dim, set_i32);
sl_blob!(st_zmflag_cb, "ST_Zmflag", st_zmflag, set_i32);
sl_blob!(st_isempty_cb, "ST_IsEmpty", st_is_empty, set_bool);
sl_blob!(st_memsize_cb, "ST_MemSize", st_mem_size, set_i64);
sl_blob_opt_f64!(st_x_cb, "ST_X", st_x);
sl_blob_opt_f64!(st_y_cb, "ST_Y", st_y);
sl_blob_opt_f64!(st_z_cb, "ST_Z", st_z);
sl_blob!(st_numpoints_cb, "ST_NumPoints", st_num_points, set_i32);
sl_blob!(st_npoints_cb, "ST_NPoints", st_npoints, set_i32);
sl_blob!(
    st_numgeometries_cb,
    "ST_NumGeometries",
    st_num_geometries,
    set_i32
);
sl_blob!(
    st_numinteriorrings_cb,
    "ST_NumInteriorRings",
    st_num_interior_rings,
    set_i32
);
sl_blob!(st_numrings_cb, "ST_NumRings", st_num_rings, set_i32);
sl_blob!(
    st_startpoint_cb,
    "ST_StartPoint",
    st_start_point,
    set_blob_vec
);
sl_blob!(st_endpoint_cb, "ST_EndPoint", st_end_point, set_blob_vec);
sl_blob!(
    st_exteriorring_cb,
    "ST_ExteriorRing",
    st_exterior_ring,
    set_blob_vec
);
sl_blob_i32_blob!(
    st_interiorringn_cb,
    "ST_InteriorRingN",
    "n",
    st_interior_ring_n
);
sl_blob_i32_blob!(st_geometryn_cb, "ST_GeometryN", "n", st_geometry_n);
sl_blob!(st_dimension_cb, "ST_Dimension", st_dimension, set_i32);
sl_blob!(st_envelope_cb, "ST_Envelope", st_envelope, set_blob_vec);
sl_blob!(st_isvalid_cb, "ST_IsValid", st_is_valid, set_bool);
sl_blob!(
    st_isvalidreason_cb,
    "ST_IsValidReason",
    st_is_valid_reason,
    set_text_str
);

// Measurement
sl_blob!(st_area_cb, "ST_Area", st_area, set_f64);
sl_blob!(st_length_cb, "ST_Length", st_length, set_f64);
sl_blob!(st_length2d_cb, "ST_Length2D", st_length, set_f64);
sl_blob!(st_perimeter_cb, "ST_Perimeter", st_perimeter, set_f64);
sl_blob2!(st_distance_cb, "ST_Distance", st_distance, set_f64);
sl_blob!(st_centroid_cb, "ST_Centroid", st_centroid, set_blob_vec);
sl_blob!(
    st_pointonsurface_cb,
    "ST_PointOnSurface",
    st_point_on_surface,
    set_blob_vec
);
sl_blob2!(
    st_hausdorffdistance_cb,
    "ST_HausdorffDistance",
    st_hausdorff_distance,
    set_f64
);
sl_blob_opt_f64!(st_xmin_cb, "ST_XMin", st_xmin);
sl_blob_opt_f64!(st_xmax_cb, "ST_XMax", st_xmax);
sl_blob_opt_f64!(st_ymin_cb, "ST_YMin", st_ymin);
sl_blob_opt_f64!(st_ymax_cb, "ST_YMax", st_ymax);

// Geodesic measurement
sl_blob2!(
    st_distancesphere_cb,
    "ST_DistanceSphere",
    st_distance_sphere,
    set_f64
);
sl_blob2!(
    st_distancespheroid_cb,
    "ST_DistanceSpheroid",
    st_distance_spheroid,
    set_f64
);
sl_blob!(
    st_lengthsphere_cb,
    "ST_LengthSphere",
    st_length_sphere,
    set_f64
);
sl_blob2!(st_azimuth_cb, "ST_Azimuth", st_azimuth, set_f64);
sl_blob_f64_f64_blob!(st_project_cb, "ST_Project", st_project);

// Operations
sl_blob2!(st_union_cb, "ST_Union", st_union, set_blob_vec);
sl_blob2!(
    st_intersection_cb,
    "ST_Intersection",
    st_intersection,
    set_blob_vec
);
sl_blob2!(
    st_difference_cb,
    "ST_Difference",
    st_difference,
    set_blob_vec
);
sl_blob2!(
    st_symdifference_cb,
    "ST_SymDifference",
    st_sym_difference,
    set_blob_vec
);
sl_blob_f64_blob!(st_buffer_cb, "ST_Buffer", "distance", st_buffer);
sl_blob2!(
    st_closestpoint_cb,
    "ST_ClosestPoint",
    st_closest_point,
    set_blob_vec
);

// Predicates
sl_blob2!(st_intersects_cb, "ST_Intersects", st_intersects, set_bool);
sl_blob2!(st_contains_cb, "ST_Contains", st_contains, set_bool);
sl_blob2!(st_within_cb, "ST_Within", st_within, set_bool);
sl_blob2!(st_disjoint_cb, "ST_Disjoint", st_disjoint, set_bool);
sl_blob2_f64_bool!(st_dwithin_cb, "ST_DWithin", st_dwithin);
sl_blob2_f64_bool!(st_dwithinsphere_cb, "ST_DWithinSphere", st_dwithin_sphere);
sl_blob2_f64_bool!(
    st_dwithinspheroid_cb,
    "ST_DWithinSpheroid",
    st_dwithin_spheroid
);
sl_blob2!(st_covers_cb, "ST_Covers", st_covers, set_bool);
sl_blob2!(st_coveredby_cb, "ST_CoveredBy", st_covered_by, set_bool);
sl_blob2!(st_equals_cb, "ST_Equals", st_equals, set_bool);
sl_blob2!(st_touches_cb, "ST_Touches", st_touches, set_bool);
sl_blob2!(st_crosses_cb, "ST_Crosses", st_crosses, set_bool);
sl_blob2!(st_overlaps_cb, "ST_Overlaps", st_overlaps, set_bool);
sl_blob2!(st_relate_cb, "ST_Relate", st_relate, set_text_str);
sl_blob2_text_bool!(st_relate_bool_cb, "ST_Relate", st_relate_match_geoms);
