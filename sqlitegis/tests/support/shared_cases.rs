macro_rules! define_shared_cases {
    ($test_attr:meta) => {
// I/O round-trips

fn assert_i32_out_of_range_error(
    db: &ActiveTestDb,
    sql: &str,
    fn_label: &str,
    arg_name: &str,
) {
    let err = db
        .try_query_i64(sql)
        .expect_err("query should fail for out-of-range i32 argument");
    assert!(
        err.contains("out of range for i32"),
        "unexpected error message for `{sql}`: {err}"
    );
    assert!(
        err.contains(fn_label),
        "error should mention function `{fn_label}` for `{sql}`: {err}"
    );
    let arg_token = format!(": {arg_name} out of range for i32:");
    assert!(
        err.contains(&arg_token),
        "error should mention argument token `{arg_token}` for `{sql}`: {err}"
    );
}

fn assert_non_overflow_error(db: &ActiveTestDb, sql: &str, expected_substring: &str) {
    let err = db
        .try_query_i64(sql)
        .expect_err("query should fail after passing i32 conversion");
    assert!(
        err.contains(expected_substring),
        "unexpected error message for `{sql}`: {err}"
    );
    assert!(
        !err.contains("out of range for i32"),
        "boundary i32 value should not trigger overflow for `{sql}`: {err}"
    );
}

#[$test_attr]
fn wkt_round_trip() {
    let db = ActiveTestDb::open();
    let wkt = db.query_text("SELECT ST_AsText(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert!(wkt.contains("POLYGON"), "got: {wkt}");
}

#[$test_attr]
fn point_empty_round_trip() {
    let db = ActiveTestDb::open();
    let wkt = db.query_text("SELECT ST_AsText(ST_GeomFromText('POINT EMPTY'))");
    assert_eq!(wkt, "POINT EMPTY");

    let is_empty = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('POINT EMPTY'))");
    assert_eq!(is_empty, 1);
}

#[$test_attr]
fn geometrycollection_with_empty_point_round_trip() {
    let db = ActiveTestDb::open();
    let npoints = db.query_i64("SELECT ST_NPoints(ST_GeomFromText('GEOMETRYCOLLECTION(POINT EMPTY)'))");
    assert_eq!(npoints, 0);

    let is_empty = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('GEOMETRYCOLLECTION(POINT EMPTY)'))");
    assert_eq!(is_empty, 1);
}

#[$test_attr]
fn geojson_round_trip() {
    let db = ActiveTestDb::open();
    let json = db.query_text(
        "SELECT ST_AsGeoJSON(ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1.0,2.0]}'))",
    );
    assert!(json.contains("Point"), "got: {json}");
}

#[$test_attr]
fn point_empty_geojson_is_postgis_compatible() {
    let db = ActiveTestDb::open();
    let json = db.query_text("SELECT ST_AsGeoJSON(ST_GeomFromText('POINT EMPTY'))");
    assert_eq!(json, r#"{"type":"Point","coordinates":[]}"#);
}

#[$test_attr]
fn point_empty_geojson_round_trip() {
    let db = ActiveTestDb::open();
    let wkt = db.query_text(
        "SELECT ST_AsText(ST_GeomFromGeoJSON(ST_AsGeoJSON(ST_GeomFromText('POINT EMPTY'))))",
    );
    assert_eq!(wkt, "POINT EMPTY");
}

#[$test_attr]
fn wkb_round_trip() {
    let db = ActiveTestDb::open();
    let wkt = db
        .query_text("SELECT ST_AsText(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(3 4)'))))");
    assert!(wkt.contains("POINT"), "got: {wkt}");
}

#[$test_attr]
fn ewkb_round_trip() {
    let db = ActiveTestDb::open();
    let wkt = db
        .query_text("SELECT ST_AsText(ST_GeomFromEWKB(ST_AsEWKB(ST_GeomFromText('POINT(1 2)'))))");
    assert!(wkt.contains("POINT"), "got: {wkt}");
}

#[$test_attr]
fn ewkb_round_trip_rejects_zm_payload() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT length(ST_AsEWKB(ST_GeomFromEWKB(X'01010000C0000000000000F03F000000000000004000000000000008400000000000001040')))",
        )
        .expect_err("ZM payload must be rejected");
    assert!(err.contains("unsupported coordinate dimensions"));
}

#[$test_attr]
fn ewkb_round_trip_rejects_big_endian_zm_payload() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT length(ST_AsEWKB(ST_GeomFromEWKB(X'00C00000013FF0000000000000400000000000000040080000000000004010000000000000')))",
        )
        .expect_err("big-endian ZM payload must be rejected");
    assert!(err.contains("unsupported coordinate dimensions"));
}

#[$test_attr]
fn ewkt_round_trip() {
    let db = ActiveTestDb::open();
    let ewkt = db.query_text("SELECT ST_AsEWKT(ST_GeomFromText('POINT(1 2)', 4326))");
    assert!(ewkt.starts_with("SRID=4326;"), "got: {ewkt}");
}

#[$test_attr]
fn geomfromwkb_with_srid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64(
        "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(0 0)')), 4326))",
    );
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn geomfromgeojson_default_srid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64(
        "SELECT ST_SRID(ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1,2]}'))",
    );
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn geomfromgeojson_srid_override_uses_setsrid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64(
        "SELECT ST_SRID(ST_SetSRID(ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1,2]}'), 3857))",
    );
    assert_eq!(srid, 3857);
}

#[$test_attr]
fn geomfromgeojson_rejects_two_arg_signature() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_SRID(ST_GeomFromGeoJSON('{\"type\":\"Point\",\"coordinates\":[1,2]}', 3857))",
        )
        .expect_err("two-arg ST_GeomFromGeoJSON should not be registered");
    let normalized = err.to_ascii_lowercase();
    assert!(
        normalized.contains("wrong number of arguments"),
        "unexpected error message: {err}"
    );
    assert!(
        err.contains("ST_GeomFromGeoJSON"),
        "error should mention ST_GeomFromGeoJSON: {err}"
    );
}

// Constructors

#[$test_attr]
fn st_make_envelope() {
    let db = ActiveTestDb::open();
    let area = db.query_f64("SELECT ST_Area(ST_MakeEnvelope(0, 0, 2, 3))");
    assert!((area - 6.0).abs() < 1e-10, "area = {area}");
}

#[$test_attr]
fn st_tile_envelope_zoom0() {
    let db = ActiveTestDb::open();
    let area = db.query_f64("SELECT ST_Area(ST_TileEnvelope(0, 0, 0))");
    // Full web-mercator extent squared: (2 * 20037508.34)^2 approximately  1.607e15
    assert!(area > 1e15, "area = {area}");
}

#[$test_attr]
fn st_tile_envelope_negative_args_rejected_with_clear_error() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Area(ST_TileEnvelope(-1, 0, 0))")
        .expect_err("negative zoom should return an error");
    assert!(
        err.contains("must be non-negative"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_tile_envelope_rejects_out_of_range_i32_args() {
    let db = ActiveTestDb::open();
    let cases = [
        ("SELECT ST_Area(ST_TileEnvelope(2147483648, 0, 0))", "zoom"),
        ("SELECT ST_Area(ST_TileEnvelope(-2147483649, 0, 0))", "zoom"),
        ("SELECT ST_Area(ST_TileEnvelope(0, 2147483648, 0))", "tile x"),
        ("SELECT ST_Area(ST_TileEnvelope(0, -2147483649, 0))", "tile x"),
        ("SELECT ST_Area(ST_TileEnvelope(0, 0, 2147483648))", "tile y"),
        ("SELECT ST_Area(ST_TileEnvelope(0, 0, -2147483649))", "tile y"),
    ];
    for (sql, arg_label) in cases {
        assert_i32_out_of_range_error(&db, sql, "ST_TileEnvelope", arg_label);
    }
}

#[$test_attr]
fn st_tile_envelope_i32_boundaries_are_not_treated_as_overflow() {
    let db = ActiveTestDb::open();
    assert_non_overflow_error(
        &db,
        "SELECT ST_Area(ST_TileEnvelope(2147483647, 0, 0))",
        "exceeds maximum of 31",
    );
    assert_non_overflow_error(
        &db,
        "SELECT ST_Area(ST_TileEnvelope(-2147483648, 0, 0))",
        "zoom must be non-negative",
    );
    assert_non_overflow_error(
        &db,
        "SELECT ST_Area(ST_TileEnvelope(1, 2147483647, 0))",
        "out of range for zoom",
    );
    assert_non_overflow_error(
        &db,
        "SELECT ST_Area(ST_TileEnvelope(1, 0, -2147483648))",
        "tile y must be non-negative",
    );
}

#[$test_attr]
fn st_point_with_srid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64("SELECT ST_SRID(ST_Point(1, 2, 4326))");
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn null_numeric_arg_st_point_returns_null() {
    let db = ActiveTestDb::open();
    let is_null = db.query_i64("SELECT ST_Point(1,2,NULL) IS NULL");
    assert_eq!(is_null, 1);
}

#[$test_attr]
fn st_point_rejects_non_numeric_args() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Point('abc', 2) IS NULL")
        .expect_err("non-numeric ST_Point argument should be a hard error");
    assert!(
        err.contains("must be numeric"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_point_rejects_non_finite_coordinates() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_IsValid(ST_Point(1e309, 0))")
        .expect_err("non-finite ST_Point coordinates must be rejected");
    assert!(
        err.contains("coordinates must be finite"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_make_envelope_null_short_circuits_invalid_numeric_args() {
    let db = ActiveTestDb::open();
    let result = db.try_query_i64("SELECT ST_MakeEnvelope('abc', 0, 1, 1, NULL) IS NULL");
    assert_eq!(
        result,
        Ok(1),
        "NULL argument should short-circuit before numeric type errors: {result:?}"
    );
}

#[$test_attr]
fn st_make_line() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumPoints(ST_MakeLine(ST_Point(0,0), ST_Point(1,1)))");
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_make_line_rejects_empty_points() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_NumPoints(ST_MakeLine(ST_GeomFromText('POINT EMPTY'), ST_Point(1,1)))")
        .expect_err("empty point input must be rejected");
    assert!(
        err.contains("point must not be empty"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_make_polygon() {
    let db = ActiveTestDb::open();
    let t = db.query_text(
        "SELECT ST_GeometryType(ST_MakePolygon(ST_GeomFromText('LINESTRING(0 0,1 0,1 1,0 1,0 0)')))",
    );
    assert_eq!(t, "ST_Polygon");
}

#[$test_attr]
fn st_make_envelope_with_srid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64("SELECT ST_SRID(ST_MakeEnvelope(0,0,1,1,4326))");
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn st_collect() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumGeometries(ST_Collect(ST_Point(0,0), ST_Point(1,1)))");
    assert_eq!(n, 2);
}

// Accessors

#[$test_attr]
fn st_srid_default() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64("SELECT ST_SRID(ST_GeomFromText('POINT(0 0)'))");
    assert_eq!(srid, 0);
}

#[$test_attr]
fn st_srid_rejects_malformed_ewkb() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_SRID(X'01') IS NULL")
        .expect_err("malformed EWKB must be a hard error");
    assert!(err.contains("invalid EWKB"), "unexpected error message: {err}");
}

#[$test_attr]
fn st_srid_set() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64("SELECT ST_SRID(ST_GeomFromText('POINT(0 0)', 4326))");
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn st_set_srid() {
    let db = ActiveTestDb::open();
    let srid = db.query_i64("SELECT ST_SRID(ST_SetSRID(ST_GeomFromText('POINT(0 0)'), 4326))");
    assert_eq!(srid, 4326);
}

#[$test_attr]
fn srid_args_reject_out_of_range_i32() {
    let db = ActiveTestDb::open();
    let cases = [
        ("SELECT ST_SRID(ST_Point(1, 2, 2147483648))", "ST_Point"),
        ("SELECT ST_SRID(ST_Point(1, 2, -2147483649))", "ST_Point"),
        (
            "SELECT ST_SRID(ST_MakeEnvelope(0, 0, 1, 1, 2147483648))",
            "ST_MakeEnvelope",
        ),
        (
            "SELECT ST_SRID(ST_MakeEnvelope(0, 0, 1, 1, -2147483649))",
            "ST_MakeEnvelope",
        ),
        (
            "SELECT ST_SRID(ST_GeomFromText('POINT(0 0)', 2147483648))",
            "ST_GeomFromText",
        ),
        (
            "SELECT ST_SRID(ST_GeomFromText('POINT(0 0)', -2147483649))",
            "ST_GeomFromText",
        ),
        (
            "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(0 0)')), 2147483648))",
            "ST_GeomFromWKB",
        ),
        (
            "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(0 0)')), -2147483649))",
            "ST_GeomFromWKB",
        ),
        (
            "SELECT ST_SRID(ST_SetSRID(ST_Point(0, 0), 2147483648))",
            "ST_SetSRID",
        ),
        (
            "SELECT ST_SRID(ST_SetSRID(ST_Point(0, 0), -2147483649))",
            "ST_SetSRID",
        ),
    ];
    for (sql, fn_label) in cases {
        assert_i32_out_of_range_error(&db, sql, fn_label, "srid");
    }
}

#[$test_attr]
fn srid_args_accept_i32_boundaries() {
    let db = ActiveTestDb::open();
    assert_eq!(db.query_i64("SELECT ST_SRID(ST_Point(1, 2, 2147483647))"), 2147483647);
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_Point(1, 2, -2147483648))"),
        -2147483648
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_MakeEnvelope(0, 0, 1, 1, 2147483647))"),
        2147483647
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_MakeEnvelope(0, 0, 1, 1, -2147483648))"),
        -2147483648
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_GeomFromText('POINT(0 0)', 2147483647))"),
        2147483647
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_GeomFromText('POINT(0 0)', -2147483648))"),
        -2147483648
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(0 0)')), 2147483647))"),
        2147483647
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_GeomFromText('POINT(0 0)')), -2147483648))"),
        -2147483648
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_SetSRID(ST_Point(0, 0), 2147483647))"),
        2147483647
    );
    assert_eq!(
        db.query_i64("SELECT ST_SRID(ST_SetSRID(ST_Point(0, 0), -2147483648))"),
        -2147483648
    );
}

#[$test_attr]
fn st_geometry_type() {
    let db = ActiveTestDb::open();
    let t = db.query_text("SELECT ST_GeometryType(ST_GeomFromText('POINT(0 0)'))");
    assert_eq!(t, "ST_Point");
}

