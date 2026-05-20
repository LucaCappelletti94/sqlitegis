#![cfg(feature = "diesel-postgres")]
#![allow(dead_code)]

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use geolite::diesel::functions::*;
use std::time::Duration;
use testcontainers_modules::postgres::Postgres;
use testcontainers_modules::testcontainers::runners::AsyncRunner;
use testcontainers_modules::testcontainers::ImageExt;

#[path = "diesel_predicate_bool_helpers.rs"]
mod predicate_bool_helpers;

diesel::table! {
    t (id) {
        id -> diesel::sql_types::Integer,
        geom -> diesel::sql_types::Nullable<geolite::diesel::types::Geometry>,
        geog -> diesel::sql_types::Nullable<geolite::diesel::types::Geography>,
    }
}

// -- Helper: start a PostGIS container and return (container, connection) ------

async fn pg_conn(
    tag: &str,
) -> (
    testcontainers_modules::testcontainers::ContainerAsync<Postgres>,
    PgConnection,
) {
    let mut last_start_err = None;
    let mut container = None;
    for attempt in 1..=3 {
        match Postgres::default()
            .with_name("postgis/postgis")
            .with_tag(tag)
            .start()
            .await
        {
            Ok(c) => {
                container = Some(c);
                break;
            }
            Err(err) => {
                last_start_err = Some(err.to_string());
                if attempt < 3 {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
    let container = container.unwrap_or_else(|| {
        panic!(
            "failed to start PostGIS container after retries: {}",
            last_start_err.unwrap_or_else(|| "unknown start error".to_string())
        )
    });

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(5432).await.unwrap();

    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");

    // PostGIS needs a moment; retry connection a few times.
    let mut conn = None;
    for _ in 0..30 {
        match PgConnection::establish(&url) {
            Ok(c) => {
                conn = Some(c);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
        }
    }
    let mut conn = conn.expect("could not connect to PostGIS container");

    // Ensure PostGIS extension is loaded and create test table.
    conn.batch_execute(
        "
        CREATE EXTENSION IF NOT EXISTS postgis;
        CREATE TABLE t (
            id   SERIAL PRIMARY KEY,
            geom geometry,
            geog geography
        );
        ",
    )
    .unwrap();

    (container, conn)
}

fn geom_from_wkt(wkt: &str) -> geo::Geometry<f64> {
    let ewkb = geolite::core::functions::io::geom_from_text(wkt, None).unwrap();
    let (geom, _srid) = geolite::core::ewkb::parse_ewkb(&ewkb).unwrap();
    geom
}

fn geometry_samples() -> Vec<(&'static str, geo::Geometry<f64>)> {
    vec![
        ("Point", geom_from_wkt("POINT(1 2)")),
        ("LineString", geom_from_wkt("LINESTRING(0 0,1 1,2 0)")),
        ("Polygon", geom_from_wkt("POLYGON((0 0,0 4,4 4,4 0,0 0))")),
        ("MultiPoint", geom_from_wkt("MULTIPOINT((0 0),(1 2),(2 1))")),
        (
            "MultiLineString",
            geom_from_wkt("MULTILINESTRING((0 0,1 1),(2 2,3 3,4 2))"),
        ),
        (
            "MultiPolygon",
            geom_from_wkt("MULTIPOLYGON(((0 0,0 1,1 1,1 0,0 0)),((2 2,2 3,3 3,3 2,2 2)))"),
        ),
        (
            "GeometryCollection",
            geom_from_wkt(
                "GEOMETRYCOLLECTION(POINT(1 2),LINESTRING(0 0,1 1),POLYGON((0 0,0 1,1 1,1 0,0 0)))",
            ),
        ),
    ]
}

// -- Test macro: generate a module per PG version -----------------------------

macro_rules! postgis_tests {
    ($mod_name:ident, $tag:expr) => {
        mod $mod_name {
            use super::*;

            // -- 1. Type roundtrips ---------------------------------------

            #[tokio::test]
            async fn type_roundtrips() {
                let (_container, mut c) = pg_conn($tag).await;
                let mut id = 1;

                for (type_name, geom) in geometry_samples() {
                    // geo::Geometry<f64> roundtrip -- Geometry
                    diesel::insert_into(t::table)
                        .values((t::id.eq(id), t::geom.eq(Some(geom.clone()))))
                        .execute(&mut c)
                        .unwrap();

                    let decoded_geom: Option<geo::Geometry<f64>> =
                        t::table.find(id).select(t::geom).first(&mut c).unwrap();
                    assert_eq!(
                        decoded_geom,
                        Some(geom.clone()),
                        "Geometry geo::Geometry roundtrip failed for {type_name}"
                    );

                    // Geometry ToSql writes no SRID.
                    let blob: Option<Vec<u8>> =
                        t::table.find(id).select(t::geom).first(&mut c).unwrap();
                    let blob = blob.expect("geom should not be NULL");
                    let (parsed_geom, srid) = geolite::core::ewkb::parse_ewkb(&blob).unwrap();
                    assert_eq!(srid, None, "Geometry SRID must be None for {type_name}");
                    assert_eq!(
                        parsed_geom, geom,
                        "Geometry EWKB parse mismatch for {type_name}"
                    );
                    id += 1;

                    // geo::Geometry<f64> roundtrip -- Geography
                    diesel::insert_into(t::table)
                        .values((t::id.eq(id), t::geog.eq(Some(geom.clone()))))
                        .execute(&mut c)
                        .unwrap();

                    let decoded_geog: Option<geo::Geometry<f64>> =
                        t::table.find(id).select(t::geog).first(&mut c).unwrap();
                    assert_eq!(
                        decoded_geog,
                        Some(geom.clone()),
                        "Geography geo::Geometry roundtrip failed for {type_name}"
                    );

                    // Geography ToSql writes SRID=4326.
                    let blob: Option<Vec<u8>> =
                        t::table.find(id).select(t::geog).first(&mut c).unwrap();
                    let blob = blob.expect("geog should not be NULL");
                    let (parsed_geom, srid) = geolite::core::ewkb::parse_ewkb(&blob).unwrap();
                    assert_eq!(
                        srid,
                        Some(4326),
                        "Geography SRID must be 4326 for {type_name}"
                    );
                    assert_eq!(
                        parsed_geom, geom,
                        "Geography EWKB parse mismatch for {type_name}"
                    );
                    id += 1;

                    // Vec<u8> EWKB roundtrip -- Geometry
                    let ewkb = geolite::core::ewkb::write_ewkb(&geom, None).unwrap();
                    diesel::insert_into(t::table)
                        .values((t::id.eq(id), t::geom.eq(Some(ewkb.clone()))))
                        .execute(&mut c)
                        .unwrap();

                    let got_geom_ewkb: Option<Vec<u8>> =
                        t::table.find(id).select(t::geom).first(&mut c).unwrap();
                    assert_eq!(
                        got_geom_ewkb,
                        Some(ewkb.clone()),
                        "Geometry Vec<u8> roundtrip failed for {type_name}"
                    );
                    id += 1;

                    // Vec<u8> EWKB roundtrip -- Geography
                    let ewkb_geog = geolite::core::ewkb::write_ewkb(&geom, Some(4326)).unwrap();
                    diesel::insert_into(t::table)
                        .values((t::id.eq(id), t::geog.eq(Some(ewkb_geog.clone()))))
                        .execute(&mut c)
                        .unwrap();

                    let got_geog_ewkb: Option<Vec<u8>> =
                        t::table.find(id).select(t::geog).first(&mut c).unwrap();
                    assert_eq!(
                        got_geog_ewkb,
                        Some(ewkb_geog.clone()),
                        "Geography Vec<u8> roundtrip failed for {type_name}"
                    );
                    id += 1;

                    // [u8] slice ToSql -- Geometry
                    diesel::insert_into(t::table)
                        .values((t::id.eq(id), t::geom.eq(Some(&ewkb[..]))))
                        .execute(&mut c)
                        .unwrap();

                    let got_slice_ewkb: Option<Vec<u8>> =
                        t::table.find(id).select(t::geom).first(&mut c).unwrap();
                    assert_eq!(
                        got_slice_ewkb,
                        Some(ewkb),
                        "[u8] ToSql roundtrip failed for {type_name}"
                    );
                    id += 1;
                }

                // NULL handling
                diesel::insert_into(t::table)
                    .values((t::id.eq(id), t::geom.eq::<Option<Vec<u8>>>(None)))
                    .execute(&mut c)
                    .unwrap();

                let null_geom: Option<Vec<u8>> =
                    t::table.find(id).select(t::geom).first(&mut c).unwrap();
                assert!(null_geom.is_none());
            }

            // -- 2. PostGIS I/O functions ---------------------------------

            #[tokio::test]
            async fn postgis_io_functions() {
                let (_container, mut c) = pg_conn($tag).await;

                // ST_GeomFromText / ST_AsText roundtrip
                let val: Option<String> =
                    diesel::dsl::select(st_astext(st_geomfromtext("POINT(1 2)")))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), "POINT(1 2)");

                // ST_GeomFromText with SRID
                let val: Option<i32> =
                    diesel::dsl::select(st_srid(st_geomfromtext_srid("POINT(1 2)", 4326)))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), 4326);

                // ST_AsEWKT (verify SRID in output)
                let val: Option<String> =
                    diesel::dsl::select(st_asewkt(st_geomfromtext_srid("POINT(1 2)", 4326)))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), "SRID=4326;POINT(1 2)");

                // ST_AsGeoJSON / ST_GeomFromGeoJSON roundtrip
                let val: Option<String> = diesel::dsl::select(st_astext(st_geomfromgeojson(
                    r#"{"type":"Point","coordinates":[1,2]}"#,
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(val.unwrap(), "POINT(1 2)");

                // ST_GeomFromGeoJSON defaults to SRID=4326 in PostGIS 3+.
                let val: Option<i32> = diesel::dsl::select(st_srid(st_geomfromgeojson(
                    r#"{"type":"Point","coordinates":[1,2]}"#,
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(val.unwrap(), 4326);

                // SRID overrides should be done with ST_SetSRID(...), not a 2-arg GeoJSON parser.
                let val: Option<i32> = diesel::dsl::select(st_srid(st_setsrid(
                    st_geomfromgeojson(r#"{"type":"Point","coordinates":[1,2]}"#),
                    3857,
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(val.unwrap(), 3857);

                // ST_Point constructor
                let val: Option<String> =
                    diesel::dsl::select(st_astext(st_point(3.0, 4.0).nullable()))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), "POINT(3 4)");

                // ST_MakeEnvelope constructor
                let val: Option<String> =
                    diesel::dsl::select(st_astext(st_makeenvelope(0.0, 0.0, 1.0, 1.0).nullable()))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), "POLYGON((0 0,0 1,1 1,1 0,0 0))");
            }

            // -- 3. PostGIS accessor functions ----------------------------

            #[tokio::test]
            async fn postgis_accessor_functions() {
                let (_container, mut c) = pg_conn($tag).await;

                // ST_SRID / ST_SetSRID
                let val: Option<i32> =
                    diesel::dsl::select(st_srid(st_setsrid(st_point(0.0, 0.0).nullable(), 4326)))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), 4326);

                // ST_GeometryType
                let val: Option<String> =
                    diesel::dsl::select(st_geometrytype(st_point(0.0, 0.0).nullable()))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(val.unwrap(), "ST_Point");

                // ST_X / ST_Y
                let x: Option<f64> = diesel::dsl::select(st_x(st_point(3.5, 7.25).nullable()))
                    .get_result(&mut c)
                    .unwrap();
                assert!((x.unwrap() - 3.5).abs() < 1e-10);

                let y: Option<f64> = diesel::dsl::select(st_y(st_point(3.5, 7.25).nullable()))
                    .get_result(&mut c)
                    .unwrap();
                assert!((y.unwrap() - 7.25).abs() < 1e-10);

                // ST_Z (Point Z -> value; Point XY -> NULL)
                let z: Option<f64> = diesel::dsl::select(st_z(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        "decode('0101000080000000000000F03F00000000000000400000000000000840','hex')",
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(z, Some(3.0));

                let z_xy: Option<f64> =
                    diesel::dsl::select(st_z(st_point(3.5, 7.25).nullable()))
                        .get_result(&mut c)
                        .unwrap();
                assert_eq!(z_xy, None);

                // ST_Area (polygon)
                let area: Option<f64> =
                    diesel::dsl::select(st_area(st_geomfromtext("POLYGON((0 0,1 0,1 1,0 1,0 0))")))
                        .get_result(&mut c)
                        .unwrap();
                assert!((area.unwrap() - 1.0).abs() < 1e-10);

                // ST_Distance (two points)
                let dist: Option<f64> = diesel::dsl::select(st_distance(
                    st_point(0.0, 0.0).nullable(),
                    st_point(3.0, 4.0).nullable(),
                ))
                .get_result(&mut c)
                .unwrap();
                assert!((dist.unwrap() - 5.0).abs() < 1e-10);

                // ST_Length (linestring)
                let length: Option<f64> =
                    diesel::dsl::select(st_length(st_geomfromtext("LINESTRING(0 0, 3 4)")))
                        .get_result(&mut c)
                        .unwrap();
                assert!((length.unwrap() - 5.0).abs() < 1e-10);

                // ST_Centroid
                let centroid_wkt: Option<String> = diesel::dsl::select(st_astext(st_centroid(
                    st_geomfromtext("POLYGON((0 0,2 0,2 2,0 2,0 0))"),
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(centroid_wkt.unwrap(), "POINT(1 1)");

                // ST_Buffer (basic check: result is non-null and a polygon)
                let geom_type: Option<String> = diesel::dsl::select(st_geometrytype(st_buffer(
                    st_point(0.0, 0.0).nullable(),
                    1.0,
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(geom_type.unwrap(), "ST_Polygon");
            }

            // -- 4. PostGIS spatial operations ----------------------------

            #[tokio::test]
            async fn postgis_spatial_operations() {
                let (_container, mut c) = pg_conn($tag).await;

                // ST_Union -- area should be 7.0 (4 + 4 - 1 overlap)
                let union_area: Option<f64> = diesel::dsl::select(st_area(st_union(
                    st_geomfromtext("POLYGON((0 0,2 0,2 2,0 2,0 0))"),
                    st_geomfromtext("POLYGON((1 1,3 1,3 3,1 3,1 1))"),
                )))
                .get_result(&mut c)
                .unwrap();
                assert!((union_area.unwrap() - 7.0).abs() < 1e-10);

                // ST_Intersection -- area should be 1.0
                let intersection_area: Option<f64> = diesel::dsl::select(st_area(st_intersection(
                    st_geomfromtext("POLYGON((0 0,2 0,2 2,0 2,0 0))"),
                    st_geomfromtext("POLYGON((1 1,3 1,3 3,1 3,1 1))"),
                )))
                .get_result(&mut c)
                .unwrap();
                assert!((intersection_area.unwrap() - 1.0).abs() < 1e-10);

                // ST_Difference (A - B) -- area should be 3.0
                let difference_area: Option<f64> = diesel::dsl::select(st_area(st_difference(
                    st_geomfromtext("POLYGON((0 0,2 0,2 2,0 2,0 0))"),
                    st_geomfromtext("POLYGON((1 1,3 1,3 3,1 3,1 1))"),
                )))
                .get_result(&mut c)
                .unwrap();
                assert!((difference_area.unwrap() - 3.0).abs() < 1e-10);
            }

            // -- 5. Predicate and DE-9IM bool semantics ------------------

            #[tokio::test]
            async fn postgis_predicates_and_relate_bool_semantics() {
                let (_container, mut c) = pg_conn($tag).await;
                predicate_bool_helpers::assert_predicates_and_relate_bool_semantics_postgres(
                    &mut c,
                );
            }

            // -- 6. EWKB Z/M pass-through via query builder --------------

            #[tokio::test]
            async fn postgis_ewkb_zm_metadata_via_query_builder() {
                let (_container, mut c) = pg_conn($tag).await;
                let ewkb_zm_le =
                    "decode('01010000C0000000000000F03F000000000000004000000000000008400000000000001040','hex')";
                let ewkb_zm_be =
                    "decode('00C00000013FF0000000000000400000000000000040080000000000004010000000000000','hex')";

                // Point ZM (1,2,3,4), little-endian EWKB in -> Z/M header flags out.
                let out_le: Option<Vec<u8>> = diesel::dsl::select(st_asewkb(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        ewkb_zm_le,
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                let out_le = out_le.expect("ST_AsEWKB should not return NULL");
                let hdr_le = geolite::core::ewkb::parse_ewkb_header(&out_le).unwrap();
                assert!(hdr_le.has_z);
                assert!(hdr_le.has_m);
                let ndims_le: Option<i16> = diesel::dsl::select(st_ndims(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        ewkb_zm_le,
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(ndims_le, Some(4));
                let coorddim_le: Option<i16> = diesel::dsl::select(st_coorddim(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        ewkb_zm_le,
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(coorddim_le, Some(4));
                let zmflag_le: Option<i16> = diesel::dsl::select(st_zmflag(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        ewkb_zm_le,
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                assert_eq!(zmflag_le, Some(3));

                // Same Point ZM payload in big-endian EWKB in -> Z/M header flags out.
                let out_be: Option<Vec<u8>> = diesel::dsl::select(st_asewkb(st_geomfromewkb(
                    diesel::dsl::sql::<diesel::sql_types::Nullable<diesel::sql_types::Binary>>(
                        ewkb_zm_be,
                    ),
                )))
                .get_result(&mut c)
                .unwrap();
                let out_be = out_be.expect("ST_AsEWKB should not return NULL");
                let hdr_be = geolite::core::ewkb::parse_ewkb_header(&out_be).unwrap();
                assert!(hdr_be.has_z);
                assert!(hdr_be.has_m);
            }
        }
    };
}

postgis_tests!(pg15, "15-3.5");
postgis_tests!(pg16, "16-3.5");
postgis_tests!(pg17, "17-3.5");
