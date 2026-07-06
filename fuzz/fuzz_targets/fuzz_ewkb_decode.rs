#![no_main]
//! Hostile-bytes safety net: feed arbitrary bytes to the blob-consuming
//! functions whose no-panic guarantee comes from input validation, and assert
//! none crashes the process.
//!
//! The computational-geometry functions (boolean ops, relate predicates,
//! `ST_Centroid`, `ST_IsValid`, ...) are NOT exercised here: they guard `geo`/
//! `i_overlay` panics with `catch_geo`, but libFuzzer aborts on any panic before
//! the inner `catch_unwind` runs, so they would report false crashes. Their
//! contract is covered by the `degenerate_geometry_never_panics` test.

use libfuzzer_sys::fuzz_target;

use sqlitegis::core::ewkb::{
    ensure_ewkb_nesting_ok, extract_mbr, extract_srid, is_empty_point_blob, parse_ewkb,
    parse_ewkb_header, set_srid, validate_ewkb_payload,
};
use sqlitegis::core::functions::accessors::{
    st_coord_dim, st_dimension, st_end_point, st_envelope, st_exterior_ring, st_geometry_n,
    st_geometry_type, st_interior_ring_n, st_is_empty, st_mem_size, st_ndims, st_npoints,
    st_num_geometries, st_num_interior_rings, st_num_points, st_num_rings, st_point_n, st_set_srid,
    st_srid, st_start_point, st_x, st_y, st_z, st_zmflag,
};
use sqlitegis::core::functions::constructors::{st_collect, st_make_line, st_make_polygon};
use sqlitegis::core::functions::io::{
    as_binary, as_ewkb, as_ewkt, as_geojson, as_text, geom_from_ewkb, geom_from_wkb,
};
use sqlitegis::core::functions::measurement::{
    st_area, st_azimuth, st_distance_sphere, st_distance_spheroid, st_length_sphere, st_perimeter,
    st_project, st_xmax, st_xmin, st_ymax, st_ymin,
};

fuzz_target!(|data: &[u8]| {
    // Header-level and cheap metadata paths.
    let _ = parse_ewkb_header(data);
    let _ = extract_srid(data);
    let _ = is_empty_point_blob(data);
    let _ = ensure_ewkb_nesting_ok(data);
    let _ = validate_ewkb_payload(data);

    // Full decode + single-blob accessors / measurements (no geo algorithms
    // that assert: areas and lengths are plain arithmetic over the vertices).
    let _ = st_srid(data);
    let _ = st_geometry_type(data);
    let _ = st_is_empty(data);
    let _ = st_dimension(data);
    let _ = st_mem_size(data);
    let _ = st_x(data);
    let _ = st_y(data);
    let _ = st_area(data);
    let _ = st_perimeter(data);
    let _ = st_npoints(data);
    let _ = st_num_points(data);
    let _ = st_num_geometries(data);
    let _ = st_num_interior_rings(data);
    let _ = st_num_rings(data);
    let _ = st_exterior_ring(data);
    let _ = st_envelope(data);

    // Dimension / coordinate metadata (Z/M aware, read straight off the header
    // and raw coordinate bytes).
    let _ = st_z(data);
    let _ = st_ndims(data);
    let _ = st_coord_dim(data);
    let _ = st_zmflag(data);
    let _ = st_xmin(data);
    let _ = st_xmax(data);
    let _ = st_ymin(data);
    let _ = st_ymax(data);
    let _ = st_start_point(data);
    let _ = st_end_point(data);

    // Index accessors. Drive the index from the fuzzer's own bytes so it sweeps
    // valid, zero, negative, and out-of-bounds positions.
    let n = if data.len() >= 4 {
        i32::from_le_bytes([data[0], data[1], data[2], data[3]])
    } else {
        1
    };
    for idx in [n, 0, 1, -1, i32::MAX, i32::MIN] {
        let _ = st_point_n(data, idx, Some(4326));
        let _ = st_geometry_n(data, idx);
        let _ = st_interior_ring_n(data, idx);
    }

    // Geodesic family (Haversine / geographiclib over parsed SRID-4326 points).
    // These run real float math and are not behind the panic guard, so a panic
    // here is a genuine finding rather than a false positive.
    let _ = st_length_sphere(data);
    let _ = st_distance_sphere(data, data);
    let _ = st_distance_spheroid(data, data);
    let _ = st_azimuth(data, data);
    for (dist, az) in [(1000.0, 45.0), (f64::NAN, 0.0), (f64::INFINITY, f64::NAN)] {
        let _ = st_project(data, dist, az);
    }

    // Serializers.
    let _ = as_text(data);
    let _ = as_ewkt(data);
    let _ = as_binary(data);
    let _ = as_ewkb(data);
    let _ = as_geojson(data);

    // Ingestion / round-trip and byte-level helpers.
    let _ = geom_from_ewkb(data);
    let _ = geom_from_wkb(data, Some(4326));
    let _ = set_srid(data, 3857);
    let _ = st_set_srid(data, 3857);
    let _ = extract_mbr(data);
    let _ = parse_ewkb(data);

    // Blob-consuming constructors: they decode through the guarded parsers and
    // assemble geo containers without invoking the assert-prone algorithms, so
    // they belong in the no-panic battery too.
    let _ = st_make_polygon(data);
    let _ = st_make_line(data, data);
    let _ = st_collect(data, data);
});