#[$test_attr]
fn st_geometry_type_rejects_truncated_ewkb() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_GeometryType(X'0101000000') IS NULL")
        .expect_err("truncated EWKB must be a hard error");
    assert!(
        err.contains("truncated"),
        "unexpected error message for truncated EWKB: {err}"
    );
}

#[$test_attr]
fn st_x_y() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_Point(3.0, 4.0))");
    let y = db.query_f64("SELECT ST_Y(ST_Point(3.0, 4.0))");
    assert!((x - 3.0).abs() < 1e-10, "x = {x}");
    assert!((y - 4.0).abs() < 1e-10, "y = {y}");
}

#[$test_attr]
fn st_x_y_point_empty_returns_null() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_X(ST_GeomFromText('POINT EMPTY'))"));
    assert!(db.query_is_null("SELECT ST_Y(ST_GeomFromText('POINT EMPTY'))"));
}

#[$test_attr]
fn st_z_contract() {
    let db = ActiveTestDb::open();

    assert!(db.query_is_null("SELECT ST_Z(ST_Point(3.0, 4.0))"));
    assert!(db.query_is_null("SELECT ST_Z(ST_GeomFromText('POINT EMPTY'))"));

    let z = db.query_f64("SELECT ST_Z(X'0101000080000000000000F03F00000000000000400000000000000840')");
    assert!((z - 3.0).abs() < 1e-10, "z = {z}");

    let zm =
        db.query_f64("SELECT ST_Z(X'01010000C0000000000000F03F000000000000004000000000000008400000000000001040')");
    assert!((zm - 3.0).abs() < 1e-10, "zm z = {zm}");

    assert!(db.query_is_null(
        "SELECT ST_Z(X'0101000040000000000000F03F00000000000000400000000000001040')"
    ));
}

#[$test_attr]
fn st_z_supports_big_endian_ewkb() {
    let db = ActiveTestDb::open();
    let z = db.query_f64("SELECT ST_Z(X'00800000013FF000000000000040000000000000004008000000000000')");
    assert!((z - 3.0).abs() < 1e-10, "z = {z}");
}

#[$test_attr]
fn st_z_rejects_non_point() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT CAST(ST_Z(ST_GeomFromText('LINESTRING(0 0,1 1)')) AS INTEGER)")
        .expect_err("ST_Z on non-point input must error");
    assert!(err.contains("Point"), "unexpected error message: {err}");
}

#[$test_attr]
fn st_is_empty() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('POINT(0 0)'))");
    assert_eq!(e, 0);
}

#[$test_attr]
fn st_ndims() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NDims(ST_GeomFromText('POINT(1 2)'))");
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_coord_dim() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_CoordDim(ST_GeomFromText('POINT(1 2)'))");
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_zmflag() {
    let db = ActiveTestDb::open();
    let z = db.query_i64("SELECT ST_Zmflag(ST_GeomFromText('POINT(1 2)'))");
    assert_eq!(z, 0);
}

#[$test_attr]
fn st_mem_size() {
    let db = ActiveTestDb::open();
    let s = db.query_i64("SELECT ST_MemSize(ST_GeomFromText('POINT(1 2)'))");
    assert!(s > 0, "mem_size = {s}");
}

#[$test_attr]
fn st_num_points() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))");
    assert_eq!(n, 3);
}

#[$test_attr]
fn st_npoints() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NPoints(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert_eq!(n, 5);
}

#[$test_attr]
fn st_num_geometries() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumGeometries(ST_Collect(ST_Point(0,0), ST_Point(1,1)))");
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_num_interior_rings() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumInteriorRings(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    );
    assert_eq!(n, 1);
}

#[$test_attr]
fn st_num_rings() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumRings(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert_eq!(n, 1);
}

#[$test_attr]
fn st_point_n() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_PointN(ST_GeomFromText('LINESTRING(10 20,30 40)'), 2))");
    assert!((x - 30.0).abs() < 1e-10, "x = {x}");
}

#[$test_attr]
fn st_start_point() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_StartPoint(ST_GeomFromText('LINESTRING(10 20,30 40)')))");
    assert!((x - 10.0).abs() < 1e-10, "x = {x}");
}

#[$test_attr]
fn st_end_point() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_EndPoint(ST_GeomFromText('LINESTRING(10 20,30 40)')))");
    assert!((x - 30.0).abs() < 1e-10, "x = {x}");
}

#[$test_attr]
fn st_exterior_ring() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumPoints(ST_ExteriorRing(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')))",
    );
    assert_eq!(n, 5);
}

#[$test_attr]
fn st_interior_ring_n() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumPoints(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'), 1))",
    );
    assert_eq!(n, 5);
}

#[$test_attr]
fn st_geometry_n() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_GeometryN(ST_Collect(ST_Point(5,6), ST_Point(7,8)), 1))");
    assert!((x - 5.0).abs() < 1e-10, "x = {x}");
}

#[$test_attr]
fn index_args_reject_out_of_range_i32() {
    let db = ActiveTestDb::open();
    let cases = [
        (
            "SELECT ST_IsEmpty(ST_PointN(ST_GeomFromText('LINESTRING(0 0,1 1)'), 2147483648))",
            "ST_PointN",
        ),
        (
            "SELECT ST_IsEmpty(ST_PointN(ST_GeomFromText('LINESTRING(0 0,1 1)'), -2147483649))",
            "ST_PointN",
        ),
        (
            "SELECT ST_IsEmpty(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'), 2147483648))",
            "ST_InteriorRingN",
        ),
        (
            "SELECT ST_IsEmpty(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'), -2147483649))",
            "ST_InteriorRingN",
        ),
        (
            "SELECT ST_IsEmpty(ST_GeometryN(ST_Collect(ST_Point(0,0), ST_Point(1,1)), 2147483648))",
            "ST_GeometryN",
        ),
        (
            "SELECT ST_IsEmpty(ST_GeometryN(ST_Collect(ST_Point(0,0), ST_Point(1,1)), -2147483649))",
            "ST_GeometryN",
        ),
    ];
    for (sql, fn_label) in cases {
        assert_i32_out_of_range_error(&db, sql, fn_label, "n");
    }
}

#[$test_attr]
fn index_args_i32_boundaries_are_not_treated_as_overflow() {
    let db = ActiveTestDb::open();
    let cases = [
        (
            "SELECT ST_IsEmpty(ST_PointN(ST_GeomFromText('LINESTRING(0 0,1 1)'), 2147483647))",
            "index out of bounds",
        ),
        (
            "SELECT ST_IsEmpty(ST_PointN(ST_GeomFromText('LINESTRING(0 0,1 1)'), -2147483648))",
            "index out of bounds",
        ),
        (
            "SELECT ST_IsEmpty(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'), 2147483647))",
            "index out of bounds",
        ),
        (
            "SELECT ST_IsEmpty(ST_InteriorRingN(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'), -2147483648))",
            "index out of bounds",
        ),
        (
            "SELECT ST_IsEmpty(ST_GeometryN(ST_Collect(ST_Point(0,0), ST_Point(1,1)), 2147483647))",
            "index out of bounds",
        ),
        (
            "SELECT ST_IsEmpty(ST_GeometryN(ST_Collect(ST_Point(0,0), ST_Point(1,1)), -2147483648))",
            "index out of bounds",
        ),
    ];
    for (sql, expected_substring) in cases {
        assert_non_overflow_error(&db, sql, expected_substring);
    }
}

#[$test_attr]
fn st_dimension() {
    let db = ActiveTestDb::open();
    let d = db.query_i64("SELECT ST_Dimension(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert_eq!(d, 2);
}

#[$test_attr]
fn st_envelope() {
    let db = ActiveTestDb::open();
    let area = db.query_f64("SELECT ST_Area(ST_Envelope(ST_GeomFromText('LINESTRING(0 0,2 3)')))");
    assert!((area - 6.0).abs() < 1e-10, "area = {area}");
}

#[$test_attr]
fn st_envelope_empty_geometry_passthrough() {
    let db = ActiveTestDb::open();
    let point_empty_wkt =
        db.query_text("SELECT ST_AsText(ST_Envelope(ST_GeomFromText('POINT EMPTY')))");
    assert_eq!(point_empty_wkt, "POINT EMPTY");

    let gc_empty_wkt = db.query_text(
        "SELECT ST_AsText(ST_Envelope(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY')))",
    );
    assert_eq!(gc_empty_wkt, "GEOMETRYCOLLECTION EMPTY");
}

#[$test_attr]
fn st_is_valid() {
    let db = ActiveTestDb::open();
    let v = db.query_i64("SELECT ST_IsValid(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_is_valid_reason() {
    let db = ActiveTestDb::open();
    let r =
        db.query_text("SELECT ST_IsValidReason(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert_eq!(r, "Valid Geometry");
}

// Measurement

#[$test_attr]
fn st_area_unit_square() {
    let db = ActiveTestDb::open();
    let area = db.query_f64("SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert!((area - 1.0).abs() < 1e-10, "area = {area}");
}

#[$test_attr]
fn st_distance_3_4_5() {
    let db = ActiveTestDb::open();
    let d = db.query_f64("SELECT ST_Distance(ST_Point(0,0), ST_Point(3,4))");
    assert!((d - 5.0).abs() < 1e-10, "distance = {d}");
}

#[$test_attr]
fn st_centroid_square() {
    let db = ActiveTestDb::open();
    let cx =
        db.query_f64("SELECT ST_X(ST_Centroid(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')))");
    let cy =
        db.query_f64("SELECT ST_Y(ST_Centroid(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')))");
    assert!((cx - 1.0).abs() < 1e-10, "cx = {cx}");
    assert!((cy - 1.0).abs() < 1e-10, "cy = {cy}");
}

#[$test_attr]
fn st_bbox() {
    let db = ActiveTestDb::open();
    let xmin = db.query_f64("SELECT ST_XMin(ST_GeomFromText('POLYGON((1 2,3 2,3 4,1 4,1 2))'))");
    let xmax = db.query_f64("SELECT ST_XMax(ST_GeomFromText('POLYGON((1 2,3 2,3 4,1 4,1 2))'))");
    let ymin = db.query_f64("SELECT ST_YMin(ST_GeomFromText('POLYGON((1 2,3 2,3 4,1 4,1 2))'))");
    let ymax = db.query_f64("SELECT ST_YMax(ST_GeomFromText('POLYGON((1 2,3 2,3 4,1 4,1 2))'))");
    assert!((xmin - 1.0).abs() < 1e-10);
    assert!((xmax - 3.0).abs() < 1e-10);
    assert!((ymin - 2.0).abs() < 1e-10);
    assert!((ymax - 4.0).abs() < 1e-10);
}

#[$test_attr]
fn st_bbox_empty_returns_null() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_XMin(ST_GeomFromText('POINT EMPTY'))"));
    assert!(db.query_is_null("SELECT ST_XMax(ST_GeomFromText('POINT EMPTY'))"));
    assert!(db.query_is_null("SELECT ST_YMin(ST_GeomFromText('POINT EMPTY'))"));
    assert!(db.query_is_null("SELECT ST_YMax(ST_GeomFromText('POINT EMPTY'))"));

    assert!(db.query_is_null(
        "SELECT ST_XMin(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))"
    ));
    assert!(db.query_is_null(
        "SELECT ST_XMax(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))"
    ));
    assert!(db.query_is_null(
        "SELECT ST_YMin(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))"
    ));
    assert!(db.query_is_null(
        "SELECT ST_YMax(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))"
    ));
}

#[$test_attr]
fn st_length() {
    let db = ActiveTestDb::open();
    let l = db.query_f64("SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0,3 4)'))");
    assert!((l - 5.0).abs() < 1e-10, "length = {l}");
}

#[$test_attr]
fn st_perimeter() {
    let db = ActiveTestDb::open();
    let p = db.query_f64("SELECT ST_Perimeter(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert!((p - 4.0).abs() < 1e-10, "perimeter = {p}");
}

#[$test_attr]
fn st_point_on_surface() {
    let db = ActiveTestDb::open();
    let c = db.query_i64(
        "SELECT ST_Contains(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_PointOnSurface(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))')))",
    );
    assert_eq!(c, 1);
}

#[$test_attr]
fn st_hausdorff_distance() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_HausdorffDistance(ST_GeomFromText('LINESTRING(0 0,1 0)'), ST_GeomFromText('LINESTRING(0 1,1 1)'))",
    );
    assert!((d - 1.0).abs() < 1e-10, "hausdorff = {d}");
}

#[$test_attr]
fn st_distance_sphere() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_DistanceSphere(ST_Point(-0.1278, 51.5074, 4326), ST_Point(2.3522, 48.8566, 4326))",
    );
    assert!(d > 300_000.0 && d < 400_000.0, "distance_sphere = {d}");
}

#[$test_attr]
fn st_distance_spheroid() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_DistanceSpheroid(ST_Point(-0.1278, 51.5074, 4326), ST_Point(2.3522, 48.8566, 4326))",
    );
    assert!(d > 300_000.0 && d < 400_000.0, "distance_spheroid = {d}");
}

#[$test_attr]
fn st_length_sphere() {
    let db = ActiveTestDb::open();
    let l = db.query_f64(
        "SELECT ST_LengthSphere(ST_GeomFromText('LINESTRING(-0.1278 51.5074, 2.3522 48.8566)', 4326))",
    );
    assert!(l > 300_000.0, "length_sphere = {l}");
}

#[$test_attr]
fn st_azimuth() {
    let db = ActiveTestDb::open();
    let a = db.query_f64("SELECT ST_Azimuth(ST_Point(0,0,4326), ST_Point(0,1,4326))");
    assert!(a.abs() < 1e-6, "azimuth = {a}");
}

#[$test_attr]
fn st_project() {
    let db = ActiveTestDb::open();
    let y = db.query_f64("SELECT ST_Y(ST_Project(ST_Point(0,0,4326), 111000.0, 0.0))");
    assert!((y - 1.0).abs() < 0.1, "y = {y}");
}

