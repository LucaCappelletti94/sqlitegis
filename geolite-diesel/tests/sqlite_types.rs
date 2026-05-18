#![cfg(feature = "sqlite")]
#![allow(dead_code)]

use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::{Integer, Nullable};
use geolite_diesel::types::{Geography, Geometry};

// -- Helper to create an in-memory connection ---------------------------------

fn conn() -> SqliteConnection {
    let mut c = SqliteConnection::establish(":memory:").unwrap();
    diesel::sql_query("CREATE TABLE t (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c)
        .unwrap();
    c
}

// -- QueryableByName row types ------------------------------------------------

#[derive(QueryableByName, Debug)]
struct GeomRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Geometry>)]
    geom: Option<Vec<u8>>,
}

#[derive(QueryableByName, Debug)]
struct GeogRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Geography>)]
    geom: Option<Vec<u8>>,
}

#[derive(QueryableByName, Debug)]
struct GeoGeomRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Geometry>)]
    geom: Option<geo::Geometry<f64>>,
}

#[derive(QueryableByName, Debug)]
struct GeoGeogRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Nullable<Geography>)]
    geom: Option<geo::Geometry<f64>>,
}

// -- Vec<u8> roundtrips -------------------------------------------------------

#[test]
fn vec_u8_roundtrip_geometry() {
    let mut c = conn();

    // Build EWKB for POINT(1 2) with no SRID via geolite-core
    let ewkb =
        geolite_core::ewkb::write_ewkb(&geo::Geometry::Point(geo::Point::new(1.0, 2.0)), None)
            .unwrap();

    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&ewkb)
        .execute(&mut c)
        .unwrap();

    let row: GeomRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    assert_eq!(row.id, 1);
    let blob = row.geom.expect("geom should not be NULL");
    assert_eq!(blob, ewkb);
}

#[test]
fn vec_u8_roundtrip_geography() {
    let mut c = conn();

    let ewkb = geolite_core::ewkb::write_ewkb(
        &geo::Geometry::Point(geo::Point::new(1.0, 2.0)),
        Some(4326),
    )
    .unwrap();

    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geography, _>(&ewkb)
        .execute(&mut c)
        .unwrap();

    let row: GeogRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    let blob = row.geom.expect("geom should not be NULL");
    assert_eq!(blob, ewkb);
}

// -- geo::Geometry roundtrips -------------------------------------------------

#[test]
fn geo_geometry_roundtrip() {
    let mut c = conn();

    let point = geo::Geometry::Point(geo::Point::new(3.5, 7.25));

    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&point)
        .execute(&mut c)
        .unwrap();

    let row: GeoGeomRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    let geom = row.geom.expect("geom should not be NULL");
    match geom {
        geo::Geometry::Point(p) => {
            assert!((p.x() - 3.5).abs() < 1e-10);
            assert!((p.y() - 7.25).abs() < 1e-10);
        }
        other => panic!("expected Point, got {other:?}"),
    }
}

#[test]
fn geo_geography_roundtrip_has_srid() {
    let mut c = conn();

    let point = geo::Geometry::Point(geo::Point::new(13.4, 52.5));

    // Geography ToSql writes SRID=4326
    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geography, _>(&point)
        .execute(&mut c)
        .unwrap();

    // Read back as raw bytes and verify SRID is present
    let row: GeogRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    let blob = row.geom.expect("geom should not be NULL");
    let (_geom, srid) = geolite_core::ewkb::parse_ewkb(&blob).unwrap();
    assert_eq!(srid, Some(4326));
}

#[test]
fn geography_fromsql_rejects_missing_srid() {
    let mut c = conn();
    let ewkb =
        geolite_core::ewkb::write_ewkb(&geo::Geometry::Point(geo::Point::new(13.4, 52.5)), None)
            .unwrap();

    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&ewkb)
        .execute(&mut c)
        .unwrap();

    let err = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result::<GeoGeogRow>(&mut c)
        .expect_err("geography deserialization should reject missing SRID");
    let msg = err.to_string();
    assert!(
        msg.contains("must include SRID 4326"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn geography_fromsql_rejects_non_4326_srid() {
    let mut c = conn();
    let ewkb = geolite_core::ewkb::write_ewkb(
        &geo::Geometry::Point(geo::Point::new(13.4, 52.5)),
        Some(3857),
    )
    .unwrap();

    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&ewkb)
        .execute(&mut c)
        .unwrap();

    let err = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result::<GeoGeogRow>(&mut c)
        .expect_err("geography deserialization should reject non-4326 SRID");
    let msg = err.to_string();
    assert!(
        msg.contains("must use SRID 4326"),
        "unexpected error message: {msg}"
    );
}

#[test]
fn geometry_tosql_no_srid() {
    let mut c = conn();

    let point = geo::Geometry::Point(geo::Point::new(1.0, 2.0));

    // Geometry ToSql writes no SRID
    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&point)
        .execute(&mut c)
        .unwrap();

    let row: GeomRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    let blob = row.geom.expect("geom should not be NULL");
    let (_geom, srid) = geolite_core::ewkb::parse_ewkb(&blob).unwrap();
    assert_eq!(srid, None);
}

