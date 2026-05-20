// Shared test logic for Diesel + SQLite integration tests (native and WASM).
//
// Included via `include!()` by both `diesel_sqlite_integration.rs` and
// `diesel_wasm_integration.rs`, following the same pattern as
// `sqlite_test_db_macro.rs`.

diesel::table! {
    features (id) {
        id   -> Integer,
        name -> Text,
        geom -> Nullable<sqlitegis::diesel::Geometry>,
    }
}

// Tables for index-aware query pattern tests
diesel::table! { grid (id) { id -> Integer, name -> Text, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { iw_grid (id) { id -> Integer, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { ip_pts (id) { id -> Integer, name -> Text, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { ic_polys (id) { id -> Integer, name -> Text, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { igr_cities (id) { id -> Integer, name -> Text, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { knn_grid (id) { id -> Integer, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { knn_cities (id) { id -> Integer, name -> Text, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { sw_grid (id) { id -> Integer, geom -> Nullable<sqlitegis::diesel::Geometry>, } }
diesel::table! { sk_grid (id) { id -> Integer, geom -> Nullable<sqlitegis::diesel::Geometry>, } }

diesel::table! { grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { iw_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { ip_pts_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { ic_polys_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { igr_cities_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { knn_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { knn_cities_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { sw_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { sk_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }

diesel::allow_tables_to_appear_in_same_query!(grid, grid_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(iw_grid, iw_grid_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(ip_pts, ip_pts_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(ic_polys, ic_polys_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(igr_cities, igr_cities_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(knn_grid, knn_grid_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(knn_cities, knn_cities_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(sw_grid, sw_grid_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(sk_grid, sk_grid_geom_rtree);

#[derive(Queryable, Debug)]
#[diesel(table_name = features)]
struct Feature {
    #[allow(dead_code)]
    id: i32,
    name: String,
    geom: Option<geo::Geometry<f64>>,
}

#[derive(QueryableByName, Debug)]
struct TextResult {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
    val: Option<String>,
}

#[derive(QueryableByName, Debug)]
struct F64Result {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Double>)]
    val: Option<f64>,
}

#[derive(QueryableByName, Debug)]
struct I32Result {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Integer>)]
    val: Option<i32>,
}

fn setup_features_table(c: &mut SqliteConnection) {
    diesel::sql_query(
        "CREATE TABLE features (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            geom BLOB
        )",
    )
    .execute(c)
    .unwrap();
}

// Spatial function execution via Diesel

macro_rules! define_diesel_sqlite_tests {
    ($test_attr:meta) => {

// ST_Point + ST_AsText

#[$test_attr]
fn st_point_and_astext() {
    let mut c = conn();
    let result: TextResult = diesel::sql_query(
        "SELECT ST_AsText(ST_Point(1.5, 2.5)) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = result.val.expect("should not be NULL");
    assert!(wkt.contains("POINT"), "got: {wkt}");
    assert!(wkt.contains("1.5"), "got: {wkt}");
    assert!(wkt.contains("2.5"), "got: {wkt}");
}

#[$test_attr]
fn st_point_srid_execution() {
    let mut c = conn();
    let result: I32Result = diesel::sql_query("SELECT ST_SRID(ST_Point(1.5, 2.5, 4326)) AS val")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(result.val, Some(4326));
}

#[$test_attr]
fn st_point_rejects_non_finite_coordinates_execution() {
    let mut c = conn();
    let err = diesel::sql_query("SELECT ST_IsValid(ST_Point(1e309, 0)) AS val")
        .get_result::<I32Result>(&mut c)
        .expect_err("non-finite ST_Point coordinates must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("coordinates must be finite"), "got: {msg}");
}

#[$test_attr]
fn st_makeenvelope_srid_execution() {
    let mut c = conn();
    let result: I32Result =
        diesel::sql_query("SELECT ST_SRID(ST_MakeEnvelope(0, 0, 1, 1, 3857)) AS val")
            .get_result(&mut c)
            .unwrap();
    assert_eq!(result.val, Some(3857));
}

#[$test_attr]
fn st_geomfromwkb_srid_execution() {
    let mut c = conn();
    let result: I32Result = diesel::sql_query(
        "SELECT ST_SRID(ST_GeomFromWKB(ST_AsBinary(ST_Point(1.5, 2.5)), 4326)) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result.val, Some(4326));
}

#[$test_attr]
fn st_geomfromgeojson_srid_override_uses_setsrid_execution() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let srid: Option<i32> = diesel::dsl::select(st_srid(st_setsrid(
        st_geomfromgeojson(r#"{"type":"Point","coordinates":[1,2]}"#),
        3857,
    )))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(srid, Some(3857));
}

// ST_Distance

#[$test_attr]
fn st_distance_execution() {
    let mut c = conn();
    let result: F64Result = diesel::sql_query(
        "SELECT ST_Distance(ST_Point(0, 0), ST_Point(3, 4)) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let dist = result.val.expect("should not be NULL");
    assert!((dist - 5.0).abs() < 1e-10, "expected 5.0, got {dist}");
}

#[$test_attr]
fn st_makeline_rejects_empty_points_execution() {
    let mut c = conn();
    let err = diesel::sql_query(
        "SELECT ST_NumPoints(ST_MakeLine(ST_GeomFromText('POINT EMPTY'), ST_Point(1,1))) AS val",
    )
    .get_result::<I32Result>(&mut c)
    .expect_err("empty point input must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("point must not be empty"), "got: {msg}");
}

// ST_GeomFromText

#[$test_attr]
fn st_geomfromtext_execution() {
    let mut c = conn();
    let result: TextResult = diesel::sql_query(
        "SELECT ST_AsText(ST_GeomFromText('LINESTRING(0 0, 1 1, 2 0)')) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = result.val.expect("should not be NULL");
    assert!(wkt.contains("LINESTRING"), "got: {wkt}");
}

// ST_Area

#[$test_attr]
fn st_area_execution() {
    let mut c = conn();
    let result: F64Result = diesel::sql_query(
        "SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let area = result.val.expect("should not be NULL");
    assert!((area - 100.0).abs() < 1e-10, "expected 100.0, got {area}");
}

// ST_Centroid

#[$test_attr]
fn st_centroid_execution() {
    let mut c = conn();
    let result: TextResult = diesel::sql_query(
        "SELECT ST_AsText(ST_Centroid(ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'))) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = result.val.expect("should not be NULL");
    assert!(wkt.contains("POINT"), "centroid should be a point, got: {wkt}");
    assert!(wkt.contains("5"), "centroid should contain 5, got: {wkt}");
}

// Diesel query builder: function-style

#[$test_attr]
fn diesel_select_st_point() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let wkt: Option<String> = diesel::dsl::select(st_astext(st_point(13.4, 52.5).nullable()))
        .get_result(&mut c)
        .unwrap();
    let wkt = wkt.expect("should not be NULL");
    assert!(wkt.contains("POINT"), "got: {wkt}");
    assert!(wkt.contains("13.4"), "got: {wkt}");
}

#[$test_attr]
fn diesel_select_st_distance() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let dist: Option<f64> = diesel::dsl::select(st_distance(
        st_point(0.0, 0.0).nullable(),
        st_point(3.0, 4.0).nullable(),
    ))
    .get_result(&mut c)
    .unwrap();
    let dist = dist.expect("should not be NULL");
    assert!((dist - 5.0).abs() < 1e-10, "expected 5.0, got {dist}");
}

#[$test_attr]
fn diesel_select_st_area() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let area: Option<f64> = diesel::dsl::select(st_area(
        st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"),
    ))
    .get_result(&mut c)
    .unwrap();
    let area = area.expect("should not be NULL");
    assert!((area - 100.0).abs() < 1e-10, "expected 100.0, got {area}");
}

#[$test_attr]
fn diesel_select_st_intersects() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    // Two overlapping squares
    let result: Option<bool> = diesel::dsl::select(st_intersects(
        st_geomfromtext("POLYGON((0 0,2 0,2 2,0 2,0 0))"),
        st_geomfromtext("POLYGON((1 1,3 1,3 3,1 3,1 1))"),
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(true));

    // Two non-overlapping squares
    let result: Option<bool> = diesel::dsl::select(st_intersects(
        st_geomfromtext("POLYGON((0 0,1 0,1 1,0 1,0 0))"),
        st_geomfromtext("POLYGON((5 5,6 5,6 6,5 6,5 5))"),
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(false));
}

#[$test_attr]
fn diesel_select_st_dwithin() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    // Points within distance
    let result: Option<bool> = diesel::dsl::select(st_dwithin(
        st_point(0.0, 0.0).nullable(),
        st_point(1.0, 0.0).nullable(),
        2.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(true));

    // Points not within distance
    let result: Option<bool> = diesel::dsl::select(st_dwithin(
        st_point(0.0, 0.0).nullable(),
        st_point(10.0, 0.0).nullable(),
        2.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(false));
}

#[$test_attr]
fn diesel_select_st_dwithin_rejects_negative_distance() {
    let mut c = conn();
    let err = diesel::sql_query(
        "SELECT ST_DWithin(ST_Point(0,0), ST_Point(3,4), -1.0) AS val",
    )
    .get_result::<I32Result>(&mut c)
    .expect_err("negative distance should be rejected for ST_DWithin");
    let msg = format!("{err}");
    assert!(msg.contains("distance must be non-negative"), "got: {msg}");
}

#[$test_attr]
fn diesel_select_st_dwithinsphere() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let result: Option<bool> = diesel::dsl::select(st_dwithinsphere(
        st_point_srid(-0.1278, 51.5074, 4326).nullable(),
        st_point_srid(2.3522, 48.8566, 4326).nullable(),
        400_000.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(true));

    let result: Option<bool> = diesel::dsl::select(st_dwithinsphere(
        st_point_srid(-0.1278, 51.5074, 4326).nullable(),
        st_point_srid(2.3522, 48.8566, 4326).nullable(),
        300_000.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(false));
}

#[$test_attr]
fn diesel_select_st_dwithinspheroid() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let result: Option<bool> = diesel::dsl::select(st_dwithinspheroid(
        st_point_srid(-0.1278, 51.5074, 4326).nullable(),
        st_point_srid(2.3522, 48.8566, 4326).nullable(),
        400_000.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(true));

    let result: Option<bool> = diesel::dsl::select(st_dwithinspheroid(
        st_point_srid(-0.1278, 51.5074, 4326).nullable(),
        st_point_srid(2.3522, 48.8566, 4326).nullable(),
        300_000.0,
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result, Some(false));
}

#[$test_attr]
fn diesel_select_st_makeenvelope() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let wkt: Option<String> = diesel::dsl::select(
        st_astext(st_makeenvelope(0.0, 0.0, 10.0, 10.0).nullable()),
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = wkt.expect("should not be NULL");
    assert!(wkt.contains("POLYGON"), "got: {wkt}");
}

#[$test_attr]
fn diesel_select_st_geomfromtext() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let wkt: Option<String> = diesel::dsl::select(
        st_astext(st_geomfromtext("MULTIPOINT((0 0),(1 1),(2 2))")),
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = wkt.expect("should not be NULL");
    assert!(wkt.contains("MULTIPOINT"), "got: {wkt}");
}

#[$test_attr]
fn diesel_select_st_geomfromewkb_rejects_little_endian_zm_payload() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let ewkb_expr = diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
        "X'01010000C0000000000000F03F000000000000004000000000000008400000000000001040'",
    );

    let err = diesel::dsl::select(st_asewkb(st_geomfromewkb(ewkb_expr)))
        .get_result::<Option<Vec<u8>>>(&mut c)
        .expect_err("ZM payload must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("unsupported coordinate dimensions"), "got: {msg}");
}

#[$test_attr]
fn diesel_select_st_geomfromewkb_rejects_big_endian_zm_payload() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let ewkb_expr = diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
        "X'00C00000013FF0000000000000400000000000000040080000000000004010000000000000'",
    );

    let err = diesel::dsl::select(st_asewkb(st_geomfromewkb(ewkb_expr)))
        .get_result::<Option<Vec<u8>>>(&mut c)
        .expect_err("big-endian ZM payload must be rejected");
    let msg = format!("{err}");
    assert!(msg.contains("unsupported coordinate dimensions"), "got: {msg}");
}

#[$test_attr]
fn diesel_select_st_centroid() {
    use sqlitegis::diesel::functions::*;

    let mut c = conn();
    let wkt: Option<String> = diesel::dsl::select(
        st_astext(st_centroid(st_geomfromtext("POLYGON((0 0,4 0,4 4,0 4,0 0))"))),
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = wkt.expect("should not be NULL");
    assert!(wkt.contains("POINT"), "got: {wkt}");
    assert!(wkt.contains("2"), "centroid should be at (2,2), got: {wkt}");
}

// Method-style ORM queries

#[$test_attr]
fn method_st_astext_in_select() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES (1, 'Berlin', ST_Point(13.4050, 52.5200))",
    )
    .execute(&mut c)
    .unwrap();

    let wkt: Option<String> = features::table
        .select(features::geom.st_astext())
        .first(&mut c)
        .unwrap();
    let wkt = wkt.expect("should not be NULL");
    assert!(wkt.contains("POINT"), "got: {wkt}");
    assert!(wkt.contains("13.405"), "got: {wkt}");
}

#[$test_attr]
fn method_st_distance_in_select() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES (1, 'origin', ST_Point(0, 0))",
    )
    .execute(&mut c)
    .unwrap();

    let dist: Option<f64> = features::table
        .select(features::geom.st_distance(st_point(3.0, 4.0).nullable()))
        .first(&mut c)
        .unwrap();
    let dist = dist.expect("should not be NULL");
    assert!((dist - 5.0).abs() < 1e-10, "expected 5.0, got {dist}");
}

#[$test_attr]
fn method_st_dwithin_in_filter() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES
            (1, 'near', ST_Point(0.5, 0.5)),
            (2, 'far',  ST_Point(100, 100))",
    )
    .execute(&mut c)
    .unwrap();

    let names: Vec<String> = features::table
        .filter(features::geom.st_dwithin(st_point(0.0, 0.0).nullable(), 2.0).eq(true))
        .select(features::name)
        .load(&mut c)
        .unwrap();
    assert_eq!(names, vec!["near"]);
}

#[$test_attr]
fn method_st_intersects_in_filter() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES
            (1, 'inside',  ST_Point(5, 5)),
            (2, 'outside', ST_Point(50, 50))",
    )
    .execute(&mut c)
    .unwrap();

    let names: Vec<String> = features::table
        .filter(features::geom.st_intersects(
            st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"),
        ).eq(true))
        .select(features::name)
        .load(&mut c)
        .unwrap();
    assert_eq!(names, vec!["inside"]);
}

#[$test_attr]
fn method_st_contains_in_filter() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    // Insert a polygon and check which points it contains
    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES
            (1, 'box', ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))'))",
    )
    .execute(&mut c)
    .unwrap();

    let result: Option<i32> = features::table
        .filter(features::geom.st_contains(st_point(5.0, 5.0).nullable()).eq(true))
        .select(features::id)
        .first(&mut c)
        .optional()
        .unwrap();
    assert_eq!(result, Some(1));

    let result: Option<i32> = features::table
        .filter(features::geom.st_contains(st_point(50.0, 50.0).nullable()).eq(true))
        .select(features::id)
        .first(&mut c)
        .optional()
        .unwrap();
    assert_eq!(result, None);
}

#[$test_attr]
fn method_inside_area_in_filter() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES
            (1, 'interior', ST_Point(5, 5)),
            (2, 'boundary', ST_Point(0, 5)),
            (3, 'outside',  ST_Point(50, 50))",
    )
    .execute(&mut c)
    .unwrap();

    let names: Vec<String> = features::table
        .filter(features::geom.inside_area(
            st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"),
        ).eq(true))
        .select(features::name)
        .order(features::id.asc())
        .load(&mut c)
        .unwrap();
    assert_eq!(names, vec!["interior"]);
}

#[$test_attr]
fn method_outside_area_in_filter() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query(
        "INSERT INTO features (id, name, geom) VALUES
            (1, 'interior', ST_Point(5, 5)),
            (2, 'boundary', ST_Point(0, 5)),
            (3, 'outside',  ST_Point(50, 50))",
    )
    .execute(&mut c)
    .unwrap();

    let names: Vec<String> = features::table
        .filter(features::geom.outside_area(
            st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"),
        ).eq(true))
        .select(features::name)
        .order(features::id.asc())
        .load(&mut c)
        .unwrap();
    assert_eq!(names, vec!["outside"]);
}

#[$test_attr]
fn method_st_relate_match_geoms_in_select() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    let polygon = "POLYGON((0 0,0 3,3 3,3 0,0 0))";

    let matrix: Option<String> = diesel::dsl::select(st_relate(
        st_point(1.0, 1.0).nullable(),
        st_geomfromtext(polygon),
    ))
    .get_result(&mut c)
    .unwrap();
    let matrix = matrix.expect("st_relate should return a DE-9IM matrix");

    let inside: Option<bool> = diesel::dsl::select(st_point(1.0, 1.0).nullable().st_relate_match_geoms(
        st_geomfromtext(polygon),
        matrix.as_str(),
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(inside, Some(true));

    let impossible_pattern: Option<bool> = diesel::dsl::select(
        st_point(1.0, 1.0)
            .nullable()
            .st_relate_match_geoms(st_geomfromtext(polygon), "FFFFFFFFF"),
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(impossible_pattern, Some(false));
}

// Full ORM roundtrip

#[$test_attr]
fn orm_geometry_roundtrip() {
    use sqlitegis::diesel::types::Geometry as GeomType;

    let mut c = conn();
    setup_features_table(&mut c);

    let point = geo::Geometry::Point(geo::Point::new(13.4050, 52.5200));
    diesel::sql_query("INSERT INTO features (id, name, geom) VALUES (1, 'Berlin', ?)")
        .bind::<GeomType, _>(&point)
        .execute(&mut c)
        .unwrap();

    let row: Feature = features::table.find(1).first(&mut c).unwrap();
    assert_eq!(row.name, "Berlin");
    let geom = row.geom.expect("geom should not be NULL");
    match geom {
        geo::Geometry::Point(p) => {
            assert!((p.x() - 13.4050).abs() < 1e-10);
            assert!((p.y() - 52.5200).abs() < 1e-10);
        }
        other => panic!("expected Point, got {other:?}"),
    }
}

#[$test_attr]
fn orm_polygon_roundtrip() {
    use sqlitegis::diesel::types::Geometry as GeomType;
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    setup_features_table(&mut c);

    let polygon = geo::Geometry::Polygon(geo::Polygon::new(
        geo::LineString::from(vec![
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 10.0),
            (0.0, 10.0),
            (0.0, 0.0),
        ]),
        vec![],
    ));
    diesel::sql_query("INSERT INTO features (id, name, geom) VALUES (1, 'square', ?)")
        .bind::<GeomType, _>(&polygon)
        .execute(&mut c)
        .unwrap();

    // Verify area via query builder
    let area: Option<f64> = features::table
        .select(features::geom.st_area())
        .first(&mut c)
        .unwrap();
    let area = area.expect("should not be NULL");
    assert!((area - 100.0).abs() < 1e-10, "expected 100.0, got {area}");
}

#[$test_attr]
fn orm_null_geometry() {
    let mut c = conn();
    setup_features_table(&mut c);

    diesel::sql_query("INSERT INTO features (id, name, geom) VALUES (1, 'empty', NULL)")
        .execute(&mut c)
        .unwrap();

    let row: Feature = features::table.find(1).first(&mut c).unwrap();
    assert_eq!(row.name, "empty");
    assert!(row.geom.is_none());
}

// Spatial index correctness

#[$test_attr]
fn spatial_index_correctness() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();

    diesel::sql_query(
        "CREATE TABLE grid (
            id   INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            geom BLOB
        )",
    )
    .execute(&mut c)
    .unwrap();

    // Insert a grid of points
    for i in 0..10 {
        for j in 0..10 {
            let id = i * 10 + j;
            diesel::sql_query(format!(
                "INSERT INTO grid (id, name, geom) VALUES ({id}, 'p{id}', ST_Point({i}, {j}))"
            ))
            .execute(&mut c)
            .unwrap();
        }
    }

    // Create spatial index
    diesel::sql_query("SELECT CreateSpatialIndex('grid', 'geom')")
        .execute(&mut c)
        .unwrap();

    // Query using ORM R-tree join: find points inside envelope (2,2)-(5,5)
    let ids: Vec<i32> = grid::table
        .inner_join(grid_geom_rtree::table.on(grid::id.eq(grid_geom_rtree::id)))
        .filter(grid_geom_rtree::xmin.ge(2.0))
        .filter(grid_geom_rtree::xmax.le(5.0))
        .filter(grid_geom_rtree::ymin.ge(2.0))
        .filter(grid_geom_rtree::ymax.le(5.0))
        .filter(
            grid::geom
                .st_intersects(st_makeenvelope(2.0, 2.0, 5.0, 5.0).nullable())
                .eq(true),
        )
        .select(grid::id)
        .order(grid::id.asc())
        .load(&mut c)
        .unwrap();

    // Points at (2..=5, 2..=5) should be found
    let mut expected = Vec::new();
    for i in 2..=5 {
        for j in 2..=5 {
            expected.push(i * 10 + j);
        }
    }
    expected.sort();
    assert_eq!(ids, expected, "spatial index query returned wrong points");
}

// Alias SQL functions
// Verify that every name alias registered in the SQLite extension is also
// reachable through a Diesel connection (raw SQL path).

#[$test_attr]
fn st_makepoint_alias_works_via_sql() {
    let mut c = conn();
    let result: TextResult =
        diesel::sql_query("SELECT ST_AsText(ST_MakePoint(3.0, 4.0)) AS val")
            .get_result(&mut c)
            .unwrap();
    let wkt = result.val.unwrap();
    assert!(wkt.contains("POINT"), "ST_MakePoint WKT = {wkt}");
}

#[$test_attr]
fn geometry_type_alias_works_via_sql() {
    let mut c = conn();
    let result: TextResult =
        diesel::sql_query("SELECT GeometryType(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')) AS val")
            .get_result(&mut c)
            .unwrap();
    assert_eq!(result.val.unwrap(), "ST_Polygon");
}

#[$test_attr]
fn st_numinteriorring_alias_works_via_sql() {
    let mut c = conn();
    let result: I32Result = diesel::sql_query(
        "SELECT ST_NumInteriorRing(\
            ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))')\
         ) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(result.val, Some(1));
}

#[$test_attr]
fn st_length2d_alias_works_via_sql() {
    let mut c = conn();
    let result: F64Result =
        diesel::sql_query("SELECT ST_Length2D(ST_GeomFromText('LINESTRING(0 0,3 4)')) AS val")
            .get_result(&mut c)
            .unwrap();
    let len = result.val.unwrap();
    assert!((len - 5.0).abs() < 1e-10, "ST_Length2D = {len}");
}

#[$test_attr]
fn st_perimeter2d_alias_works_via_sql() {
    let mut c = conn();
    let result: F64Result = diesel::sql_query(
        "SELECT ST_Perimeter2D(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))')) AS val",
    )
    .get_result(&mut c)
    .unwrap();
    let perim = result.val.unwrap();
    assert!((perim - 4.0).abs() < 1e-10, "ST_Perimeter2D = {perim}");
}

// Typed Diesel alias functions
// These use the declared Diesel SQL functions (not raw SQL), verifying that
// the type-system wrappers are wired to the correct SQL names.

#[$test_attr]
fn st_makepoint_typed_diesel_function() {
    use sqlitegis::diesel::functions::st_makepoint;
    let mut c = conn();
    let wkt: Option<String> = diesel::dsl::select(
        sqlitegis::diesel::functions::st_astext(st_makepoint(1.5_f64, 2.5_f64).nullable()),
    )
    .get_result(&mut c)
    .unwrap();
    let wkt = wkt.unwrap();
    assert!(wkt.contains("POINT") && wkt.contains("1.5"), "got: {wkt}");
}

#[$test_attr]
fn geometry_type_typed_diesel_function() {
    use sqlitegis::diesel::functions::{geometry_type, st_geomfromtext};
    let mut c = conn();
    let val: Option<String> = diesel::dsl::select(geometry_type(
        st_geomfromtext("POLYGON((0 0,1 0,1 1,0 1,0 0))"),
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(val.unwrap(), "ST_Polygon");
}

#[$test_attr]
fn st_numinteriorring_typed_diesel_function() {
    use sqlitegis::diesel::functions::{st_geomfromtext, st_numinteriorring};
    let mut c = conn();
    let val: Option<i32> = diesel::dsl::select(st_numinteriorring(
        st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))"),
    ))
    .get_result(&mut c)
    .unwrap();
    assert_eq!(val, Some(1));
}

#[$test_attr]
fn st_length2d_typed_diesel_function() {
    use sqlitegis::diesel::functions::{st_geomfromtext, st_length2d};
    let mut c = conn();
    let len: Option<f64> =
        diesel::dsl::select(st_length2d(st_geomfromtext("LINESTRING(0 0,3 4)")))
            .get_result(&mut c)
            .unwrap();
    let len = len.unwrap();
    assert!((len - 5.0).abs() < 1e-10, "ST_Length2D = {len}");
}

#[$test_attr]
fn st_perimeter2d_typed_diesel_function() {
    use sqlitegis::diesel::functions::{st_geomfromtext, st_perimeter2d};
    let mut c = conn();
    let perim: Option<f64> = diesel::dsl::select(st_perimeter2d(st_geomfromtext(
        "POLYGON((0 0,1 0,1 1,0 1,0 0))",
    )))
    .get_result(&mut c)
    .unwrap();
    let perim = perim.unwrap();
    assert!((perim - 4.0).abs() < 1e-10, "ST_Perimeter2D = {perim}");
}

// Index-aware query pattern tests
// Indexed and non-indexed paths both use the Diesel query builder.

#[$test_attr]
fn indexed_intersects_window() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE iw_grid (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c).unwrap();

    for x in 0..10 {
        for y in 0..10 {
            let id = x * 10 + y;
            diesel::sql_query(format!(
                "INSERT INTO iw_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            )).execute(&mut c).unwrap();
        }
    }
    diesel::sql_query("SELECT CreateSpatialIndex('iw_grid', 'geom')")
        .execute(&mut c).unwrap();

    let idx_ids: Vec<i32> = iw_grid::table
        .inner_join(iw_grid_geom_rtree::table.on(iw_grid::id.eq(iw_grid_geom_rtree::id)))
        .filter(iw_grid_geom_rtree::xmax.ge(2.0))
        .filter(iw_grid_geom_rtree::xmin.le(5.0))
        .filter(iw_grid_geom_rtree::ymax.ge(2.0))
        .filter(iw_grid_geom_rtree::ymin.le(5.0))
        .filter(
            iw_grid::geom
                .st_intersects(st_makeenvelope(2.0, 2.0, 5.0, 5.0).nullable())
                .eq(true),
        )
        .select(iw_grid::id)
        .order(iw_grid::id.asc())
        .load(&mut c)
        .unwrap();

    // Non-indexed: Diesel ORM
    let non_ids: Vec<i32> = iw_grid::table
        .filter(
            iw_grid::geom
                .st_intersects(st_makeenvelope(2.0, 2.0, 5.0, 5.0).nullable())
                .eq(true),
        )
        .select(iw_grid::id)
        .order(iw_grid::id.asc())
        .load(&mut c)
        .unwrap();

    assert_eq!(idx_ids, non_ids, "indexed and non-indexed must match");
    assert_eq!(idx_ids.len(), 16); // (2..=5, 2..=5) = 4x4
}

#[$test_attr]
fn indexed_inside_polygon() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE ip_pts (id INTEGER PRIMARY KEY, name TEXT NOT NULL, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query(
        "INSERT INTO ip_pts (id, name, geom) VALUES \
            (1, 'interior', ST_Point(5, 5)), \
            (2, 'edge',     ST_Point(0, 5)), \
            (3, 'outside',  ST_Point(50, 50))",
    ).execute(&mut c).unwrap();

    diesel::sql_query("SELECT CreateSpatialIndex('ip_pts', 'geom')")
        .execute(&mut c).unwrap();

    let idx_ids: Vec<i32> = ip_pts::table
        .inner_join(ip_pts_geom_rtree::table.on(ip_pts::id.eq(ip_pts_geom_rtree::id)))
        .filter(ip_pts_geom_rtree::xmax.ge(0.0))
        .filter(ip_pts_geom_rtree::xmin.le(10.0))
        .filter(ip_pts_geom_rtree::ymax.ge(0.0))
        .filter(ip_pts_geom_rtree::ymin.le(10.0))
        .filter(
            ip_pts::geom
                .st_within(st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"))
                .eq(true),
        )
        .select(ip_pts::id)
        .order(ip_pts::id.asc())
        .load(&mut c)
        .unwrap();

    // Non-indexed: Diesel ORM
    let non_ids: Vec<i32> = ip_pts::table
        .filter(
            ip_pts::geom
                .st_within(st_geomfromtext("POLYGON((0 0,10 0,10 10,0 10,0 0))"))
                .eq(true),
        )
        .select(ip_pts::id)
        .order(ip_pts::id.asc())
        .load(&mut c)
        .unwrap();

    assert_eq!(idx_ids, non_ids);
    // Only interior point (5,5) is strictly within. Boundary (0,5) is not
    assert_eq!(idx_ids, vec![1]);
}

#[$test_attr]
fn indexed_contains_point() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE ic_polys (id INTEGER PRIMARY KEY, name TEXT NOT NULL, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query(
        "INSERT INTO ic_polys (id, name, geom) VALUES \
            (1, 'small', ST_GeomFromText('POLYGON((0 0,5 0,5 5,0 5,0 0))')), \
            (2, 'big',   ST_GeomFromText('POLYGON((0 0,20 0,20 20,0 20,0 0))')), \
            (3, 'far',   ST_GeomFromText('POLYGON((50 50,60 50,60 60,50 60,50 50))'))",
    ).execute(&mut c).unwrap();

    diesel::sql_query("SELECT CreateSpatialIndex('ic_polys', 'geom')")
        .execute(&mut c).unwrap();

    let idx_ids: Vec<i32> = ic_polys::table
        .inner_join(ic_polys_geom_rtree::table.on(ic_polys::id.eq(ic_polys_geom_rtree::id)))
        .filter(ic_polys_geom_rtree::xmin.le(3.0))
        .filter(ic_polys_geom_rtree::xmax.ge(3.0))
        .filter(ic_polys_geom_rtree::ymin.le(3.0))
        .filter(ic_polys_geom_rtree::ymax.ge(3.0))
        .filter(ic_polys::geom.st_contains(st_point(3.0, 3.0).nullable()).eq(true))
        .select(ic_polys::id)
        .order(ic_polys::id.asc())
        .load(&mut c)
        .unwrap();

    // Non-indexed: Diesel ORM
    let non_ids: Vec<i32> = ic_polys::table
        .filter(ic_polys::geom.st_contains(st_point(3.0, 3.0).nullable()).eq(true))
        .select(ic_polys::id)
        .order(ic_polys::id.asc())
        .load(&mut c)
        .unwrap();

    assert_eq!(idx_ids, non_ids);
    // Point (3,3) inside both 'small' and 'big', not 'far'
    assert_eq!(idx_ids, vec![1, 2]);
}

#[$test_attr]
fn indexed_geodesic_radius() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE igr_cities (id INTEGER PRIMARY KEY, name TEXT NOT NULL, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query(
        "INSERT INTO igr_cities (id, name, geom) VALUES \
            (1, 'London', ST_Point(-0.1278, 51.5074, 4326)), \
            (2, 'Paris',  ST_Point(2.3522, 48.8566, 4326)), \
            (3, 'Berlin', ST_Point(13.4050, 52.5200, 4326)), \
            (4, 'Tokyo',  ST_Point(139.6917, 35.6895, 4326))",
    ).execute(&mut c).unwrap();

    diesel::sql_query("SELECT CreateSpatialIndex('igr_cities', 'geom')")
        .execute(&mut c).unwrap();

    let lon: f64 = -0.1278;
    let lat: f64 = 51.5074;
    let radius_m: f64 = 400_000.0;
    let dlat = radius_m / 111_320.0;
    let dlon = radius_m / (111_320.0 * lat.to_radians().cos());

    let idx_ids: Vec<i32> = igr_cities::table
        .inner_join(igr_cities_geom_rtree::table.on(igr_cities::id.eq(igr_cities_geom_rtree::id)))
        .filter(igr_cities_geom_rtree::xmax.ge(lon - dlon))
        .filter(igr_cities_geom_rtree::xmin.le(lon + dlon))
        .filter(igr_cities_geom_rtree::ymax.ge(lat - dlat))
        .filter(igr_cities_geom_rtree::ymin.le(lat + dlat))
        .filter(
            igr_cities::geom
                .st_dwithinsphere(st_point_srid(lon, lat, 4326).nullable(), radius_m)
                .eq(true),
        )
        .select(igr_cities::id)
        .order(igr_cities::id.asc())
        .load(&mut c)
        .unwrap();

    // Non-indexed: Diesel ORM
    let non_ids: Vec<i32> = igr_cities::table
        .filter(igr_cities::geom.st_dwithinsphere(
            st_point_srid(lon, lat, 4326).nullable(),
            radius_m,
        ).eq(true))
        .select(igr_cities::id)
        .order(igr_cities::id.asc())
        .load(&mut c)
        .unwrap();

    assert_eq!(idx_ids, non_ids);
    // London->Paris approximately  344 km (within), London->Berlin approximately  930 km (outside)
    assert_eq!(idx_ids, vec![1, 2]);
}

#[$test_attr]
fn knn_nearest_n_planar() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE knn_grid (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c).unwrap();

    for x in 0..10 {
        for y in 0..10 {
            let id = x * 10 + y;
            diesel::sql_query(format!(
                "INSERT INTO knn_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            )).execute(&mut c).unwrap();
        }
    }
    diesel::sql_query("SELECT CreateSpatialIndex('knn_grid', 'geom')")
        .execute(&mut c).unwrap();

    let indexed_dists: Vec<Option<f64>> = knn_grid::table
        .inner_join(knn_grid_geom_rtree::table.on(knn_grid::id.eq(knn_grid_geom_rtree::id)))
        .filter(knn_grid_geom_rtree::xmax.ge(1.5))
        .filter(knn_grid_geom_rtree::xmin.le(7.5))
        .filter(knn_grid_geom_rtree::ymax.ge(1.5))
        .filter(knn_grid_geom_rtree::ymin.le(7.5))
        .select(knn_grid::geom.st_distance(st_point(4.5, 4.5).nullable()))
        .order(knn_grid::geom.st_distance(st_point(4.5, 4.5).nullable()))
        .limit(5)
        .load(&mut c)
        .unwrap();

    assert_eq!(indexed_dists.len(), 5);
    let dists: Vec<f64> = indexed_dists
        .iter()
        .map(|d| d.expect("distance should not be NULL"))
        .collect();
    let sqrt_half = (0.5_f64).sqrt();
    for d in &dists[..4] {
        assert!((*d - sqrt_half).abs() < 1e-10, "expected {sqrt_half}, got {d}");
    }
    assert!(dists[4] > sqrt_half + 0.1);

    // Non-indexed: Diesel ORM
    let non_dists: Vec<Option<f64>> = knn_grid::table
        .select(knn_grid::geom.st_distance(st_point(4.5, 4.5).nullable()))
        .order(knn_grid::geom.st_distance(st_point(4.5, 4.5).nullable()))
        .limit(5)
        .load(&mut c)
        .unwrap();

    assert_eq!(dists.len(), non_dists.len());
    for (a, b) in dists.iter().zip(non_dists.iter()) {
        let b = b.expect("distance should not be NULL");
        assert!((a - b).abs() < 1e-10, "indexed={a} vs non-indexed={b}");
    }
}

#[$test_attr]
fn knn_nearest_n_geodesic() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE knn_cities (id INTEGER PRIMARY KEY, name TEXT NOT NULL, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query(
        "INSERT INTO knn_cities (id, name, geom) VALUES \
            (1, 'London', ST_Point(-0.1278, 51.5074, 4326)), \
            (2, 'Paris',  ST_Point(2.3522, 48.8566, 4326)), \
            (3, 'Berlin', ST_Point(13.4050, 52.5200, 4326)), \
            (4, 'Madrid', ST_Point(-3.7038, 40.4168, 4326)), \
            (5, 'Tokyo',  ST_Point(139.6917, 35.6895, 4326))",
    ).execute(&mut c).unwrap();

    diesel::sql_query("SELECT CreateSpatialIndex('knn_cities', 'geom')")
        .execute(&mut c).unwrap();

    let lon: f64 = 2.3522;
    let lat: f64 = 48.8566;
    let search_radius_m: f64 = 2_000_000.0;
    let dlat = search_radius_m / 111_320.0;
    let dlon = search_radius_m / (111_320.0 * lat.to_radians().cos());

    let ids: Vec<i32> = knn_cities::table
        .inner_join(knn_cities_geom_rtree::table.on(knn_cities::id.eq(knn_cities_geom_rtree::id)))
        .filter(knn_cities_geom_rtree::xmax.ge(lon - dlon))
        .filter(knn_cities_geom_rtree::xmin.le(lon + dlon))
        .filter(knn_cities_geom_rtree::ymax.ge(lat - dlat))
        .filter(knn_cities_geom_rtree::ymin.le(lat + dlat))
        .order(knn_cities::geom.st_distancesphere(st_point_srid(lon, lat, 4326).nullable()))
        .select(knn_cities::id)
        .limit(3)
        .load(&mut c)
        .unwrap();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], 2, "Paris should be nearest to itself");
    assert_eq!(ids[1], 1, "London should be second");
    assert_eq!(ids[2], 3, "Berlin should be third");

    // Non-indexed: Diesel ORM
    let non_ids: Vec<i32> = knn_cities::table
        .order(knn_cities::geom.st_distancesphere(st_point_srid(lon, lat, 4326).nullable()))
        .select(knn_cities::id)
        .limit(3)
        .load(&mut c)
        .unwrap();

    assert_eq!(ids, non_ids);
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
fn indexed_intersects_window_is_faster() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE sw_grid (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query("BEGIN").execute(&mut c).unwrap();
    for x in 0..100 {
        for y in 0..100 {
            let id = x * 100 + y;
            diesel::sql_query(format!(
                "INSERT INTO sw_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            )).execute(&mut c).unwrap();
        }
    }
    diesel::sql_query("COMMIT").execute(&mut c).unwrap();
    diesel::sql_query("SELECT CreateSpatialIndex('sw_grid', 'geom')")
        .execute(&mut c).unwrap();

    let run_indexed = |conn: &mut SqliteConnection| -> Vec<i32> {
        sw_grid::table
            .inner_join(sw_grid_geom_rtree::table.on(sw_grid::id.eq(sw_grid_geom_rtree::id)))
            .filter(sw_grid_geom_rtree::xmax.ge(10.0))
            .filter(sw_grid_geom_rtree::xmin.le(20.0))
            .filter(sw_grid_geom_rtree::ymax.ge(10.0))
            .filter(sw_grid_geom_rtree::ymin.le(20.0))
            .filter(
                sw_grid::geom
                    .st_intersects(st_makeenvelope(10.0, 10.0, 20.0, 20.0).nullable())
                    .eq(true),
            )
            .select(sw_grid::id)
            .load(conn)
            .unwrap()
    };

    let run_full = |conn: &mut SqliteConnection| -> Vec<i32> {
        sw_grid::table
            .filter(
                sw_grid::geom
                    .st_intersects(st_makeenvelope(10.0, 10.0, 20.0, 20.0).nullable())
                    .eq(true),
            )
            .select(sw_grid::id)
            .load(conn)
            .unwrap()
    };

    // Warmup
    let _: Vec<i32> = run_indexed(&mut c);
    let _: Vec<i32> = run_full(&mut c);

    let n = 20;
    let mut indexed_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _: Vec<i32> = run_indexed(&mut c);
        indexed_best = indexed_best.min(elapsed_since_utc(t));
    }

    let mut full_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _: Vec<i32> = run_full(&mut c);
        full_best = full_best.min(elapsed_since_utc(t));
    }

    // Both return 121 rows (11x11 points in [10,20])
    let idx: Vec<i32> = run_indexed(&mut c);
    let full: Vec<i32> = run_full(&mut c);
    assert_eq!(idx.len(), full.len());
    assert_eq!(idx.len(), 121);

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
fn indexed_knn_is_faster() {
    use sqlitegis::diesel::prelude::*;

    let mut c = conn();
    diesel::sql_query("CREATE TABLE sk_grid (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c).unwrap();

    diesel::sql_query("BEGIN").execute(&mut c).unwrap();
    for x in 0..100 {
        for y in 0..100 {
            let id = x * 100 + y;
            diesel::sql_query(format!(
                "INSERT INTO sk_grid (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            )).execute(&mut c).unwrap();
        }
    }
    diesel::sql_query("COMMIT").execute(&mut c).unwrap();
    diesel::sql_query("SELECT CreateSpatialIndex('sk_grid', 'geom')")
        .execute(&mut c).unwrap();

    let run_indexed = |conn: &mut SqliteConnection| -> Vec<i32> {
        sk_grid::table
            .inner_join(sk_grid_geom_rtree::table.on(sk_grid::id.eq(sk_grid_geom_rtree::id)))
            .filter(sk_grid_geom_rtree::xmax.ge(45.0))
            .filter(sk_grid_geom_rtree::xmin.le(55.0))
            .filter(sk_grid_geom_rtree::ymax.ge(45.0))
            .filter(sk_grid_geom_rtree::ymin.le(55.0))
            .order(sk_grid::geom.st_distance(st_point(50.0, 50.0).nullable()))
            .select(sk_grid::id)
            .limit(5)
            .load(conn)
            .unwrap()
    };

    let run_full = |conn: &mut SqliteConnection| -> Vec<i32> {
        sk_grid::table
            .order(sk_grid::geom.st_distance(st_point(50.0, 50.0).nullable()))
            .select(sk_grid::id)
            .limit(5)
            .load(conn)
            .unwrap()
    };

    // Warmup
    let _: Vec<i32> = run_indexed(&mut c);
    let _: Vec<i32> = run_full(&mut c);

    let n = 20;
    let mut indexed_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _: Vec<i32> = run_indexed(&mut c);
        indexed_best = indexed_best.min(elapsed_since_utc(t));
    }

    let mut full_best = std::time::Duration::MAX;
    for _ in 0..n {
        let t = chrono::Utc::now();
        let _: Vec<i32> = run_full(&mut c);
        full_best = full_best.min(elapsed_since_utc(t));
    }

    let idx: Vec<i32> = run_indexed(&mut c);
    let full: Vec<i32> = run_full(&mut c);
    assert_eq!(idx.len(), full.len());

    eprintln!("knn 10K: indexed={indexed_best:?}  full_scan={full_best:?}  speedup={:.1}x", full_best.as_nanos() as f64 / indexed_best.as_nanos() as f64);
    assert!(
        indexed_best < full_best,
        "indexed KNN ({indexed_best:?}) should be faster than full scan ({full_best:?}) \
         over 10K rows"
    );
}

    }; // end macro
}