#[$test_attr]
fn st_project_rejects_non_finite_distance() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_IsEmpty(ST_Project(ST_Point(0,0,4326), 1e309, 0.0))")
        .expect_err("non-finite distance should be rejected");
    assert!(
        err.contains("distance must be finite"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_project_rejects_non_finite_azimuth() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_IsEmpty(ST_Project(ST_Point(0,0,4326), 1000.0, 1e309))")
        .expect_err("non-finite azimuth should be rejected");
    assert!(
        err.contains("azimuth must be finite"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_dwithin_sphere() {
    let db = ActiveTestDb::open();
    let yes = db.query_i64(
        "SELECT ST_DWithinSphere(
            ST_Point(-0.1278, 51.5074, 4326),
            ST_Point(2.3522, 48.8566, 4326),
            400000.0
        )",
    );
    let no = db.query_i64(
        "SELECT ST_DWithinSphere(
            ST_Point(-0.1278, 51.5074, 4326),
            ST_Point(2.3522, 48.8566, 4326),
            300000.0
        )",
    );
    assert_eq!(yes, 1);
    assert_eq!(no, 0);
}

#[$test_attr]
fn st_dwithin_spheroid() {
    let db = ActiveTestDb::open();
    let yes = db.query_i64(
        "SELECT ST_DWithinSpheroid(
            ST_Point(-0.1278, 51.5074, 4326),
            ST_Point(2.3522, 48.8566, 4326),
            400000.0
        )",
    );
    let no = db.query_i64(
        "SELECT ST_DWithinSpheroid(
            ST_Point(-0.1278, 51.5074, 4326),
            ST_Point(2.3522, 48.8566, 4326),
            300000.0
        )",
    );
    assert_eq!(yes, 1);
    assert_eq!(no, 0);
}

#[$test_attr]
fn st_distance_sphere_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_DistanceSphere(ST_Point(0,0), ST_Point(1,1))")
        .expect_err("SRID-less geodesic distance should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_distance_spheroid_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_DistanceSpheroid(ST_Point(0,0), ST_Point(1,1))")
        .expect_err("SRID-less geodesic distance should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_length_sphere_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_LengthSphere(ST_GeomFromText('LINESTRING(0 0,1 1)'))")
        .expect_err("SRID-less geodesic length should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_azimuth_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Azimuth(ST_Point(0,0), ST_Point(0,1))")
        .expect_err("SRID-less geodesic azimuth should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_project_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Project(ST_Point(0,0), 111000.0, 0.0)")
        .expect_err("SRID-less geodesic projection should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_dwithin_geodesic_requires_explicit_4326_srid() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_DWithinSphere(ST_Point(0,0), ST_Point(1,1), 1000.0)")
        .expect_err("SRID-less geodesic dwithin (sphere) should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");

    let err = db
        .try_query_i64("SELECT ST_DWithinSpheroid(ST_Point(0,0), ST_Point(1,1), 1000.0)")
        .expect_err("SRID-less geodesic dwithin (spheroid) should error");
    assert!(err.contains("requires SRID 4326"), "unexpected error: {err}");
}

#[$test_attr]
fn st_dwithin_rejects_non_finite_distance() {
    let db = ActiveTestDb::open();
    for sql in [
        "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), 1e309)",
        "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), -1e309)",
    ] {
        let err = db
            .try_query_i64(sql)
            .expect_err("non-finite distance should be rejected for ST_DWithin");
        assert!(
            err.contains("distance must be finite"),
            "unexpected error for `{sql}`: {err}"
        );
    }
}

#[$test_attr]
fn st_dwithin_rejects_negative_distance() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), -1.0)")
        .expect_err("negative distance should be rejected for ST_DWithin");
    assert!(
        err.contains("distance must be non-negative"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_dwithin_nan_distance_binds_as_null_in_sqlite() {
    let db = ActiveTestDb::open();
    let is_null = db
        .try_query_i64_with_f64_param(
            "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), ?1) IS NULL",
            f64::NAN,
        )
        .expect("NaN bind should produce a row");
    assert_eq!(is_null, 1);
}

#[$test_attr]
fn st_dwithin_negative_distance_bind_is_rejected() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64_with_f64_param(
            "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), ?1)",
            -1.0,
        )
        .expect_err("negative distance bind should be rejected for ST_DWithin");
    assert!(
        err.contains("distance must be non-negative"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_dwithin_sphere_nan_distance_binds_as_null_in_sqlite() {
    let db = ActiveTestDb::open();
    let is_null = db
        .try_query_i64_with_f64_param(
            "SELECT ST_DWithinSphere(ST_Point(0,0,4326), ST_Point(1,1,4326), ?1) IS NULL",
            f64::NAN,
        )
        .expect("NaN bind should produce a row");
    assert_eq!(is_null, 1);
}

#[$test_attr]
fn st_dwithin_spheroid_nan_distance_binds_as_null_in_sqlite() {
    let db = ActiveTestDb::open();
    let is_null = db
        .try_query_i64_with_f64_param(
            "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), ST_Point(1,1,4326), ?1) IS NULL",
            f64::NAN,
        )
        .expect("NaN bind should produce a row");
    assert_eq!(is_null, 1);
}

#[$test_attr]
fn st_dwithin_inf_distance_binds_are_rejected() {
    let db = ActiveTestDb::open();
    for distance in [f64::INFINITY, f64::NEG_INFINITY] {
        let err = db
            .try_query_i64_with_f64_param(
                "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), ?1)",
                distance,
            )
            .expect_err("infinite distance should be rejected for ST_DWithin");
        assert!(
            err.contains("distance must be finite"),
            "unexpected error for distance={distance}: {err}"
        );
    }
}

#[$test_attr]
fn st_dwithin_sphere_inf_distance_binds_are_rejected() {
    let db = ActiveTestDb::open();
    for distance in [f64::INFINITY, f64::NEG_INFINITY] {
        let err = db
            .try_query_i64_with_f64_param(
                "SELECT ST_DWithinSphere(ST_Point(0,0,4326), ST_Point(1,1,4326), ?1)",
                distance,
            )
            .expect_err("infinite distance should be rejected for ST_DWithinSphere");
        assert!(
            err.contains("distance must be finite"),
            "unexpected error for distance={distance}: {err}"
        );
    }
}

#[$test_attr]
fn st_dwithin_spheroid_inf_distance_binds_are_rejected() {
    let db = ActiveTestDb::open();
    for distance in [f64::INFINITY, f64::NEG_INFINITY] {
        let err = db
            .try_query_i64_with_f64_param(
                "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), ST_Point(1,1,4326), ?1)",
                distance,
            )
            .expect_err("infinite distance should be rejected for ST_DWithinSpheroid");
        assert!(
            err.contains("distance must be finite"),
            "unexpected error for distance={distance}: {err}"
        );
    }
}

#[$test_attr]
fn st_dwithin_geodesic_rejects_non_finite_distance() {
    let db = ActiveTestDb::open();
    for sql in [
        "SELECT ST_DWithinSphere(ST_Point(0,0,4326), ST_Point(1,1,4326), 1e309)",
        "SELECT ST_DWithinSphere(ST_Point(0,0,4326), ST_Point(1,1,4326), -1e309)",
        "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), ST_Point(1,1,4326), 1e309)",
        "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), ST_Point(1,1,4326), -1e309)",
    ] {
        let err = db
            .try_query_i64(sql)
            .expect_err("non-finite distance should be rejected for geodesic ST_DWithin");
        assert!(
            err.contains("distance must be finite"),
            "unexpected error for `{sql}`: {err}"
        );
    }
}

#[$test_attr]
fn st_dwithin_geodesic_rejects_negative_distance() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_DWithinSphere(ST_Point(0,0,4326), ST_Point(1,1,4326), -1.0)",
        )
        .expect_err("negative distance should be rejected for ST_DWithinSphere");
    assert!(
        err.contains("distance must be non-negative"),
        "unexpected error: {err}"
    );

    let err = db
        .try_query_i64(
            "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), ST_Point(1,1,4326), -1.0)",
        )
        .expect_err("negative distance should be rejected for ST_DWithinSpheroid");
    assert!(
        err.contains("distance must be non-negative"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_dwithin_geodesic_rejects_non_point_inputs() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_DWithinSphere(
                ST_GeomFromText('LINESTRING(0 0,1 1)', 4326),
                ST_Point(0,0,4326),
                1000.0
            )",
        )
        .expect_err("non-point input should be rejected for ST_DWithinSphere");
    assert!(err.contains("not a Point"), "unexpected error: {err}");

    let err = db
        .try_query_i64(
            "SELECT ST_DWithinSpheroid(
                ST_GeomFromText('LINESTRING(0 0,1 1)', 4326),
                ST_Point(0,0,4326),
                1000.0
            )",
        )
        .expect_err("non-point input should be rejected for ST_DWithinSpheroid");
    assert!(err.contains("not a Point"), "unexpected error: {err}");
}

#[$test_attr]
fn st_distance_empty_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Distance(ST_GeomFromText('POINT EMPTY'), ST_Point(0,0))")
        .expect_err("empty point should be rejected");
    assert!(
        err.contains("does not accept empty geometries"),
        "unexpected error: {err}"
    );
}

#[$test_attr]
fn st_distance_sphere_empty_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_DistanceSphere(ST_GeomFromText('POINT EMPTY', 4326), ST_Point(0,0,4326))",
        )
        .expect_err("empty point should be rejected");
    assert!(err.contains("does not accept empty points"), "unexpected error: {err}");
}

#[$test_attr]
fn st_distance_spheroid_empty_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_DistanceSpheroid(ST_GeomFromText('POINT EMPTY', 4326), ST_Point(0,0,4326))",
        )
        .expect_err("empty point should be rejected");
    assert!(err.contains("does not accept empty points"), "unexpected error: {err}");
}

#[$test_attr]
fn st_azimuth_empty_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Azimuth(ST_GeomFromText('POINT EMPTY', 4326), ST_Point(0,1,4326))")
        .expect_err("empty point should be rejected");
    assert!(err.contains("does not accept empty points"), "unexpected error: {err}");
}

#[$test_attr]
fn st_project_empty_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Project(ST_GeomFromText('POINT EMPTY', 4326), 1000.0, 0.0)")
        .expect_err("empty point should be rejected");
    assert!(err.contains("does not accept empty points"), "unexpected error: {err}");
}

#[$test_attr]
fn st_closest_point() {
    let db = ActiveTestDb::open();
    let y = db.query_f64(
        "SELECT ST_Y(ST_ClosestPoint(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_Point(5,5)))",
    );
    assert!(y.abs() < 1e-10, "y = {y}");
}

#[$test_attr]
fn st_closest_point_empty_target_point_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64(
            "SELECT ST_X(ST_ClosestPoint(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('POINT EMPTY')))",
        )
        .expect_err("empty target point should be rejected");
    assert!(err.contains("does not accept empty points"), "unexpected error: {err}");
}

// Predicates

#[$test_attr]
fn st_intersects() {
    let db = ActiveTestDb::open();
    let yes = db.query_i64(
        "SELECT ST_Intersects(
            ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'),
            ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))')
         )",
    );
    assert_eq!(yes, 1);

    let no = db.query_i64(
        "SELECT ST_Intersects(
            ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'),
            ST_GeomFromText('POLYGON((2 2,3 2,3 3,2 3,2 2))')
         )",
    );
    assert_eq!(no, 0);
}

#[$test_attr]
fn st_contains() {
    let db = ActiveTestDb::open();
    let yes = db.query_i64(
        "SELECT ST_Contains(
            ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'),
            ST_Point(2,2)
         )",
    );
    assert_eq!(yes, 1);
}

#[$test_attr]
fn st_dwithin() {
    let db = ActiveTestDb::open();
    let yes = db.query_i64("SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), 5.0)");
    assert_eq!(yes, 1);
    let no = db.query_i64("SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), 4.9)");
    assert_eq!(no, 0);
}

#[$test_attr]
fn st_within() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Within(ST_Point(2,2), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_disjoint() {
    let db = ActiveTestDb::open();
    let v = db.query_i64("SELECT ST_Disjoint(ST_Point(0,0), ST_Point(10,10))");
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_covers() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Covers(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_Point(2,2))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_covered_by() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_CoveredBy(ST_Point(2,2), ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_equals() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Equals(ST_GeomFromText('LINESTRING(0 0,1 1)'), ST_GeomFromText('LINESTRING(1 1,0 0)'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_touches() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Touches(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), ST_GeomFromText('POLYGON((1 0,2 0,2 1,1 1,1 0))'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_crosses() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Crosses(ST_GeomFromText('LINESTRING(-1 0.5,2 0.5)'), ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_overlaps() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Overlaps(ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'), ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))'))",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_relate() {
    let db = ActiveTestDb::open();
    let r = db.query_text("SELECT ST_Relate(ST_Point(0,0), ST_Point(0,0))");
    assert_eq!(r, "0FFFFFFF2");
}

#[$test_attr]
fn st_relate_pattern() {
    let db = ActiveTestDb::open();
    let v = db.query_i64(
        "SELECT ST_Relate(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), ST_Point(2,2), 'T*****FF*')",
    );
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_relate_match() {
    let db = ActiveTestDb::open();
    let v = db.query_i64("SELECT ST_RelateMatch('0FFFFFFF2', '0FFF*FFF2')");
    assert_eq!(v, 1);
}

#[$test_attr]
fn st_relate_pattern_invalid_pattern_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_Relate(ST_Point(0,0), ST_Point(0,0), 'INVALID')")
        .expect_err("invalid DE-9IM pattern should return an error");
    assert!(
        err.contains("invalid DE-9IM pattern"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_relate_match_invalid_pattern_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_RelateMatch('0FFFFFFF2', 'INVALID')")
        .expect_err("invalid DE-9IM pattern should return an error");
    assert!(
        err.contains("invalid DE-9IM pattern"),
        "unexpected error message: {err}"
    );
}

#[$test_attr]
fn st_relate_match_invalid_matrix_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_RelateMatch('INVALID', 'T*****FF*')")
        .expect_err("invalid DE-9IM matrix should return an error");
    assert!(
        err.contains("invalid DE-9IM matrix"),
        "unexpected error message: {err}"
    );
}

// Alias function tests

#[$test_attr]
fn st_make_point_alias() {
    let db = ActiveTestDb::open();
    let x = db.query_f64("SELECT ST_X(ST_MakePoint(7, 8))");
    assert!((x - 7.0).abs() < 1e-10, "x = {x}");
}

#[$test_attr]
fn geometry_type_alias() {
    let db = ActiveTestDb::open();
    let t = db.query_text("SELECT GeometryType(ST_GeomFromText('LINESTRING(0 0,1 1)'))");
    assert_eq!(t, "ST_LineString");
}

#[$test_attr]
fn st_num_interior_ring_alias() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumInteriorRing(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    );
    assert_eq!(n, 1);
}

#[$test_attr]
fn st_length2d_alias() {
    let db = ActiveTestDb::open();
    let l = db.query_f64("SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)'))");
    assert!((l - 5.0).abs() < 1e-10, "length2d = {l}");
}

#[$test_attr]
fn st_perimeter2d_alias() {
    let db = ActiveTestDb::open();
    let p =
        db.query_f64("SELECT ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))");
    assert!((p - 4.0).abs() < 1e-10, "perimeter2d = {p}");
}

// NULL input handling tests

#[$test_attr]
fn null_input_st_astext() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_AsText(NULL)"));
}

#[$test_attr]
fn null_input_st_geomfromtext() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeomFromText(NULL)"));
}

#[$test_attr]
fn st_geomfromtext_invalid_utf8_errors() {
    let db = ActiveTestDb::open();
    let err = db
        .try_query_i64("SELECT ST_GeomFromText(CAST(X'80' AS TEXT)) IS NULL")
        .expect_err("invalid UTF-8 ST_GeomFromText input should be a hard error");
    assert!(err.contains("UTF-8"), "unexpected error message: {err}");
}

#[$test_attr]
fn null_input_st_area() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Area(NULL)"));
}

#[$test_attr]
fn null_input_st_distance() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Distance(NULL, ST_Point(0,0))"));
    assert!(db.query_is_null("SELECT ST_Distance(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_intersects() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Intersects(NULL, ST_Point(0,0))"));
    assert!(db.query_is_null("SELECT ST_Intersects(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_srid() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_SRID(NULL)"));
}

#[$test_attr]
fn null_input_st_x() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_X(NULL)"));
}