// -- [u8] slice ToSql ---------------------------------------------------------

#[test]
fn slice_tosql() {
    let mut c = conn();

    let ewkb =
        geolite_core::ewkb::write_ewkb(&geo::Geometry::Point(geo::Point::new(9.0, 10.0)), None)
            .unwrap();

    // Use the [u8] ToSql impl by passing a slice
    sql_query("INSERT INTO t (id, geom) VALUES (1, ?)")
        .bind::<Geometry, _>(&ewkb[..])
        .execute(&mut c)
        .unwrap();

    let row: GeomRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    let blob = row.geom.expect("geom should not be NULL");
    assert_eq!(blob, ewkb);
}

// -- NULL handling ------------------------------------------------------------

#[test]
fn null_handling() {
    let mut c = conn();

    sql_query("INSERT INTO t (id, geom) VALUES (1, NULL)")
        .execute(&mut c)
        .unwrap();

    let row: GeomRow = sql_query("SELECT id, geom FROM t WHERE id = 1")
        .get_result(&mut c)
        .unwrap();

    assert_eq!(row.id, 1);
    assert!(row.geom.is_none());
}

// -- debug_query: exercise all define_sql_function! declarations --------------
//
// Each call builds a diesel expression and serializes it to SQL via
// debug_query, which exercises the code generated by define_sql_function!
// (struct construction, QueryFragment::walk_ast, Expression type info).

macro_rules! assert_sql_contains {
    ($q:expr, $needle:expr) => {{
        let sql = diesel::debug_query::<diesel::sqlite::Sqlite, _>(&$q).to_string();
        assert!(
            sql.to_lowercase().contains($needle),
            "expected {:?} in: {sql}",
            $needle
        );
    }};
}

/// Helper: fresh `Nullable<Geometry>` SQL literal for each use (not Clone).
macro_rules! g {
    () => {
        diesel::dsl::sql::<Nullable<Geometry>>("x")
    };
}

macro_rules! t {
    () => {
        diesel::dsl::sql::<diesel::sql_types::Text>("'POINT(0 0)'")
    };
}

macro_rules! d {
    () => {
        diesel::dsl::sql::<diesel::sql_types::Double>("1.0")
    };
}

macro_rules! i {
    () => {
        diesel::dsl::sql::<Integer>("1")
    };
}

// -- I/O functions ------------------------------------------------------------

#[test]
fn debug_query_st_geomfromtext() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromtext(t!())),
        "st_geomfromtext"
    );
}

#[test]
fn debug_query_st_geomfromtext_srid() {
    use geolite_diesel::functions::*;
    // sql_name = "ST_GeomFromText" -- the Rust name differs but SQL name is the same
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromtext_srid(t!(), i!())),
        "st_geomfromtext"
    );
}

#[test]
fn debug_query_st_astext() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_astext(g!())), "st_astext");
}

#[test]
fn debug_query_st_asewkt() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_asewkt(g!())), "st_asewkt");
}

#[test]
fn debug_query_st_asbinary() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_asbinary(g!())), "st_asbinary");
}

#[test]
fn debug_query_st_asewkb() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_asewkb(g!())), "st_asewkb");
}

#[test]
fn debug_query_st_geomfromwkb() {
    use geolite_diesel::functions::*;
    let wkb = diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>("x");
    assert_sql_contains!(diesel::dsl::select(st_geomfromwkb(wkb)), "st_geomfromwkb");
}

#[test]
fn debug_query_st_geomfromwkb_srid() {
    use geolite_diesel::functions::*;
    let wkb = diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>("x");
    // sql_name = "ST_GeomFromWKB" -- Rust alias maps to the same SQL function.
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromwkb_srid(wkb, i!())),
        "st_geomfromwkb"
    );
}

#[test]
fn debug_query_st_geomfromwkb_accepts_st_asbinary_output() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromwkb(st_asbinary(g!()))),
        "st_geomfromwkb"
    );
}

