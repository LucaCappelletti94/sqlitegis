use diesel::prelude::*;
use geolite::diesel::functions::*;

macro_rules! run_predicates_and_relate_bool_semantics {
    ($conn:expr) => {{
        let conn = $conn;
        let is_empty: Option<bool> =
            diesel::dsl::select(st_isempty(st_geomfromtext("GEOMETRYCOLLECTION EMPTY")))
                .get_result(conn)
                .unwrap();
        assert_eq!(is_empty, Some(true));

        let is_not_empty: Option<bool> =
            diesel::dsl::select(st_isempty(st_geomfromtext("POINT(1 1)")))
                .get_result(conn)
                .unwrap();
        assert_eq!(is_not_empty, Some(false));

        let is_valid: Option<bool> = diesel::dsl::select(st_isvalid(st_geomfromtext(
            "POLYGON((0 0,0 3,3 3,3 0,0 0))",
        )))
        .get_result(conn)
        .unwrap();
        assert_eq!(is_valid, Some(true));

        let is_invalid: Option<bool> = diesel::dsl::select(st_isvalid(st_geomfromtext(
            "POLYGON((0 0,2 2,0 2,2 0,0 0))",
        )))
        .get_result(conn)
        .unwrap();
        assert_eq!(is_invalid, Some(false));

        let intersects: Option<bool> = diesel::dsl::select(st_intersects(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(intersects, Some(true));

        let intersects_false: Option<bool> = diesel::dsl::select(st_intersects(
            st_geomfromtext("POINT(10 10)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(intersects_false, Some(false));

        let contains: Option<bool> = diesel::dsl::select(st_contains(
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
            st_geomfromtext("POLYGON((1 1,1 2,2 2,2 1,1 1))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(contains, Some(true));

        let within: Option<bool> = diesel::dsl::select(st_within(
            st_geomfromtext("POLYGON((1 1,1 2,2 2,2 1,1 1))"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(within, Some(true));

        let inside_area_interior: Option<bool> = diesel::dsl::select(inside_area(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(inside_area_interior, Some(true));

        // Boundary touch: strict inside should be false.
        let inside_area_boundary: Option<bool> = diesel::dsl::select(inside_area(
            st_geomfromtext("POINT(0 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(inside_area_boundary, Some(false));

        let covers: Option<bool> = diesel::dsl::select(st_covers(
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
            st_geomfromtext("POINT(0 1)"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(covers, Some(true));

        let covered_by: Option<bool> = diesel::dsl::select(st_coveredby(
            st_geomfromtext("POINT(0 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(covered_by, Some(true));

        let disjoint: Option<bool> = diesel::dsl::select(st_disjoint(
            st_geomfromtext("POINT(10 10)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(disjoint, Some(true));

        let outside_area_disjoint: Option<bool> = diesel::dsl::select(outside_area(
            st_geomfromtext("POINT(10 10)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(outside_area_disjoint, Some(true));

        // Boundary touch: strict outside should be false.
        let outside_area_boundary: Option<bool> = diesel::dsl::select(outside_area(
            st_geomfromtext("POINT(0 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(outside_area_boundary, Some(false));

        let outside_area_interior: Option<bool> = diesel::dsl::select(outside_area(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(outside_area_interior, Some(false));

        let equals: Option<bool> = diesel::dsl::select(st_equals(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POINT(1 1)"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(equals, Some(true));

        let dwithin_true: Option<bool> = diesel::dsl::select(st_dwithin(
            st_geomfromtext("POINT(0 0)"),
            st_geomfromtext("POINT(3 4)"),
            5.0,
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(dwithin_true, Some(true));

        let dwithin_false: Option<bool> = diesel::dsl::select(st_dwithin(
            st_geomfromtext("POINT(0 0)"),
            st_geomfromtext("POINT(3 4)"),
            4.9,
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(dwithin_false, Some(false));

        let touches: Option<bool> = diesel::dsl::select(st_touches(
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
            st_geomfromtext("POLYGON((3 0,3 3,6 3,6 0,3 0))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(touches, Some(true));

        let crosses: Option<bool> = diesel::dsl::select(st_crosses(
            st_geomfromtext("LINESTRING(0 0,2 2)"),
            st_geomfromtext("LINESTRING(0 2,2 0)"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(crosses, Some(true));

        let overlaps: Option<bool> = diesel::dsl::select(st_overlaps(
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
            st_geomfromtext("POLYGON((2 2,2 4,4 4,4 2,2 2))"),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(overlaps, Some(true));

        let matrix: Option<String> = diesel::dsl::select(st_relate(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
        ))
        .get_result(conn)
        .unwrap();
        let matrix = matrix.expect("st_relate should return a DE-9IM matrix");
        assert_eq!(matrix.len(), 9);

        let relate_match_geoms_exact: Option<bool> = diesel::dsl::select(st_relate_match_geoms(
            st_geomfromtext("POINT(1 1)"),
            st_geomfromtext("POLYGON((0 0,0 3,3 3,3 0,0 0))"),
            matrix.as_str(),
        ))
        .get_result(conn)
        .unwrap();
        assert_eq!(relate_match_geoms_exact, Some(true));

        let relate_match_exact: Option<bool> =
            diesel::dsl::select(st_relatematch(matrix.as_str(), matrix.as_str()))
                .get_result(conn)
                .unwrap();
        assert_eq!(relate_match_exact, Some(true));

        let relate_match_false: Option<bool> =
            diesel::dsl::select(st_relatematch(matrix.as_str(), "FFFFFFFFF"))
                .get_result(conn)
                .unwrap();
        assert_eq!(relate_match_false, Some(false));

        let relate_match_alias_exact: Option<bool> =
            diesel::dsl::select(st_relate_match(matrix.as_str(), matrix.as_str()))
                .get_result(conn)
                .unwrap();
        assert_eq!(relate_match_alias_exact, Some(true));

        let relate_match_alias_false: Option<bool> =
            diesel::dsl::select(st_relate_match(matrix.as_str(), "FFFFFFFFF"))
                .get_result(conn)
                .unwrap();
        assert_eq!(relate_match_alias_false, Some(false));
    }};
}

#[cfg(feature = "diesel-sqlite")]
#[allow(dead_code)]
pub fn assert_predicates_and_relate_bool_semantics_sqlite(
    conn: &mut diesel::sqlite::SqliteConnection,
) {
    run_predicates_and_relate_bool_semantics!(conn);
}

#[cfg(feature = "diesel-postgres")]
#[allow(dead_code)]
pub fn assert_predicates_and_relate_bool_semantics_postgres(conn: &mut diesel::pg::PgConnection) {
    run_predicates_and_relate_bool_semantics!(conn);
}