#[$test_attr]
fn null_input_st_geometrytype() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeometryType(NULL)"));
}

#[$test_attr]
fn null_input_st_isempty() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_IsEmpty(NULL)"));
}

#[$test_attr]
fn null_input_st_centroid() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Centroid(NULL)"));
}

#[$test_attr]
fn null_input_st_geomfromgeojson() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeomFromGeoJSON(NULL)"));
}

#[$test_attr]
fn null_input_st_relate() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Relate(NULL, ST_Point(0,0))"));
    assert!(db.query_is_null("SELECT ST_Relate(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_relate_pattern() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Relate(NULL, ST_Point(0,0), 'T*****FF*')"));
    assert!(db.query_is_null("SELECT ST_Relate(ST_Point(0,0), NULL, 'T*****FF*')"));
    assert!(db.query_is_null("SELECT ST_Relate(ST_Point(0,0), ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_relatematch() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_RelateMatch(NULL, '0FFF*FFF2')"));
    assert!(db.query_is_null("SELECT ST_RelateMatch('0FFFFFFF2', NULL)"));
}

#[$test_attr]
fn null_input_st_closestpoint() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_ClosestPoint(NULL, ST_Point(0,0))"));
    assert!(
        db.query_is_null("SELECT ST_ClosestPoint(ST_GeomFromText('LINESTRING(0 0,1 1)'), NULL)")
    );
}

#[$test_attr]
fn null_input_st_makeline() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_MakeLine(NULL, ST_Point(0,0))"));
    assert!(db.query_is_null("SELECT ST_MakeLine(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_collect() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Collect(NULL, ST_Point(0,0))"));
    assert!(db.query_is_null("SELECT ST_Collect(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_setsrid() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_SetSRID(NULL, 4326)"));
}

#[$test_attr]
fn null_numeric_arg_st_setsrid_returns_null() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_SetSRID(ST_Point(0,0), NULL)"));
}

#[$test_attr]
fn null_input_st_pointn() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_PointN(NULL, 1)"));
}

#[$test_attr]
fn null_input_st_geometryn() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeometryN(NULL, 1)"));
}

#[$test_attr]
fn null_input_st_interiorringn() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_InteriorRingN(NULL, 1)"));
}

#[$test_attr]
fn null_input_st_project() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_Project(NULL, 100.0, 0.0)"));
}

#[$test_attr]
fn null_input_st_dwithin() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_DWithin(NULL, ST_Point(0,0), 5.0)"));
    assert!(db.query_is_null("SELECT ST_DWithin(ST_Point(0,0), NULL, 5.0)"));
    assert!(db.query_is_null(
        "SELECT ST_DWithinSphere(NULL, ST_Point(0,0,4326), 5.0)"
    ));
    assert!(db.query_is_null(
        "SELECT ST_DWithinSphere(ST_Point(0,0,4326), NULL, 5.0)"
    ));
    assert!(db.query_is_null(
        "SELECT ST_DWithinSpheroid(NULL, ST_Point(0,0,4326), 5.0)"
    ));
    assert!(db.query_is_null(
        "SELECT ST_DWithinSpheroid(ST_Point(0,0,4326), NULL, 5.0)"
    ));
}

#[$test_attr]
fn null_input_st_geomfromwkb() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeomFromWKB(NULL)"));
}

#[$test_attr]
fn null_input_st_geomfromewkb() {
    let db = ActiveTestDb::open();
    assert!(db.query_is_null("SELECT ST_GeomFromEWKB(NULL)"));
}

#[$test_attr]
fn empty_blob_input_reports_error_not_null() {
    let db = ActiveTestDb::open();
    let res = db.try_query_i64("SELECT ST_IsEmpty(X'')");
    assert!(res.is_err(), "empty blob should be rejected, got: {res:?}");
}

// Multi-geometry tests

#[$test_attr]
fn st_npoints_multipoint() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NPoints(ST_GeomFromText('MULTIPOINT((0 0),(1 1),(2 2))'))");
    assert_eq!(n, 3);
}

#[$test_attr]
fn st_npoints_multilinestring() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NPoints(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,3 3,4 4))'))",
    );
    assert_eq!(n, 5); // 2 + 3
}

#[$test_attr]
fn st_npoints_multipolygon() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NPoints(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))'))",
    );
    assert_eq!(n, 10); // 5 + 5
}

#[$test_attr]
fn st_npoints_geometrycollection() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NPoints(ST_GeomFromText('GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(1 1,2 2))'))",
    );
    assert_eq!(n, 3); // 1 + 2
}

#[$test_attr]
fn st_num_geometries_multipoint() {
    let db = ActiveTestDb::open();
    let n =
        db.query_i64("SELECT ST_NumGeometries(ST_GeomFromText('MULTIPOINT((0 0),(1 1),(2 2))'))");
    assert_eq!(n, 3);
}

#[$test_attr]
fn st_num_geometries_multilinestring() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumGeometries(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,3 3))'))",
    );
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_num_geometries_multipolygon() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumGeometries(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))'))",
    );
    assert_eq!(n, 2);
}

#[$test_attr]
fn st_num_geometries_single_point() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumGeometries(ST_Point(1, 2))");
    assert_eq!(n, 1);
}

#[$test_attr]
fn st_geometry_n_multilinestring() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumPoints(ST_GeometryN(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,3 3,4 4))'), 2))",
    );
    assert_eq!(n, 3);
}

#[$test_attr]
fn st_geometry_n_multipolygon() {
    let db = ActiveTestDb::open();
    let t = db.query_text(
        "SELECT ST_GeometryType(ST_GeometryN(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))'), 1))",
    );
    assert_eq!(t, "ST_Polygon");
}

#[$test_attr]
fn st_is_empty_linestring() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('LINESTRING EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_polygon() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('POLYGON EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_multipoint() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('MULTIPOINT EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_multilinestring() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('MULTILINESTRING EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_multipolygon() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('MULTIPOLYGON EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_geometrycollection() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_is_empty_collections_with_only_empty_members() {
    let db = ActiveTestDb::open();
    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('MULTILINESTRING(EMPTY,EMPTY)'))");
    assert_eq!(e, 1);

    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('MULTIPOLYGON(EMPTY)'))");
    assert_eq!(e, 1);

    let e = db.query_i64("SELECT ST_IsEmpty(ST_GeomFromText('GEOMETRYCOLLECTION(LINESTRING EMPTY)'))");
    assert_eq!(e, 1);
}

#[$test_attr]
fn st_perimeter_multipolygon() {
    let db = ActiveTestDb::open();
    let p = db.query_f64(
        "SELECT ST_Perimeter(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,4 2,4 4,2 4,2 2)))'))",
    );
    // First polygon perimeter = 4.0, second = 8.0, total = 12.0
    assert!((p - 12.0).abs() < 1e-10, "perimeter = {p}");
}

// Mixed-type distance tests

#[$test_attr]
fn st_distance_point_to_linestring() {
    let db = ActiveTestDb::open();
    let d =
        db.query_f64("SELECT ST_Distance(ST_Point(0,5), ST_GeomFromText('LINESTRING(0 0,10 0)'))");
    assert!((d - 5.0).abs() < 1e-10, "distance = {d}");
}

#[$test_attr]
fn st_distance_point_to_polygon() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_Distance(ST_Point(0,5), ST_GeomFromText('POLYGON((1 0,3 0,3 2,1 2,1 0))'))",
    );
    // Point (0,5) to nearest point on polygon border
    assert!(d > 0.0, "distance = {d}");
}

#[$test_attr]
fn st_distance_linestring_to_linestring() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_Distance(ST_GeomFromText('LINESTRING(0 0,10 0)'), ST_GeomFromText('LINESTRING(0 3,10 3)'))",
    );
    assert!((d - 3.0).abs() < 1e-10, "distance = {d}");
}

#[$test_attr]
fn st_distance_linestring_to_polygon() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_Distance(ST_GeomFromText('LINESTRING(0 5,10 5)'), ST_GeomFromText('POLYGON((0 0,10 0,10 2,0 2,0 0))'))",
    );
    assert!((d - 3.0).abs() < 1e-10, "distance = {d}");
}

#[$test_attr]
fn st_distance_polygon_to_polygon() {
    let db = ActiveTestDb::open();
    let d = db.query_f64(
        "SELECT ST_Distance(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'), ST_GeomFromText('POLYGON((3 0,4 0,4 1,3 1,3 0))'))",
    );
    assert!((d - 2.0).abs() < 1e-10, "distance = {d}");
}

// Validity edge cases

#[$test_attr]
fn st_is_valid_invalid_polygon() {
    let db = ActiveTestDb::open();
    // Bowtie / self-intersecting polygon
    let v = db.query_i64("SELECT ST_IsValid(ST_GeomFromText('POLYGON((0 0,2 2,2 0,0 2,0 0))'))");
    assert_eq!(v, 0);
}

#[$test_attr]
fn st_is_valid_reason_invalid_polygon() {
    let db = ActiveTestDb::open();
    let r =
        db.query_text("SELECT ST_IsValidReason(ST_GeomFromText('POLYGON((0 0,2 2,2 0,0 2,0 0))'))");
    // Should return something other than "Valid Geometry"
    assert_ne!(r, "Valid Geometry", "got: {r}");
}

// MultiLineString spherical length

#[$test_attr]
fn st_length_sphere_multilinestring() {
    let db = ActiveTestDb::open();
    let l = db.query_f64(
        "SELECT ST_LengthSphere(ST_GeomFromText('MULTILINESTRING((-0.1278 51.5074, 2.3522 48.8566),(2.3522 48.8566, 13.4050 52.5200))', 4326))",
    );
    // London->Paris + Paris->Berlin, should be > 600km
    assert!(l > 600_000.0, "length_sphere = {l}");
}

// MultiLineString planar length

#[$test_attr]
fn st_length_multilinestring() {
    let db = ActiveTestDb::open();
    let l =
        db.query_f64("SELECT ST_Length(ST_GeomFromText('MULTILINESTRING((0 0,3 4),(10 0,10 5))'))");
    // sqrt(9+16)=5 + 5=10, total=10
    assert!((l - 10.0).abs() < 1e-10, "length = {l}");
}

// Dimension for various types

#[$test_attr]
fn st_dimension_point() {
    let db = ActiveTestDb::open();
    let d = db.query_i64("SELECT ST_Dimension(ST_Point(0, 0))");
    assert_eq!(d, 0);
}

#[$test_attr]
fn st_dimension_linestring() {
    let db = ActiveTestDb::open();
    let d = db.query_i64("SELECT ST_Dimension(ST_GeomFromText('LINESTRING(0 0,1 1)'))");
    assert_eq!(d, 1);
}

#[$test_attr]
fn st_dimension_multipoint() {
    let db = ActiveTestDb::open();
    let d = db.query_i64("SELECT ST_Dimension(ST_GeomFromText('MULTIPOINT((0 0),(1 1))'))");
    assert_eq!(d, 0);
}

#[$test_attr]
fn st_dimension_multilinestring() {
    let db = ActiveTestDb::open();
    let d = db
        .query_i64("SELECT ST_Dimension(ST_GeomFromText('MULTILINESTRING((0 0,1 1),(2 2,3 3))'))");
    assert_eq!(d, 1);
}

#[$test_attr]
fn st_dimension_multipolygon() {
    let db = ActiveTestDb::open();
    let d = db
        .query_i64("SELECT ST_Dimension(ST_GeomFromText('MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))'))");
    assert_eq!(d, 2);
}

// Centroid of a LineString

#[$test_attr]
fn st_centroid_linestring() {
    let db = ActiveTestDb::open();
    let cx = db.query_f64("SELECT ST_X(ST_Centroid(ST_GeomFromText('LINESTRING(0 0,10 0)')))");
    assert!((cx - 5.0).abs() < 1e-10, "cx = {cx}");
}

// Num rings with holes

#[$test_attr]
fn st_num_rings_with_hole() {
    let db = ActiveTestDb::open();
    let n = db.query_i64(
        "SELECT ST_NumRings(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))'))",
    );
    assert_eq!(n, 2); // exterior + 1 interior
}

// Spatial Index tests

#[$test_attr]
fn spatial_index_create_query_drop() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec(
        "INSERT INTO places (geom) VALUES (ST_GeomFromText('POINT(1 2)')),\
         (ST_GeomFromText('POINT(3 4)')),\
         (ST_GeomFromText('POINT(5 6)'))",
    );

    // Create the spatial index
    let rc = db.query_i64("SELECT CreateSpatialIndex('places', 'geom')");
    assert_eq!(rc, 1);

    // R-tree should have 3 entries
    let count = db.query_i64("SELECT COUNT(*) FROM places_geom_rtree");
    assert_eq!(count, 3);

    // Query the R-tree directly
    let hits = db.query_all_i64(
        "SELECT id FROM places_geom_rtree WHERE xmin >= 2 AND xmax <= 6 AND ymin >= 3 AND ymax <= 7",
    );
    assert_eq!(hits.len(), 2); // POINT(3,4) and POINT(5,6)

    // Drop the spatial index
    let rc = db.query_i64("SELECT DropSpatialIndex('places', 'geom')");
    assert_eq!(rc, 1);

    // R-tree table should be gone
    let count = db.query_i64("SELECT COUNT(*) FROM sqlite_master WHERE name = 'places_geom_rtree'");
    assert_eq!(count, 0);
}