#[test]
fn debug_query_st_geomfromewkb() {
    use geolite_diesel::functions::*;
    let ewkb = diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>("x");
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromewkb(ewkb)),
        "st_geomfromewkb"
    );
}

#[test]
fn debug_query_st_geomfromewkb_accepts_st_asewkb_output() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromewkb(st_asewkb(g!()))),
        "st_geomfromewkb"
    );
}

#[test]
fn debug_query_st_asgeojson() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_asgeojson(g!())), "st_asgeojson");
}

#[test]
fn debug_query_st_geomfromgeojson() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geomfromgeojson(t!())),
        "st_geomfromgeojson"
    );
}

// -- Constructor functions ----------------------------------------------------

#[test]
fn debug_query_st_point() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_point(d!(), d!())), "st_point");
}

#[test]
fn debug_query_st_point_srid() {
    use geolite_diesel::functions::*;
    // sql_name = "ST_Point" -- Rust alias maps to the same SQL function.
    assert_sql_contains!(
        diesel::dsl::select(st_point_srid(d!(), d!(), i!())),
        "st_point"
    );
}

#[test]
fn debug_query_st_makeenvelope() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_makeenvelope(d!(), d!(), d!(), d!())),
        "st_makeenvelope"
    );
}

#[test]
fn debug_query_st_makeenvelope_srid() {
    use geolite_diesel::functions::*;
    // sql_name = "ST_MakeEnvelope" -- Rust alias maps to the same SQL function.
    assert_sql_contains!(
        diesel::dsl::select(st_makeenvelope_srid(d!(), d!(), d!(), d!(), i!())),
        "st_makeenvelope"
    );
}

#[test]
fn debug_query_st_tileenvelope() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_tileenvelope(i!(), i!(), i!())),
        "st_tileenvelope"
    );
}

#[test]
fn debug_query_st_makeline() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_makeline(g!(), g!())), "st_makeline");
}

#[test]
fn debug_query_st_makepolygon() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_makepolygon(g!())), "st_makepolygon");
}

#[test]
fn debug_query_st_collect() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_collect(g!(), g!())), "st_collect");
}

// -- Accessor functions -------------------------------------------------------

#[test]
fn debug_query_st_srid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_srid(g!())), "st_srid");
}

#[test]
fn debug_query_st_setsrid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_setsrid(g!(), i!())), "st_setsrid");
}

#[test]
fn debug_query_st_geometrytype() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geometrytype(g!())),
        "st_geometrytype"
    );
}

#[test]
fn debug_query_st_x() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_x(g!())), "st_x");
}

#[test]
fn debug_query_st_y() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_y(g!())), "st_y");
}

#[test]
fn debug_query_st_z() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_z(g!())), "st_z");
}

#[test]
fn debug_query_st_isempty() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_isempty(g!())), "st_isempty");
}

#[test]
fn debug_query_st_ndims() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_ndims(g!())), "st_ndims");
}

#[test]
fn debug_query_st_coorddim() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_coorddim(g!())), "st_coorddim");
}

#[test]
fn debug_query_st_zmflag() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_zmflag(g!())), "st_zmflag");
}

#[test]
fn debug_query_st_memsize() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_memsize(g!())), "st_memsize");
}

#[test]
fn debug_query_st_isvalid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_isvalid(g!())), "st_isvalid");
}

#[test]
fn debug_query_st_isvalidreason() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_isvalidreason(g!())),
        "st_isvalidreason"
    );
}

#[test]
fn debug_query_st_numpoints() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_numpoints(g!())), "st_numpoints");
}

#[test]
fn debug_query_st_npoints() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_npoints(g!())), "st_npoints");
}

#[test]
fn debug_query_st_numgeometries() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_numgeometries(g!())),
        "st_numgeometries"
    );
}

#[test]
fn debug_query_st_numinteriorrings() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_numinteriorrings(g!())),
        "st_numinteriorrings"
    );
}

#[test]
fn debug_query_st_numrings() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_numrings(g!())), "st_numrings");
}

#[test]
fn debug_query_st_dimension() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_dimension(g!())), "st_dimension");
}

#[test]
fn debug_query_st_envelope() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_envelope(g!())), "st_envelope");
}

#[test]
fn debug_query_st_pointn() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_pointn(g!(), i!())), "st_pointn");
}

#[test]
fn debug_query_st_startpoint() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_startpoint(g!())), "st_startpoint");
}

#[test]
fn debug_query_st_endpoint() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_endpoint(g!())), "st_endpoint");
}