#[$test_attr]
fn spatial_index_create_idempotent() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec(
        "INSERT INTO pts (geom) VALUES (ST_Point(1, 2)), (ST_Point(3, 4)), (ST_Point(5, 6))",
    );

    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);
    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    // No duplicate rows after repeated create.
    let count = db.query_i64("SELECT COUNT(*) FROM pts_geom_rtree");
    assert_eq!(count, 3);
}

#[$test_attr]
fn spatial_index_create_rolls_back_when_population_fails() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE broken (id INTEGER PRIMARY KEY, geom INTEGER)");
    db.exec("INSERT INTO broken (geom) VALUES (42)");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('broken', 'geom')")
        .expect_err("index creation should fail for invalid geometry payloads");
    assert!(
        err.contains("invalid EWKB"),
        "unexpected error message: {err}"
    );
    assert!(
        !err.to_ascii_uppercase().contains("ROLLBACK"),
        "original populate error should not be overwritten by rollback errors: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'broken_geom_rtree'",
    );
    assert_eq!(rtree_exists, 0, "failed create should not leave rtree table");

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'broken_geom_%'",
    );
    assert_eq!(trigger_count, 0, "failed create should not leave triggers");
}

#[$test_attr]
fn spatial_index_drop_rejects_non_rtree_object_with_managed_name() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE broken (id INTEGER PRIMARY KEY, geom BLOB)");

    // Simulate an unexpected schema shape: object exists with the expected rtree name,
    // but it is a VIEW instead of a managed R-tree table.
    db.exec("CREATE VIEW broken_geom_rtree AS SELECT 1 AS id, 0.0 AS xmin, 0.0 AS xmax, 0.0 AS ymin, 0.0 AS ymax");
    db.exec("CREATE TRIGGER broken_geom_insert AFTER INSERT ON broken BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER broken_geom_update AFTER UPDATE OF geom ON broken BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER broken_geom_delete AFTER DELETE ON broken BEGIN SELECT 1; END");

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('broken', 'geom')")
        .expect_err("drop should reject non-table objects using managed names");
    assert!(
        err.contains("unexpected sqlite_master entry"),
        "unexpected error message: {err}"
    );

    // Safety check runs before any DROP statements, so all objects remain.
    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'broken_geom_%'",
    );
    assert_eq!(trigger_count, 3, "all triggers should remain after failed drop");

    let view_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view' AND name = 'broken_geom_rtree'",
    );
    assert_eq!(view_exists, 1, "view should remain after failed drop");
}

#[$test_attr]
fn spatial_index_drop_rolls_back_when_post_drop_catalog_delete_fails() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE broken (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("INSERT INTO broken (geom) VALUES (ST_Point(0, 0))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('broken', 'geom')");
    assert_eq!(rc, 1);

    db.exec(
        "CREATE TRIGGER sqlitegis_catalog_block_delete \
         BEFORE DELETE ON sqlitegis_spatial_index_catalog \
         BEGIN \
           SELECT RAISE(FAIL, 'catalog delete blocked'); \
         END",
    );

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('broken', 'geom')")
        .expect_err("drop should fail after entering savepoint and applying DROP statements");
    assert!(
        err.contains("catalog delete blocked"),
        "unexpected error message: {err}"
    );

    // Rollback must restore all managed index objects dropped earlier in the savepoint.
    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'broken_geom_rtree'",
    );
    assert_eq!(rtree_exists, 1, "rtree table should be restored by rollback");

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'broken_geom_%'",
    );
    assert_eq!(trigger_count, 3, "managed triggers should be restored by rollback");

    let catalog_row_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE prefix = 'broken_geom' AND table_name = 'broken' AND column_name = 'geom'",
    );
    assert_eq!(catalog_row_count, 1, "catalog marker should be restored by rollback");
}

#[$test_attr]
fn spatial_index_rtree_plus_exact_predicate() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE polys (id INTEGER PRIMARY KEY, geom BLOB)");
    // Two overlapping squares and one far away
    db.exec(
        "INSERT INTO polys (geom) VALUES \
         (ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))')),\
         (ST_GeomFromText('POLYGON((1 1,3 1,3 3,1 3,1 1))')),\
         (ST_GeomFromText('POLYGON((10 10,11 10,11 11,10 11,10 10))'))",
    );
    db.exec("SELECT CreateSpatialIndex('polys', 'geom')");

    // Two-stage query: coarse R-tree filter + exact ST_Intersects refinement
    let hits = db.query_all_i64(
        "SELECT p.id FROM polys p \
         JOIN polys_geom_rtree r ON p.rowid = r.id \
         WHERE r.xmax >= 0.5 AND r.xmin <= 2.5 AND r.ymax >= 0.5 AND r.ymin <= 2.5 \
         AND ST_Intersects(p.geom, ST_MakeEnvelope(0.5, 0.5, 2.5, 2.5))",
    );
    assert_eq!(hits.len(), 2); // polys 1 and 2
}

#[$test_attr]
fn spatial_index_trigger_sync() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("SELECT CreateSpatialIndex('t', 'geom')");

    // INSERT with non-NULL geom -> appears in R-tree
    db.exec("INSERT INTO t (geom) VALUES (ST_Point(1, 2))");
    let count = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree");
    assert_eq!(count, 1);

    // INSERT with NULL geom -> not in R-tree
    db.exec("INSERT INTO t (geom) VALUES (NULL)");
    let count = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree");
    assert_eq!(count, 1); // still 1

    // UPDATE geom -> R-tree updated
    db.exec("UPDATE t SET geom = ST_Point(10, 20) WHERE id = 1");
    let xmin = db.query_f64("SELECT xmin FROM t_geom_rtree WHERE id = 1");
    assert!((xmin - 10.0).abs() < 1e-10, "xmin = {xmin}");

    // UPDATE geom to NULL -> removed from R-tree
    db.exec("UPDATE t SET geom = NULL WHERE id = 1");
    let count = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree WHERE id = 1");
    assert_eq!(count, 0);

    // UPDATE NULL -> non-NULL -> added to R-tree
    db.exec("UPDATE t SET geom = ST_Point(7, 8) WHERE id = 2");
    let count = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree WHERE id = 2");
    assert_eq!(count, 1);

    // DELETE -> removed from R-tree
    db.exec("DELETE FROM t WHERE id = 2");
    let count = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree");
    assert_eq!(count, 0);
}

#[$test_attr]
fn spatial_index_rejects_without_rowid_tables() {
    // CreateSpatialIndex installs triggers that reference NEW.rowid /
    // OLD.rowid. Those references break against WITHOUT ROWID tables, so
    // CreateSpatialIndex must reject them up front and leave the database
    // free of any partial state.
    let db = ActiveTestDb::open();

    // Bootstrap the catalog with an unrelated successful index so we can
    // distinguish "no catalog row" from "no catalog table" in the assertions
    // below.
    db.exec("CREATE TABLE ok (id INTEGER PRIMARY KEY, geom BLOB)");
    let rc = db.query_i64("SELECT CreateSpatialIndex('ok', 'geom')");
    assert_eq!(rc, 1);

    db.exec("CREATE TABLE wr (id INTEGER PRIMARY KEY, geom BLOB) WITHOUT ROWID");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('wr', 'geom')")
        .expect_err("WITHOUT ROWID tables must be rejected");
    assert!(
        err.contains("WITHOUT ROWID"),
        "unexpected error message: {err}"
    );

    // No rtree shadow for the rejected table.
    let rtree_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'wr_geom_rtree'",
    );
    assert_eq!(rtree_count, 0);

    // No triggers for the rejected table.
    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name LIKE 'wr_geom_%'",
    );
    assert_eq!(trigger_count, 0);

    // No catalog row for the rejected table, and the prior 'ok' entry is
    // untouched.
    let catalog_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE table_name = 'wr' AND column_name = 'geom'",
    );
    assert_eq!(catalog_count, 0);
    let ok_catalog_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE table_name = 'ok' AND column_name = 'geom'",
    );
    assert_eq!(ok_catalog_count, 1);
}

#[$test_attr]
fn spatial_index_update_trigger_skips_unrelated_columns() {
    // The UPDATE trigger has a WHEN clause that filters out updates which
    // change neither the geometry blob nor the rowid. This test pins that
    // optimization. If the WHEN clause is removed, the rtree row will be
    // deleted and re-inserted on every UPDATE, which is wasteful but does
    // not produce a visible behavioral change. To detect that, this test
    // pins a single rtree row's content across an UPDATE of an unrelated
    // column and asserts that the bbox values are bitwise identical.
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE features (id INTEGER PRIMARY KEY, name TEXT, geom BLOB)");
    db.exec("INSERT INTO features (name, geom) VALUES ('a', ST_Point(1, 2))");
    db.exec("SELECT CreateSpatialIndex('features', 'geom')");

    let rtree_count_before = db.query_i64("SELECT COUNT(*) FROM features_geom_rtree");
    assert_eq!(rtree_count_before, 1);
    let xmin_before = db.query_f64("SELECT xmin FROM features_geom_rtree WHERE id = 1");

    // Update an unrelated column. The WHEN clause must suppress trigger
    // body execution since neither geom nor rowid changes.
    db.exec("UPDATE features SET name = 'b' WHERE id = 1");

    let rtree_count_after = db.query_i64("SELECT COUNT(*) FROM features_geom_rtree");
    assert_eq!(rtree_count_after, 1);
    let xmin_after = db.query_f64("SELECT xmin FROM features_geom_rtree WHERE id = 1");
    assert_eq!(
        xmin_before.to_bits(),
        xmin_after.to_bits(),
        "bbox must be unchanged across an unrelated-column UPDATE"
    );

    // Sanity: an UPDATE that DOES change geom still propagates.
    db.exec("UPDATE features SET geom = ST_Point(10, 20) WHERE id = 1");
    let xmin_after_geom = db.query_f64("SELECT xmin FROM features_geom_rtree WHERE id = 1");
    assert!(
        (xmin_after_geom - 10.0).abs() < 1e-10,
        "xmin = {xmin_after_geom}"
    );
}

#[$test_attr]
fn spatial_index_trigger_sync_tracks_rowid_updates() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("SELECT CreateSpatialIndex('t', 'geom')");
    db.exec("INSERT INTO t (geom) VALUES (ST_Point(1, 2))");

    let count_old = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree WHERE id = 1");
    assert_eq!(count_old, 1);

    db.exec("UPDATE t SET rowid = 42 WHERE rowid = 1");

    let count_stale = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree WHERE id = 1");
    assert_eq!(count_stale, 0, "stale rowid must be removed from index");

    let count_new = db.query_i64("SELECT COUNT(*) FROM t_geom_rtree WHERE id = 42");
    assert_eq!(count_new, 1, "new rowid must be inserted into index");
}

#[$test_attr]
fn spatial_index_narrows_candidates() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE grid (id INTEGER PRIMARY KEY, geom BLOB)");

    // Insert 100 points in a 10x10 grid: (0,0) through (9,9)
    for x in 0..10 {
        for y in 0..10 {
            db.exec(&format!(
                "INSERT INTO grid (geom) VALUES (ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("SELECT CreateSpatialIndex('grid', 'geom')");

    let full_scan = db.query_i64("SELECT COUNT(*) FROM grid");
    assert_eq!(full_scan, 100);

    // R-tree query for bbox [1.5,1.5 -> 3.5,3.5] should return only 4 points: (2,2),(2,3),(3,2),(3,3)
    let rtree_hits = db.query_all_i64(
        "SELECT g.id FROM grid g \
         JOIN grid_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmin >= 1.5 AND r.xmax <= 3.5 AND r.ymin >= 1.5 AND r.ymax <= 3.5",
    );
    assert_eq!(rtree_hits.len(), 4);
}

#[$test_attr]
fn spatial_index_ignores_empty_geometries() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE empties (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec(
        "INSERT INTO empties (id, geom) VALUES \
         (1, ST_Point(1, 2)), \
         (2, ST_GeomFromText('POLYGON EMPTY')), \
         (3, ST_GeomFromText('GEOMETRYCOLLECTION EMPTY'))",
    );

    let rc = db.query_i64("SELECT CreateSpatialIndex('empties', 'geom')");
    assert_eq!(rc, 1);

    let count = db.query_i64("SELECT COUNT(*) FROM empties_geom_rtree");
    assert_eq!(count, 1, "only non-empty geometries should be indexed");

    db.exec("UPDATE empties SET geom = ST_GeomFromText('POINT EMPTY') WHERE id = 1");
    let count = db.query_i64("SELECT COUNT(*) FROM empties_geom_rtree");
    assert_eq!(count, 0, "row should be removed when geometry becomes empty");

    db.exec("UPDATE empties SET geom = ST_Point(5, 6) WHERE id = 2");
    let count = db.query_i64("SELECT COUNT(*) FROM empties_geom_rtree");
    assert_eq!(count, 1, "row should be indexed when geometry becomes non-empty");
}

#[$test_attr]
fn spatial_index_rejects_invalid_names() {
    let db = ActiveTestDb::open();

    // SQL injection attempt
    let res = db.try_query_i64("SELECT CreateSpatialIndex('places; DROP TABLE x', 'geom')");
    assert!(res.is_err(), "should reject: {res:?}");

    // Empty name
    let res = db.try_query_i64("SELECT CreateSpatialIndex('', 'geom')");
    assert!(res.is_err(), "should reject empty: {res:?}");

    // Name with spaces
    let res = db.try_query_i64("SELECT CreateSpatialIndex('my table', 'geom')");
    assert!(res.is_err(), "should reject spaces: {res:?}");

    // DropSpatialIndex also validates
    let res = db.try_query_i64("SELECT DropSpatialIndex('ok', 'col name')");
    assert!(res.is_err(), "should reject spaces in col: {res:?}");
}

#[$test_attr]
fn spatial_index_rejects_colliding_object_names() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('a', 'b_c')")
        .expect_err("colliding index object names must be rejected");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    // Existing index must remain intact and must still belong to a_b/c.
    let row_count = db.query_i64("SELECT COUNT(*) FROM a_b_c_rtree");
    assert_eq!(row_count, 1);
    let trigger_count_on_a = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a'",
    );
    assert_eq!(trigger_count_on_a, 0);
}

#[$test_attr]
fn spatial_index_drop_rejects_colliding_object_names() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('a', 'b_c')")
        .expect_err("colliding index object names must be rejected on drop");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    // Existing index must remain intact and must still belong to a_b/c.
    let row_count = db.query_i64("SELECT COUNT(*) FROM a_b_c_rtree");
    assert_eq!(row_count, 1);
    let trigger_count_on_ab = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a_b'",
    );
    assert_eq!(trigger_count_on_ab, 3);
    let trigger_count_on_a = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a'",
    );
    assert_eq!(trigger_count_on_a, 0);
}

#[$test_attr]
fn spatial_index_rejects_colliding_create_when_all_owner_triggers_removed() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    db.exec("DROP TRIGGER a_b_c_insert");
    db.exec("DROP TRIGGER a_b_c_update");
    db.exec("DROP TRIGGER a_b_c_delete");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('a', 'b_c')")
        .expect_err("colliding create must still fail when all owner triggers are missing");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    let row_count = db.query_i64("SELECT COUNT(*) FROM a_b_c_rtree");
    assert_eq!(row_count, 1);
    let trigger_count_on_a = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a'",
    );
    assert_eq!(trigger_count_on_a, 0);
}

#[$test_attr]
fn spatial_index_rejects_colliding_create_when_owner_triggers_partially_removed() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    db.exec("DROP TRIGGER a_b_c_update");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('a', 'b_c')")
        .expect_err("colliding create must fail when owner triggers are only partially present");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    let row_count = db.query_i64("SELECT COUNT(*) FROM a_b_c_rtree");
    assert_eq!(row_count, 1);
    let trigger_count_on_ab = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a_b'",
    );
    assert_eq!(trigger_count_on_ab, 2);
    let trigger_count_on_a = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a'",
    );
    assert_eq!(trigger_count_on_a, 0);
}

#[$test_attr]
fn spatial_index_drop_rejects_colliding_drop_when_all_owner_triggers_removed() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    db.exec("DROP TRIGGER a_b_c_insert");
    db.exec("DROP TRIGGER a_b_c_update");
    db.exec("DROP TRIGGER a_b_c_delete");

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('a', 'b_c')")
        .expect_err("colliding drop must fail when all owner triggers are missing");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'a_b_c_rtree'",
    );
    assert_eq!(rtree_exists, 1);
}

#[$test_attr]
fn spatial_index_drop_rejects_colliding_drop_when_owner_triggers_partially_removed() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE a_b (id INTEGER PRIMARY KEY, c BLOB)");
    db.exec("CREATE TABLE a (id INTEGER PRIMARY KEY, b_c BLOB)");
    db.exec("INSERT INTO a_b (c) VALUES (ST_Point(0, 0))");
    db.exec("INSERT INTO a (b_c) VALUES (ST_Point(1, 1)), (ST_Point(2, 2))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('a_b', 'c')");
    assert_eq!(rc, 1);

    db.exec("DROP TRIGGER a_b_c_delete");

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('a', 'b_c')")
        .expect_err("colliding drop must fail when owner triggers are only partially present");
    assert!(
        err.contains("naming collision"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'a_b_c_rtree'",
    );
    assert_eq!(rtree_exists, 1);
    let trigger_count_on_ab = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a_b'",
    );
    assert_eq!(trigger_count_on_ab, 2);
    let trigger_count_on_a = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'a'",
    );
    assert_eq!(trigger_count_on_a, 0);
}

#[$test_attr]
fn spatial_index_null_identifiers_error_instead_of_null() {
    let db = ActiveTestDb::open();

    let create_null_table = db
        .try_query_i64("SELECT CreateSpatialIndex(NULL, 'geom')")
        .expect_err("NULL table must be a hard error");
    assert!(
        create_null_table.contains("table name must not be NULL"),
        "unexpected error message: {create_null_table}"
    );

    let create_null_column = db
        .try_query_i64("SELECT CreateSpatialIndex('pts', NULL)")
        .expect_err("NULL column must be a hard error");
    assert!(
        create_null_column.contains("column name must not be NULL"),
        "unexpected error message: {create_null_column}"
    );

    let drop_null_table = db
        .try_query_i64("SELECT DropSpatialIndex(NULL, 'geom')")
        .expect_err("NULL table must be a hard error");
    assert!(
        drop_null_table.contains("table name must not be NULL"),
        "unexpected error message: {drop_null_table}"
    );

    let drop_null_column = db
        .try_query_i64("SELECT DropSpatialIndex('pts', NULL)")
        .expect_err("NULL column must be a hard error");
    assert!(
        drop_null_column.contains("column name must not be NULL"),
        "unexpected error message: {drop_null_column}"
    );
}

#[$test_attr]
fn spatial_index_drop_idempotent() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("SELECT CreateSpatialIndex('pts', 'geom')");

    let rc = db.query_i64("SELECT DropSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    // Second drop should also succeed (IF EXISTS)
    let rc = db.query_i64("SELECT DropSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);
}

#[$test_attr]
fn spatial_index_fresh_db_drop_creates_empty_catalog_and_succeeds() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");

    let rc = db.query_i64("SELECT DropSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    let catalog_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'sqlitegis_spatial_index_catalog'",
    );
    assert_eq!(
        catalog_exists, 1,
        "drop on a fresh DB should lazily create the catalog table"
    );

    let catalog_rows = db.query_i64("SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog");
    assert_eq!(catalog_rows, 0, "fresh drop should leave an empty catalog");

    let managed_objects = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE name IN ('pts_geom_rtree', 'pts_geom_rtree_node', \
                        'pts_geom_rtree_parent', 'pts_geom_rtree_rowid', \
                        'pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(managed_objects, 0, "drop should not create managed objects");
}

#[$test_attr]
fn spatial_index_fresh_db_create_initializes_catalog_marker() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("INSERT INTO pts (geom) VALUES (ST_Point(1, 2)), (ST_Point(3, 4))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    let catalog_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'sqlitegis_spatial_index_catalog'",
    );
    assert_eq!(
        catalog_exists, 1,
        "create on a fresh DB should lazily create the catalog table"
    );

    let marker_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE prefix = 'pts_geom' AND table_name = 'pts' AND column_name = 'geom'",
    );
    assert_eq!(marker_count, 1, "create should persist exactly one ownership marker");

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(rtree_exists, 1);

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(trigger_count, 3);
}

#[$test_attr]
fn spatial_index_drop_cleans_marker_but_keeps_catalog_table() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("INSERT INTO pts (geom) VALUES (ST_Point(1, 2))");
    db.exec("SELECT CreateSpatialIndex('pts', 'geom')");

    let rc = db.query_i64("SELECT DropSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    let marker_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE prefix = 'pts_geom'",
    );
    assert_eq!(marker_count, 0, "drop should remove the ownership marker");

    let catalog_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'sqlitegis_spatial_index_catalog'",
    );
    assert_eq!(
        catalog_exists, 1,
        "drop should retain the catalog table even when empty"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(rtree_exists, 0);

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(trigger_count, 0);

    // Idempotent cleanup should keep the empty catalog table in place.
    let rc = db.query_i64("SELECT DropSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);
    let catalog_exists_after_second_drop = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'sqlitegis_spatial_index_catalog'",
    );
    assert_eq!(catalog_exists_after_second_drop, 1);
}

#[$test_attr]
fn spatial_index_create_fails_closed_when_catalog_marker_missing_but_objects_exist() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("INSERT INTO pts (geom) VALUES (ST_Point(0, 0))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    db.exec("DELETE FROM sqlitegis_spatial_index_catalog WHERE prefix = 'pts_geom'");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('pts', 'geom')")
        .expect_err("create must fail closed when ownership marker is missing");
    assert!(
        err.contains("cannot prove ownership"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(rtree_exists, 1, "existing managed table should remain untouched");

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(
        trigger_count, 3,
        "existing managed triggers should remain untouched"
    );
}

#[$test_attr]
fn spatial_index_drop_fails_closed_when_catalog_marker_missing_but_objects_exist() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("INSERT INTO pts (geom) VALUES (ST_Point(0, 0))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    db.exec("DELETE FROM sqlitegis_spatial_index_catalog WHERE prefix = 'pts_geom'");

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('pts', 'geom')")
        .expect_err("drop must fail closed when ownership marker is missing");
    assert!(
        err.contains("cannot prove ownership"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(rtree_exists, 1, "drop must not proceed without ownership proof");

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(trigger_count, 3, "drop must not remove triggers on ownership failure");
}

#[$test_attr]
fn spatial_index_external_catalog_row_reassignment_is_rejected() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("CREATE TABLE other (id INTEGER PRIMARY KEY, geom2 BLOB)");
    db.exec("INSERT INTO pts (geom) VALUES (ST_Point(0, 0))");

    let rc = db.query_i64("SELECT CreateSpatialIndex('pts', 'geom')");
    assert_eq!(rc, 1);

    db.exec(
        "UPDATE sqlitegis_spatial_index_catalog \
         SET table_name = 'other', column_name = 'geom2' \
         WHERE prefix = 'pts_geom'",
    );

    let create_err = db
        .try_query_i64("SELECT CreateSpatialIndex('pts', 'geom')")
        .expect_err("create should reject externally reassigned catalog ownership");
    assert!(
        create_err.contains("naming collision"),
        "unexpected error message: {create_err}"
    );

    let drop_err = db
        .try_query_i64("SELECT DropSpatialIndex('pts', 'geom')")
        .expect_err("drop should reject externally reassigned catalog ownership");
    assert!(
        drop_err.contains("naming collision"),
        "unexpected error message: {drop_err}"
    );

    let marker_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog \
         WHERE prefix = 'pts_geom' AND table_name = 'other' AND column_name = 'geom2'",
    );
    assert_eq!(marker_count, 1, "external marker rewrite should remain unchanged");

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(rtree_exists, 1, "managed table should remain after rejected operations");

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(
        trigger_count, 3,
        "managed triggers should remain after rejected operations"
    );
}

#[$test_attr]
fn spatial_index_create_fails_when_catalog_name_is_non_table_object() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec(
        "CREATE VIEW sqlitegis_spatial_index_catalog \
         AS SELECT 'stub' AS prefix, 'pts' AS table_name, 'geom' AS column_name",
    );

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('pts', 'geom')")
        .expect_err("create should fail when catalog name resolves to a non-table object");
    assert!(
        err.contains("invalid spatial index catalog object type"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(
        rtree_exists, 0,
        "failed create should not leave rtree table when catalog ensure fails"
    );

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'pts_geom_%'",
    );
    assert_eq!(
        trigger_count, 0,
        "failed create should not leave managed triggers when catalog ensure fails"
    );
}

#[$test_attr]
fn spatial_index_drop_fails_when_catalog_name_is_non_table_object() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("CREATE VIRTUAL TABLE pts_geom_rtree USING rtree(id, xmin, xmax, ymin, ymax)");
    db.exec("CREATE TRIGGER pts_geom_insert AFTER INSERT ON pts BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER pts_geom_update AFTER UPDATE ON pts BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER pts_geom_delete AFTER DELETE ON pts BEGIN SELECT 1; END");
    db.exec(
        "CREATE VIEW sqlitegis_spatial_index_catalog \
         AS SELECT 'pts_geom' AS prefix, 'pts' AS table_name, 'geom' AS column_name",
    );

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('pts', 'geom')")
        .expect_err("drop should fail when catalog name resolves to a non-table object");
    assert!(
        err.contains("invalid spatial index catalog object type"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(
        rtree_exists, 1,
        "drop should roll back managed table deletion when catalog delete fails"
    );

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(
        trigger_count, 3,
        "drop should roll back managed trigger deletion when catalog delete fails"
    );
}

#[$test_attr]
fn spatial_index_create_fails_when_catalog_schema_is_malformed() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("CREATE TABLE sqlitegis_spatial_index_catalog (prefix TEXT PRIMARY KEY)");

    let err = db
        .try_query_i64("SELECT CreateSpatialIndex('pts', 'geom')")
        .expect_err("create should fail when catalog schema is missing required columns");
    assert!(
        err.contains("invalid spatial index catalog schema"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(
        rtree_exists, 0,
        "failed create should not leave rtree table when catalog inspection fails"
    );

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'pts_geom_%'",
    );
    assert_eq!(
        trigger_count, 0,
        "failed create should not leave managed triggers when catalog inspection fails"
    );
}

#[$test_attr]
fn spatial_index_drop_fails_when_catalog_schema_is_malformed_without_dropping_managed_objects() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("CREATE VIRTUAL TABLE pts_geom_rtree USING rtree(id, xmin, xmax, ymin, ymax)");
    db.exec("CREATE TRIGGER pts_geom_insert AFTER INSERT ON pts BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER pts_geom_update AFTER UPDATE ON pts BEGIN SELECT 1; END");
    db.exec("CREATE TRIGGER pts_geom_delete AFTER DELETE ON pts BEGIN SELECT 1; END");
    db.exec("CREATE TABLE sqlitegis_spatial_index_catalog (prefix TEXT PRIMARY KEY)");

    let err = db
        .try_query_i64("SELECT DropSpatialIndex('pts', 'geom')")
        .expect_err("drop should fail when catalog schema is missing required columns");
    assert!(
        err.contains("invalid spatial index catalog schema"),
        "unexpected error message: {err}"
    );

    let rtree_exists = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'pts_geom_rtree'",
    );
    assert_eq!(
        rtree_exists, 1,
        "drop should fail before mutating managed table when catalog inspection fails"
    );

    let trigger_count = db.query_i64(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'trigger' AND name IN ('pts_geom_insert', 'pts_geom_update', 'pts_geom_delete')",
    );
    assert_eq!(
        trigger_count, 3,
        "drop should fail before mutating managed triggers when catalog inspection fails"
    );
}

// Boolean operations

#[$test_attr]
fn st_union_overlapping_polygons() {
    let db = ActiveTestDb::open();
    // Two 2x2 squares overlapping by 1x2 strip -> union area = 6
    let area = db.query_f64(
        "SELECT ST_Area(ST_Union(\
            ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'),\
            ST_GeomFromText('POLYGON((1 0,3 0,3 2,1 2,1 0))')\
         ))",
    );
    assert!((area - 6.0).abs() < 1e-10, "ST_Union area = {area}");
}

#[$test_attr]
fn st_union_disjoint_returns_combined_area() {
    let db = ActiveTestDb::open();
    let area = db.query_f64(
        "SELECT ST_Area(ST_Union(\
            ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'),\
            ST_GeomFromText('POLYGON((5 5,6 5,6 6,5 6,5 5))')\
         ))",
    );
    assert!((area - 2.0).abs() < 1e-10, "ST_Union disjoint area = {area}");
}

#[$test_attr]
fn st_intersection_overlapping_polygons() {
    let db = ActiveTestDb::open();
    // Intersection of the same two squares = 1x2 strip -> area = 2
    let area = db.query_f64(
        "SELECT ST_Area(ST_Intersection(\
            ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'),\
            ST_GeomFromText('POLYGON((1 0,3 0,3 2,1 2,1 0))')\
         ))",
    );
    assert!((area - 2.0).abs() < 1e-10, "ST_Intersection area = {area}");
}