#[test]
fn debug_query_st_exteriorring() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_exteriorring(g!())),
        "st_exteriorring"
    );
}

#[test]
fn debug_query_st_interiorringn() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_interiorringn(g!(), i!())),
        "st_interiorringn"
    );
}

#[test]
fn debug_query_st_geometryn() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_geometryn(g!(), i!())),
        "st_geometryn"
    );
}

#[test]
fn debug_query_st_xmin() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_xmin(g!())), "st_xmin");
}

#[test]
fn debug_query_st_xmax() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_xmax(g!())), "st_xmax");
}

#[test]
fn debug_query_st_ymin() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_ymin(g!())), "st_ymin");
}

#[test]
fn debug_query_st_ymax() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_ymax(g!())), "st_ymax");
}

// -- Measurement functions ----------------------------------------------------

#[test]
fn debug_query_st_area() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_area(g!())), "st_area");
}

#[test]
fn debug_query_st_length() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_length(g!())), "st_length");
}

#[test]
fn debug_query_st_perimeter() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_perimeter(g!())), "st_perimeter");
}

#[test]
fn debug_query_st_distance() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_distance(g!(), g!())), "st_distance");
}

#[test]
fn debug_query_st_distancesphere() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_distancesphere(g!(), g!())),
        "st_distancesphere"
    );
}

#[test]
fn debug_query_st_distancespheroid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_distancespheroid(g!(), g!())),
        "st_distancespheroid"
    );
}

#[test]
fn debug_query_st_centroid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_centroid(g!())), "st_centroid");
}

#[test]
fn debug_query_st_pointonsurface() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_pointonsurface(g!())),
        "st_pointonsurface"
    );
}

#[test]
fn debug_query_st_hausdorffdistance() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_hausdorffdistance(g!(), g!())),
        "st_hausdorffdistance"
    );
}

// -- Predicate functions ------------------------------------------------------

#[test]
fn debug_query_st_intersects() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_intersects(g!(), g!())),
        "st_intersects"
    );
}

#[test]
fn debug_query_st_contains() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_contains(g!(), g!())), "st_contains");
}

#[test]
fn debug_query_st_within() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_within(g!(), g!())), "st_within");
}

#[test]
fn debug_query_inside_area() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(inside_area(g!(), g!())), "st_within");
}

#[test]
fn debug_query_st_covers() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_covers(g!(), g!())), "st_covers");
}

#[test]
fn debug_query_st_coveredby() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_coveredby(g!(), g!())),
        "st_coveredby"
    );
}

#[test]
fn debug_query_st_disjoint() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_disjoint(g!(), g!())), "st_disjoint");
}

#[test]
fn debug_query_outside_area() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(outside_area(g!(), g!())), "st_disjoint");
}

#[test]
fn debug_query_st_equals() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_equals(g!(), g!())), "st_equals");
}

#[test]
fn debug_query_st_dwithin() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_dwithin(g!(), g!(), d!())),
        "st_dwithin"
    );
}

#[test]
fn debug_query_st_dwithinsphere() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_dwithinsphere(g!(), g!(), d!())),
        "st_dwithinsphere"
    );
}

#[test]
fn debug_query_st_dwithinspheroid() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_dwithinspheroid(g!(), g!(), d!())),
        "st_dwithinspheroid"
    );
}

#[test]
fn debug_query_st_relate() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_relate(g!(), g!())), "st_relate");
}

#[test]
fn debug_query_st_relate_match_geoms() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_relate_match_geoms(g!(), g!(), t!())),
        "st_relate"
    );
}

#[test]
fn debug_query_st_relatematch() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_relatematch(t!(), t!())),
        "st_relatematch"
    );
}

#[test]
fn debug_query_st_relate_match() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_relate_match(t!(), t!())),
        "st_relatematch"
    );
}

// -- Geography variant functions ----------------------------------------------

#[test]
fn debug_query_st_lengthsphere() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_lengthsphere(g!())),
        "st_lengthsphere"
    );
}

#[test]
fn debug_query_st_azimuth() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(diesel::dsl::select(st_azimuth(g!(), g!())), "st_azimuth");
}

#[test]
fn debug_query_st_project() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_project(g!(), d!(), d!())),
        "st_project"
    );
}

#[test]
fn debug_query_st_closestpoint() {
    use geolite_diesel::functions::*;
    assert_sql_contains!(
        diesel::dsl::select(st_closestpoint(g!(), g!())),
        "st_closestpoint"
    );
}