#[$test_attr]
fn st_intersection_disjoint_is_empty() {
    let db = ActiveTestDb::open();
    let is_empty = db.query_i64(
        "SELECT ST_IsEmpty(ST_Intersection(\
            ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'),\
            ST_GeomFromText('POLYGON((5 5,6 5,6 6,5 6,5 5))')\
         ))",
    );
    assert_eq!(is_empty, 1, "disjoint intersection should be empty");
}

#[$test_attr]
fn st_difference_overlapping_polygons() {
    let db = ActiveTestDb::open();
    // A(2x2) minus overlap(1x2) = left strip, area = 2
    let area = db.query_f64(
        "SELECT ST_Area(ST_Difference(\
            ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'),\
            ST_GeomFromText('POLYGON((1 0,3 0,3 2,1 2,1 0))')\
         ))",
    );
    assert!((area - 2.0).abs() < 1e-10, "ST_Difference area = {area}");
}

#[$test_attr]
fn st_symdifference_overlapping_polygons() {
    let db = ActiveTestDb::open();
    // SymDiff of two 2x2 overlapping by 1x2 = 2 + 2 = 4
    let area = db.query_f64(
        "SELECT ST_Area(ST_SymDifference(\
            ST_GeomFromText('POLYGON((0 0,2 0,2 2,0 2,0 0))'),\
            ST_GeomFromText('POLYGON((1 0,3 0,3 2,1 2,1 0))')\
         ))",
    );
    assert!((area - 4.0).abs() < 1e-10, "ST_SymDifference area = {area}");
}

#[$test_attr]
fn st_buffer_point_has_positive_area() {
    let db = ActiveTestDb::open();
    let area = db.query_f64(
        "SELECT ST_Area(ST_Buffer(ST_GeomFromText('POINT(0 0)'), 1.0))",
    );
    // Area of a radius-1 circle approximately  pi approximately  3.14159
    assert!(
        (area - std::f64::consts::PI).abs() < 0.1,
        "ST_Buffer(point,1) area = {area}"
    );
}

#[$test_attr]
fn st_buffer_polygon_grows_area() {
    let db = ActiveTestDb::open();
    let area = db.query_f64(
        "SELECT ST_Area(ST_Buffer(ST_GeomFromText('POLYGON((0 0,4 0,4 4,0 4,0 0))'), 1.0))",
    );
    // Buffered square is larger than the original 4x4 = 16
    assert!(area > 16.0, "buffered polygon area ({area}) should exceed original");
}

#[$test_attr]
fn st_buffer_empty_returns_empty_polygon() {
    let db = ActiveTestDb::open();
    let is_empty = db.query_i64(
        "SELECT ST_IsEmpty(ST_Buffer(ST_GeomFromText('POINT EMPTY'), 1.0))",
    );
    assert_eq!(is_empty, 1, "buffer of empty geometry should be empty");
}

// Alias functions

#[$test_attr]
fn st_makepoint_is_alias_for_st_point() {
    let db = ActiveTestDb::open();
    let wkt = db.query_text("SELECT ST_AsText(ST_MakePoint(3.0, 4.0))");
    assert!(wkt.contains("POINT"), "ST_MakePoint WKT = {wkt}");
    assert!(wkt.contains("3"), "ST_MakePoint WKT = {wkt}");
    assert!(wkt.contains("4"), "ST_MakePoint WKT = {wkt}");

    // Coordinates must match ST_Point exactly
    let x = db.query_f64("SELECT ST_X(ST_MakePoint(1.5, 2.5))");
    assert!((x - 1.5).abs() < 1e-10, "ST_MakePoint X = {x}");
}

#[$test_attr]
fn geometry_type_is_alias_for_st_geometrytype() {
    let db = ActiveTestDb::open();
    let via_alias = db.query_text(
        "SELECT GeometryType(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    );
    let via_st = db.query_text(
        "SELECT ST_GeometryType(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    );
    assert_eq!(via_alias, via_st, "GeometryType must match ST_GeometryType");
    assert_eq!(via_alias, "ST_Polygon");
}

#[$test_attr]
fn st_numinteriorring_is_alias_for_st_numinteriorrings() {
    let db = ActiveTestDb::open();
    // Polygon with one hole
    let via_singular = db.query_i64(
        "SELECT ST_NumInteriorRing(\
            ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))')\
         )",
    );
    let via_plural = db.query_i64(
        "SELECT ST_NumInteriorRings(\
            ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))')\
         )",
    );
    assert_eq!(via_singular, 1);
    assert_eq!(via_singular, via_plural, "ST_NumInteriorRing must equal ST_NumInteriorRings");
}

#[$test_attr]
fn st_length2d_is_alias_for_st_length() {
    let db = ActiveTestDb::open();
    let via_2d = db.query_f64(
        "SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)'))",
    );
    let via_plain = db.query_f64(
        "SELECT ST_Length(ST_GeomFromText('LINESTRING(0 0,3 4)'))",
    );
    assert!((via_2d - 5.0).abs() < 1e-10, "ST_Length2D = {via_2d}");
    assert!((via_2d - via_plain).abs() < 1e-10, "ST_Length2D must equal ST_Length");
}

#[$test_attr]
fn st_perimeter2d_is_alias_for_st_perimeter() {
    let db = ActiveTestDb::open();
    let via_2d = db.query_f64(
        "SELECT ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    );
    let via_plain = db.query_f64(
        "SELECT ST_Perimeter(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))",
    );
    assert!((via_2d - 4.0).abs() < 1e-10, "ST_Perimeter2D = {via_2d}");
    assert!((via_2d - via_plain).abs() < 1e-10, "ST_Perimeter2D must equal ST_Perimeter");
}

// Group 1 correctness

#[$test_attr]
fn st_num_rings_empty_polygon() {
    let db = ActiveTestDb::open();
    let n = db.query_i64("SELECT ST_NumRings(ST_GeomFromText('POLYGON EMPTY'))");
    assert_eq!(n, 0);
}

#[$test_attr]
fn st_make_envelope_inverted_coords_errors() {
    let db = ActiveTestDb::open();
    db.try_query_i64("SELECT ST_MakeEnvelope(10, 0, 5, 5, 4326)")
        .expect_err("inverted xmin/xmax should return an error");
}

// Index-aware query pattern tests

#[$test_attr]
fn spatial_index_intersects_window() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE iw_grid (id INTEGER PRIMARY KEY, geom BLOB)");

    for x in 0..10 {
        for y in 0..10 {
            let id = x * 10 + y;
            db.exec(&format!(
                "INSERT INTO iw_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("SELECT CreateSpatialIndex('iw_grid', 'geom')");

    // Indexed: R-tree prefilter + ST_Intersects refinement
    let indexed = db.query_all_i64(
        "SELECT g.id FROM iw_grid g \
         JOIN iw_grid_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 2 AND r.xmin <= 5 \
           AND r.ymax >= 2 AND r.ymin <= 5 \
           AND ST_Intersects(g.geom, ST_MakeEnvelope(2, 2, 5, 5)) = 1 \
         ORDER BY g.id",
    );

    // Non-indexed reference
    let non_indexed = db.query_all_i64(
        "SELECT id FROM iw_grid \
         WHERE ST_Intersects(geom, ST_MakeEnvelope(2, 2, 5, 5)) = 1 \
         ORDER BY id",
    );

    assert_eq!(indexed, non_indexed, "indexed and non-indexed must match");
    assert_eq!(indexed.len(), 16); // (2..=5, 2..=5) = 4x4 = 16
}

#[$test_attr]
fn spatial_index_inside_polygon() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE ip_pts (id INTEGER PRIMARY KEY, geom BLOB)");

    db.exec(
        "INSERT INTO ip_pts (id, geom) VALUES \
            (1, ST_Point(5, 5)), \
            (2, ST_Point(0, 5)), \
            (3, ST_Point(50, 50))",
    );
    db.exec("SELECT CreateSpatialIndex('ip_pts', 'geom')");

    // Indexed: R-tree prefilter + ST_Within refinement
    let indexed = db.query_all_i64(
        "SELECT p.id FROM ip_pts p \
         JOIN ip_pts_geom_rtree r ON p.rowid = r.id \
         WHERE r.xmax >= 0 AND r.xmin <= 10 \
           AND r.ymax >= 0 AND r.ymin <= 10 \
           AND ST_Within(p.geom, ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')) = 1 \
         ORDER BY p.id",
    );

    // Non-indexed reference
    let non_indexed = db.query_all_i64(
        "SELECT id FROM ip_pts \
         WHERE ST_Within(geom, ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')) = 1 \
         ORDER BY id",
    );

    assert_eq!(indexed, non_indexed);
    // Only interior point (5,5) is strictly within. Boundary (0,5) is not
    assert_eq!(indexed, vec![1]);
}

#[$test_attr]
fn spatial_index_contains_point() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE ic_polys (id INTEGER PRIMARY KEY, geom BLOB)");

    db.exec(
        "INSERT INTO ic_polys (id, geom) VALUES \
            (1, ST_GeomFromText('POLYGON((0 0,5 0,5 5,0 5,0 0))')), \
            (2, ST_GeomFromText('POLYGON((0 0,20 0,20 20,0 20,0 0))')), \
            (3, ST_GeomFromText('POLYGON((50 50,60 50,60 60,50 60,50 50))'))",
    );
    db.exec("SELECT CreateSpatialIndex('ic_polys', 'geom')");

    // Indexed: R-tree point containment + ST_Contains refinement
    let indexed = db.query_all_i64(
        "SELECT p.id FROM ic_polys p \
         JOIN ic_polys_geom_rtree r ON p.rowid = r.id \
         WHERE r.xmin <= 3 AND r.xmax >= 3 \
           AND r.ymin <= 3 AND r.ymax >= 3 \
           AND ST_Contains(p.geom, ST_Point(3, 3)) = 1 \
         ORDER BY p.id",
    );

    // Non-indexed reference
    let non_indexed = db.query_all_i64(
        "SELECT id FROM ic_polys \
         WHERE ST_Contains(geom, ST_Point(3, 3)) = 1 \
         ORDER BY id",
    );

    assert_eq!(indexed, non_indexed);
    // Point (3,3) inside both 'small' and 'big', not 'far'
    assert_eq!(indexed, vec![1, 2]);
}

#[$test_attr]
fn spatial_index_geodesic_radius() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE igr_cities (id INTEGER PRIMARY KEY, name TEXT, geom BLOB)");

    db.exec(
        "INSERT INTO igr_cities (id, name, geom) VALUES \
            (1, 'London', ST_Point(-0.1278, 51.5074, 4326)), \
            (2, 'Paris',  ST_Point(2.3522, 48.8566, 4326)), \
            (3, 'Berlin', ST_Point(13.4050, 52.5200, 4326)), \
            (4, 'Tokyo',  ST_Point(139.6917, 35.6895, 4326))",
    );
    db.exec("SELECT CreateSpatialIndex('igr_cities', 'geom')");

    let lon: f64 = -0.1278;
    let lat: f64 = 51.5074;
    let radius_m: f64 = 400_000.0;
    let dlat = radius_m / 111_320.0;
    let dlon = radius_m / (111_320.0 * lat.to_radians().cos());

    // Indexed query
    let indexed = db.query_all_i64(&format!(
        "SELECT c.id FROM igr_cities c \
         JOIN igr_cities_geom_rtree r ON c.rowid = r.id \
         WHERE r.xmax >= {xmin} AND r.xmin <= {xmax} \
           AND r.ymax >= {ymin} AND r.ymin <= {ymax} \
           AND ST_DWithinSphere(c.geom, ST_Point({lon}, {lat}, 4326), {radius_m}) = 1 \
         ORDER BY c.id",
        xmin = lon - dlon,
        xmax = lon + dlon,
        ymin = lat - dlat,
        ymax = lat + dlat,
    ));

    // Non-indexed reference
    let non_indexed = db.query_all_i64(&format!(
        "SELECT id FROM igr_cities \
         WHERE ST_DWithinSphere(geom, ST_Point({lon}, {lat}, 4326), {radius_m}) = 1 \
         ORDER BY id",
    ));

    assert_eq!(indexed, non_indexed);
    // London->Paris approximately  344 km (within), London->Berlin approximately  930 km (outside)
    assert_eq!(indexed, vec![1, 2]);
}

#[$test_attr]
fn spatial_index_knn_nearest_n() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE knn_grid (id INTEGER PRIMARY KEY, geom BLOB)");

    for x in 0..10 {
        for y in 0..10 {
            let id = x * 10 + y;
            db.exec(&format!(
                "INSERT INTO knn_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("SELECT CreateSpatialIndex('knn_grid', 'geom')");

    // KNN: 5 nearest to (4.5, 4.5), search box half_w = 3
    // Use id as tiebreaker for deterministic ordering when distances are equal
    let indexed = db.query_all_i64(
        "SELECT g.id FROM knn_grid g \
         JOIN knn_grid_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 1.5 AND r.xmin <= 7.5 \
           AND r.ymax >= 1.5 AND r.ymin <= 7.5 \
         ORDER BY ST_Distance(g.geom, ST_Point(4.5, 4.5)), g.id \
         LIMIT 5",
    );

    // Non-indexed reference
    let non_indexed = db.query_all_i64(
        "SELECT id FROM knn_grid \
         ORDER BY ST_Distance(geom, ST_Point(4.5, 4.5)), id \
         LIMIT 5",
    );

    assert_eq!(indexed.len(), 5);
    assert_eq!(indexed, non_indexed, "indexed KNN must match non-indexed");

    // The 4 nearest points to (4.5, 4.5) are (4,4),(4,5),(5,4),(5,5) = ids 44,45,54,55
    let mut top4: Vec<i64> = indexed[..4].to_vec();
    top4.sort();
    assert_eq!(top4, vec![44, 45, 54, 55]);
}

#[$test_attr]
fn spatial_index_knn_nearest_n_geodesic() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE knn_cities (id INTEGER PRIMARY KEY, name TEXT, geom BLOB)");

    db.exec(
        "INSERT INTO knn_cities (id, name, geom) VALUES \
            (1, 'London', ST_Point(-0.1278, 51.5074, 4326)), \
            (2, 'Paris',  ST_Point(2.3522, 48.8566, 4326)), \
            (3, 'Berlin', ST_Point(13.4050, 52.5200, 4326)), \
            (4, 'Madrid', ST_Point(-3.7038, 40.4168, 4326)), \
            (5, 'Tokyo',  ST_Point(139.6917, 35.6895, 4326))",
    );
    db.exec("SELECT CreateSpatialIndex('knn_cities', 'geom')");

    // KNN: 3 nearest cities to Paris
    let lon: f64 = 2.3522;
    let lat: f64 = 48.8566;
    let search_radius_m: f64 = 2_000_000.0;
    let dlat = search_radius_m / 111_320.0;
    let dlon = search_radius_m / (111_320.0 * lat.to_radians().cos());

    let indexed = db.query_all_i64(&format!(
        "SELECT c.id FROM knn_cities c \
         JOIN knn_cities_geom_rtree r ON c.rowid = r.id \
         WHERE r.xmax >= {xmin} AND r.xmin <= {xmax} \
           AND r.ymax >= {ymin} AND r.ymin <= {ymax} \
         ORDER BY ST_DistanceSphere(c.geom, ST_Point({lon}, {lat}, 4326)) \
         LIMIT 3",
        xmin = lon - dlon,
        xmax = lon + dlon,
        ymin = lat - dlat,
        ymax = lat + dlat,
    ));

    // Non-indexed reference
    let non_indexed = db.query_all_i64(&format!(
        "SELECT id FROM knn_cities \
         ORDER BY ST_DistanceSphere(geom, ST_Point({lon}, {lat}, 4326)) \
         LIMIT 3",
    ));

    assert_eq!(indexed.len(), 3);
    assert_eq!(indexed, non_indexed, "indexed geodesic KNN must match non-indexed");

    // Paris->self=0, Paris->Londonapproximately 344km, Paris->Berlinapproximately 878km
    assert_eq!(indexed[0], 2, "Paris should be nearest to itself");
    assert_eq!(indexed[1], 1, "London should be second");
    assert_eq!(indexed[2], 3, "Berlin should be third");
}

// Index speed tests

#[cfg(not(target_arch = "wasm32"))]
fn elapsed_since_utc(start: chrono::DateTime<chrono::Utc>) -> std::time::Duration {
    (chrono::Utc::now() - start)
        .to_std()
        .unwrap_or(std::time::Duration::ZERO)
}

#[cfg(not(target_arch = "wasm32"))]
#[ignore = "perf-only: run in dedicated perf lane with --ignored"]
#[$test_attr]
fn spatial_index_accelerates_intersects_window() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE sw_grid (id INTEGER PRIMARY KEY, geom BLOB)");

    // 10 000 points in a 100x100 grid
    db.exec("BEGIN");
    for x in 0..100 {
        for y in 0..100 {
            let id = x * 100 + y;
            db.exec(&format!(
                "INSERT INTO sw_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("COMMIT");
    db.exec("SELECT CreateSpatialIndex('sw_grid', 'geom')");

    let indexed_sql =
        "SELECT g.id FROM sw_grid g \
         JOIN sw_grid_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 10 AND r.xmin <= 20 \
           AND r.ymax >= 10 AND r.ymin <= 20 \
           AND ST_Intersects(g.geom, ST_MakeEnvelope(10, 10, 20, 20)) = 1";
    let full_scan_sql =
        "SELECT id FROM sw_grid \
         WHERE ST_Intersects(geom, ST_MakeEnvelope(10, 10, 20, 20)) = 1";

    // Warmup
    let _ = db.query_all_i64(indexed_sql);
    let _ = db.query_all_i64(full_scan_sql);

    // Take best of 20 runs
    let n = 20;
    let mut indexed_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(indexed_sql);
        indexed_best = indexed_best.min(elapsed_since_utc(t));
    }

    let mut full_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(full_scan_sql);
        full_best = full_best.min(elapsed_since_utc(t));
    }

    // Sanity: both return the same count
    let idx_count = db.query_all_i64(indexed_sql).len();
    let full_count = db.query_all_i64(full_scan_sql).len();
    assert_eq!(idx_count, full_count);
    assert_eq!(idx_count, 121); // 11x11 points in [10,20]

    eprintln!("intersects_window 10K: indexed={indexed_best:?}  full_scan={full_best:?}  speedup={:.1}x", full_best.as_nanos() as f64 / indexed_best.as_nanos() as f64);
    assert!(
        indexed_best < full_best,
        "indexed ({indexed_best:?}) should be faster than full scan ({full_best:?}) \
         over 10K rows"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[ignore = "perf-only: run in dedicated perf lane with --ignored"]
#[$test_attr]
fn spatial_index_accelerates_knn() {
    let db = ActiveTestDb::open();
    db.exec("CREATE TABLE sk_grid (id INTEGER PRIMARY KEY, geom BLOB)");

    db.exec("BEGIN");
    for x in 0..100 {
        for y in 0..100 {
            let id = x * 100 + y;
            db.exec(&format!(
                "INSERT INTO sk_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("COMMIT");
    db.exec("SELECT CreateSpatialIndex('sk_grid', 'geom')");

    let indexed_sql =
        "SELECT g.id FROM sk_grid g \
         JOIN sk_grid_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 45 AND r.xmin <= 55 \
           AND r.ymax >= 45 AND r.ymin <= 55 \
         ORDER BY ST_Distance(g.geom, ST_Point(50, 50)), g.id \
         LIMIT 5";
    let full_scan_sql =
        "SELECT id FROM sk_grid \
         ORDER BY ST_Distance(geom, ST_Point(50, 50)), id \
         LIMIT 5";

    let _ = db.query_all_i64(indexed_sql);
    let _ = db.query_all_i64(full_scan_sql);

    let n = 20;
    let mut indexed_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(indexed_sql);
        indexed_best = indexed_best.min(elapsed_since_utc(t));
    }

    let mut full_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(full_scan_sql);
        full_best = full_best.min(elapsed_since_utc(t));
    }

    // Both must return the same top-5
    let idx_ids = db.query_all_i64(indexed_sql);
    let full_ids = db.query_all_i64(full_scan_sql);
    assert_eq!(idx_ids, full_ids);

    eprintln!("knn 10K: indexed={indexed_best:?}  full_scan={full_best:?}  speedup={:.1}x", full_best.as_nanos() as f64 / indexed_best.as_nanos() as f64);
    assert!(
        indexed_best < full_best,
        "indexed KNN ({indexed_best:?}) should be faster than full scan ({full_best:?}) \
         over 10K rows"
    );
}

// --- Type-aware index strategy benchmark ---

#[cfg(not(target_arch = "wasm32"))]
#[ignore = "perf-only: run in dedicated perf lane with --ignored"]
#[$test_attr]
fn type_partitioned_vs_mixed_index() {
    let db = ActiveTestDb::open();

    // --- Mixed-type table: 7000 Points + 2000 LineStrings + 1000 Polygons ---
    db.exec("CREATE TABLE mixed (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("BEGIN");
    let mut id = 0i64;
    // 7000 Points in a 100x70 grid
    for x in 0..100 {
        for y in 0..70 {
            db.exec(&format!(
                "INSERT INTO mixed (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ));
            id += 1;
        }
    }
    // 2000 short LineStrings
    for i in 0..2000 {
        let x = (i % 100) as f64;
        let y = (i / 100) as f64 * 5.0;
        db.exec(&format!(
            "INSERT INTO mixed (id, geom) VALUES ({id}, \
             ST_GeomFromText('LINESTRING({x} {y}, {} {})'))",
            x + 1.0, y + 1.0
        ));
        id += 1;
    }
    // 1000 unit-square Polygons
    for i in 0..1000 {
        let x = (i % 50) as f64 * 2.0;
        let y = (i / 50) as f64 * 2.0;
        db.exec(&format!(
            "INSERT INTO mixed (id, geom) VALUES ({id}, \
             ST_GeomFromText('POLYGON(({x} {y}, {} {y}, {} {}, {x} {}, {x} {y}))'))",
            x + 1.0, x + 1.0, y + 1.0, y + 1.0
        ));
        id += 1;
    }
    db.exec("COMMIT");
    db.exec("SELECT CreateSpatialIndex('mixed', 'geom')");

    // --- Points-only table: same 7000 points ---
    db.exec("CREATE TABLE pts_only (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("BEGIN");
    for x in 0..100 {
        for y in 0..70 {
            let pid = x * 70 + y;
            db.exec(&format!(
                "INSERT INTO pts_only (id, geom) VALUES ({pid}, ST_Point({x}, {y}))"
            ));
        }
    }
    db.exec("COMMIT");
    db.exec("SELECT CreateSpatialIndex('pts_only', 'geom')");

    // --- Polygons-only table: same 1000 polygons ---
    db.exec("CREATE TABLE poly_only (id INTEGER PRIMARY KEY, geom BLOB)");
    db.exec("BEGIN");
    for i in 0..1000 {
        let x = (i % 50) as f64 * 2.0;
        let y = (i / 50) as f64 * 2.0;
        db.exec(&format!(
            "INSERT INTO poly_only (id, geom) VALUES ({i}, \
             ST_GeomFromText('POLYGON(({x} {y}, {} {y}, {} {}, {x} {}, {x} {y}))'))",
            x + 1.0, x + 1.0, y + 1.0, y + 1.0
        ));
    }
    db.exec("COMMIT");
    db.exec("SELECT CreateSpatialIndex('poly_only', 'geom')");

    // ===== Benchmark 1: "Find Points in window [40,40]-[60,60]" =====
    let mixed_pts_sql =
        "SELECT g.id FROM mixed g \
         JOIN mixed_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 40 AND r.xmin <= 60 \
           AND r.ymax >= 40 AND r.ymin <= 60 \
           AND ST_GeometryType(g.geom) = 'ST_Point' \
           AND ST_Intersects(g.geom, ST_MakeEnvelope(40, 40, 60, 60)) = 1";
    let pts_only_sql =
        "SELECT g.id FROM pts_only g \
         JOIN pts_only_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 40 AND r.xmin <= 60 \
           AND r.ymax >= 40 AND r.ymin <= 60 \
           AND ST_Intersects(g.geom, ST_MakeEnvelope(40, 40, 60, 60)) = 1";

    // Verify correctness: same point counts
    let mixed_pts_count = db.query_all_i64(mixed_pts_sql).len();
    let pts_only_count = db.query_all_i64(pts_only_sql).len();
    assert_eq!(mixed_pts_count, pts_only_count,
        "mixed type-filtered ({mixed_pts_count}) vs pts-only ({pts_only_count})");
    assert_eq!(pts_only_count, 21 * 21); // points [40..60] x [40..60]

    // Warmup
    let _ = db.query_all_i64(mixed_pts_sql);
    let _ = db.query_all_i64(pts_only_sql);

    let n = 20;
    let mut mixed_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(mixed_pts_sql);
        mixed_best = mixed_best.min(elapsed_since_utc(t));
    }
    let mut pts_only_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(pts_only_sql);
        pts_only_best = pts_only_best.min(elapsed_since_utc(t));
    }

    // Also benchmark mixed table WITHOUT type filter (all types)
    let mixed_all_sql =
        "SELECT g.id FROM mixed g \
         JOIN mixed_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmax >= 40 AND r.xmin <= 60 \
           AND r.ymax >= 40 AND r.ymin <= 60 \
           AND ST_Intersects(g.geom, ST_MakeEnvelope(40, 40, 60, 60)) = 1";
    let _ = db.query_all_i64(mixed_all_sql);
    let mut mixed_all_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(mixed_all_sql);
        mixed_all_best = mixed_all_best.min(elapsed_since_utc(t));
    }

    eprintln!("=== Type-Partitioned vs Mixed Index (Points in window) ===");
    eprintln!("  pts-only table (7K rows):     {:?}", pts_only_best);
    eprintln!("  mixed table + type filter (10K rows): {:?}", mixed_best);
    eprintln!("  mixed table, no filter (10K rows):    {:?}", mixed_all_best);
    eprintln!("  overhead of mixed+filter vs pts-only: {:.1}x",
        mixed_best.as_nanos() as f64 / pts_only_best.as_nanos() as f64);

    // Benchmark 2: reverse geocode, find Polygon containing (25, 9)
    let mixed_poly_sql =
        "SELECT g.id FROM mixed g \
         JOIN mixed_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmin <= 25 AND r.xmax >= 25 \
           AND r.ymin <= 9 AND r.ymax >= 9 \
           AND ST_GeometryType(g.geom) = 'ST_Polygon' \
           AND ST_Contains(g.geom, ST_Point(25, 9)) = 1";
    let poly_only_sql =
        "SELECT g.id FROM poly_only g \
         JOIN poly_only_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmin <= 25 AND r.xmax >= 25 \
           AND r.ymin <= 9 AND r.ymax >= 9 \
           AND ST_Contains(g.geom, ST_Point(25, 9)) = 1";

    let mixed_poly_count = db.query_all_i64(mixed_poly_sql).len();
    let poly_only_count = db.query_all_i64(poly_only_sql).len();
    assert_eq!(mixed_poly_count, poly_only_count,
        "mixed poly ({mixed_poly_count}) vs poly-only ({poly_only_count})");

    let _ = db.query_all_i64(mixed_poly_sql);
    let _ = db.query_all_i64(poly_only_sql);

    let mut mixed_poly_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(mixed_poly_sql);
        mixed_poly_best = mixed_poly_best.min(elapsed_since_utc(t));
    }
    let mut poly_only_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(poly_only_sql);
        poly_only_best = poly_only_best.min(elapsed_since_utc(t));
    }

    // Mixed without type filter (all R-tree hits get ST_Contains)
    let mixed_poly_nofilter_sql =
        "SELECT g.id FROM mixed g \
         JOIN mixed_geom_rtree r ON g.rowid = r.id \
         WHERE r.xmin <= 25 AND r.xmax >= 25 \
           AND r.ymin <= 9 AND r.ymax >= 9 \
           AND ST_Contains(g.geom, ST_Point(25, 9)) = 1";
    let _ = db.query_all_i64(mixed_poly_nofilter_sql);
    let mut mixed_poly_nofilter_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _ = db.query_all_i64(mixed_poly_nofilter_sql);
        mixed_poly_nofilter_best = mixed_poly_nofilter_best.min(elapsed_since_utc(t));
    }

    eprintln!("=== Type-Partitioned vs Mixed Index (Reverse Geocode) ===");
    eprintln!("  poly-only table (1K rows):    {:?}", poly_only_best);
    eprintln!("  mixed table + type filter (10K rows): {:?}", mixed_poly_best);
    eprintln!("  mixed table, no filter (10K rows):    {:?}", mixed_poly_nofilter_best);
    eprintln!("  overhead of mixed+filter vs poly-only: {:.1}x",
        mixed_poly_best.as_nanos() as f64 / poly_only_best.as_nanos() as f64);
}

    };
}
