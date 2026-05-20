#![cfg(all(feature = "diesel-sqlite", not(target_arch = "wasm32")))]
//! Native SQLite integration tests for geolite-diesel.
//!
//! Uses `sqlite3_auto_extension` to register geolite functions on every
//! `SqliteConnection::establish()` call, then exercises spatial functions
//! through the Diesel query builder against a real SQLite database.

use std::sync::Once;

use diesel::prelude::*;
use diesel::sql_query;
use geolite::core::function_catalog::{
    SemanticCase, SemanticExpectation, SqliteFunctionSpec, SQLITE_DETERMINISTIC_FUNCTIONS,
    SQLITE_DIRECT_ONLY_FUNCTIONS,
};

#[path = "diesel_predicate_bool_helpers.rs"]
mod predicate_bool_helpers;

#[derive(QueryableByName, Debug)]
struct PlanRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    detail: String,
}

#[derive(QueryableByName, Debug)]
struct BlobResult {
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Binary>)]
    val: Option<Vec<u8>>,
}

diesel::table! { perf_grid (id) { id -> Integer, geom -> Nullable<geolite::diesel::Geometry>, } }
diesel::table! { perf_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::allow_tables_to_appear_in_same_query!(perf_grid, perf_grid_geom_rtree);

// -- Auto-extension registration ----------------------------------------------

static INIT: Once = Once::new();

/// Entry point called by SQLite for each new connection.
unsafe extern "C" fn geolite_init(
    db: *mut libsqlite3_sys::sqlite3,
    _pz_err_msg: *mut *mut std::ffi::c_char,
    _p_api: *const libsqlite3_sys::sqlite3_api_routines,
) -> std::ffi::c_int {
    geolite::sqlite::register_functions(db)
}

fn conn() -> SqliteConnection {
    INIT.call_once(|| unsafe {
        libsqlite3_sys::sqlite3_auto_extension(Some(geolite_init));
    });
    SqliteConnection::establish(":memory:").unwrap()
}

// -- Shared test definitions --------------------------------------------------

include!("diesel_test_helpers.rs");
define_diesel_sqlite_tests!(test);

#[test]
fn shared_predicates_and_relate_bool_semantics() {
    let mut c = conn();
    predicate_bool_helpers::assert_predicates_and_relate_bool_semantics_sqlite(&mut c);
}

// -- Native-only: deterministic spatial index behavior -----------------------

#[test]
fn spatial_index_narrows_candidates_deterministically() {
    use geolite::diesel::prelude::*;

    let mut c = conn();

    // Create table with 10K points on a 100x100 grid
    sql_query(
        "CREATE TABLE perf_grid (
            id   INTEGER PRIMARY KEY,
            geom BLOB
        )",
    )
    .execute(&mut c)
    .unwrap();

    for i in 0..100 {
        for j in 0..100 {
            let id = i * 100 + j;
            sql_query(format!(
                "INSERT INTO perf_grid (id, geom) VALUES ({id}, ST_Point({i}, {j}))"
            ))
            .execute(&mut c)
            .unwrap();
        }
    }

    // Non-indexed query: scan all rows
    let non_indexed_sql = "SELECT COUNT(*) AS val FROM perf_grid
        WHERE ST_Intersects(geom, ST_MakeEnvelope(20, 20, 30, 30)) = 1";

    // Baseline table cardinality (all rows)
    let full_scan_count = perf_grid::table.count().get_result::<i64>(&mut c).unwrap();
    assert_eq!(full_scan_count, 10_000);

    // Create spatial index
    sql_query("SELECT CreateSpatialIndex('perf_grid', 'geom')")
        .execute(&mut c)
        .unwrap();

    // Indexed query: R-tree join
    let indexed_sql = "SELECT COUNT(*) AS val FROM perf_grid g
        JOIN perf_grid_geom_rtree r ON g.rowid = r.id
         WHERE r.xmin >= 20 AND r.xmax <= 30 AND r.ymin >= 20 AND r.ymax <= 30
        AND ST_Intersects(g.geom, ST_MakeEnvelope(20, 20, 30, 30)) = 1";

    // Deterministic coarse candidate count from the R-tree join only.
    let candidate_count = perf_grid::table
        .inner_join(perf_grid_geom_rtree::table.on(perf_grid::id.eq(perf_grid_geom_rtree::id)))
        .filter(perf_grid_geom_rtree::xmin.ge(20.0))
        .filter(perf_grid_geom_rtree::xmax.le(30.0))
        .filter(perf_grid_geom_rtree::ymin.ge(20.0))
        .filter(perf_grid_geom_rtree::ymax.le(30.0))
        .count()
        .get_result::<i64>(&mut c)
        .unwrap();
    assert_eq!(candidate_count, 121);
    assert!(candidate_count < full_scan_count);

    // Verify correctness: both should return 11*11 = 121 points (20..=30 inclusive).
    let non_indexed_count = perf_grid::table
        .filter(
            perf_grid::geom
                .st_intersects(st_makeenvelope(20.0, 20.0, 30.0, 30.0).nullable())
                .eq(true),
        )
        .count()
        .get_result::<i64>(&mut c)
        .unwrap();
    let indexed_count = perf_grid::table
        .inner_join(perf_grid_geom_rtree::table.on(perf_grid::id.eq(perf_grid_geom_rtree::id)))
        .filter(perf_grid_geom_rtree::xmin.ge(20.0))
        .filter(perf_grid_geom_rtree::xmax.le(30.0))
        .filter(perf_grid_geom_rtree::ymin.ge(20.0))
        .filter(perf_grid_geom_rtree::ymax.le(30.0))
        .filter(
            perf_grid::geom
                .st_intersects(st_makeenvelope(20.0, 20.0, 30.0, 30.0).nullable())
                .eq(true),
        )
        .count()
        .get_result::<i64>(&mut c)
        .unwrap();
    assert_eq!(non_indexed_count, indexed_count);
    assert_eq!(non_indexed_count, 121);

    // Query-plan sanity checks (deterministic structure, not wall-clock timing).
    let non_indexed_plan: Vec<PlanRow> = sql_query(format!("EXPLAIN QUERY PLAN {non_indexed_sql}"))
        .load(&mut c)
        .unwrap();
    assert!(
        non_indexed_plan
            .iter()
            .any(|row| row.detail.contains("SCAN perf_grid")),
        "expected a table scan in non-indexed plan, got: {non_indexed_plan:?}"
    );

    let indexed_plan: Vec<PlanRow> = sql_query(format!("EXPLAIN QUERY PLAN {indexed_sql}"))
        .load(&mut c)
        .unwrap();
    assert!(
        indexed_plan.iter().any(|row| {
            row.detail.contains("perf_grid_geom_rtree")
                || row.detail.contains("VIRTUAL TABLE INDEX")
                || row.detail.contains("USING INDEX")
        }),
        "expected indexed plan to reference the R-tree/index path, got: {indexed_plan:?}"
    );
}

#[test]
fn spatial_index_lifecycle_via_raw_sql() {
    let mut c = conn();

    sql_query(
        "CREATE TABLE lifecycle (
            id   INTEGER PRIMARY KEY,
            geom BLOB
        )",
    )
    .execute(&mut c)
    .unwrap();

    // Index lifecycle is intentionally exercised through raw SQL.
    let created: I32Result = sql_query("SELECT CreateSpatialIndex('lifecycle', 'geom') AS val")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(created.val, Some(1));

    let rtree_table_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'table' AND name = 'lifecycle_geom_rtree'",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(rtree_table_count.val, Some(1));

    let trigger_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'trigger' AND name LIKE 'lifecycle_geom_%'",
    )
    .get_result(&mut c)
    .unwrap();
    assert!(trigger_count.val.expect("trigger count should not be NULL") > 0);

    let dropped: I32Result = sql_query("SELECT DropSpatialIndex('lifecycle', 'geom') AS val")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(dropped.val, Some(1));

    let rtree_table_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'table' AND name = 'lifecycle_geom_rtree'",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(rtree_table_count.val, Some(0));

    let trigger_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'trigger' AND name LIKE 'lifecycle_geom_%'",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(trigger_count.val, Some(0));
}

#[test]
fn spatial_index_stays_in_sync_across_writes() {
    // End-to-end sync verification. The structural test above proves the
    // triggers exist after CreateSpatialIndex. This one proves they fire on
    // the right events and produce the right rtree contents across INSERT,
    // UPDATE, DELETE, and post-drop writes. Failure here means the rtree has
    // drifted from the base table -- the silent-corruption mode the trigger
    // installation is meant to prevent.
    let mut c = conn();
    sql_query("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)")
        .execute(&mut c)
        .unwrap();

    // 1. CreateSpatialIndex on an empty table installs triggers and an empty
    // rtree.
    sql_query("SELECT CreateSpatialIndex('pts', 'geom')")
        .execute(&mut c)
        .unwrap();
    let rtree_count: I32Result = sql_query("SELECT COUNT(*) AS val FROM pts_geom_rtree")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(rtree_count.val, Some(0));

    // 2. INSERT 100 rows. AFTER INSERT must populate the rtree row by row.
    for i in 0..100 {
        sql_query(format!(
            "INSERT INTO pts (id, geom) VALUES ({i}, ST_Point({i}, {i}))"
        ))
        .execute(&mut c)
        .unwrap();
    }
    let rtree_count: I32Result = sql_query("SELECT COUNT(*) AS val FROM pts_geom_rtree")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(rtree_count.val, Some(100));

    // 3. UPDATE 10 geometries to move them out to (1000+i, 1000+i). AFTER
    // UPDATE must re-bound their bboxes so the moved rows are findable at
    // their new positions and absent at their old ones.
    for i in 0..10 {
        sql_query(format!(
            "UPDATE pts SET geom = ST_Point({}, {}) WHERE id = {i}",
            i + 1000,
            i + 1000
        ))
        .execute(&mut c)
        .unwrap();
    }
    let moved_in_rtree: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM pts_geom_rtree \
         WHERE xmin >= 1000 AND xmax <= 1009 AND ymin >= 1000 AND ymax <= 1009",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(moved_in_rtree.val, Some(10));
    let stale_in_rtree: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM pts_geom_rtree \
         WHERE id < 10 AND xmin < 100",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(
        stale_in_rtree.val,
        Some(0),
        "moved rows must not appear at their pre-update positions"
    );

    // 4. DELETE 25 rows. AFTER DELETE must remove their rtree entries.
    sql_query("DELETE FROM pts WHERE id >= 75")
        .execute(&mut c)
        .unwrap();
    let rtree_count: I32Result = sql_query("SELECT COUNT(*) AS val FROM pts_geom_rtree")
        .get_result(&mut c)
        .unwrap();
    assert_eq!(rtree_count.val, Some(75));

    // 5. Cross-check: an rtree-windowed join must return the same count as
    // a full-table-scan predicate evaluation for the same window.
    let full_scan: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM pts \
         WHERE ST_Intersects(geom, ST_MakeEnvelope(0, 0, 50, 50)) = 1",
    )
    .get_result(&mut c)
    .unwrap();
    let rtree_join: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM pts p \
         JOIN pts_geom_rtree r ON p.id = r.id \
         WHERE r.xmin <= 50 AND r.xmax >= 0 AND r.ymin <= 50 AND r.ymax >= 0 \
           AND ST_Intersects(p.geom, ST_MakeEnvelope(0, 0, 50, 50)) = 1",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(full_scan.val, rtree_join.val);
    assert!(
        full_scan.val.expect("full_scan should not be NULL") > 0,
        "window must catch some rows"
    );

    // 6. DropSpatialIndex removes the rtree and all three triggers.
    sql_query("SELECT DropSpatialIndex('pts', 'geom')")
        .execute(&mut c)
        .unwrap();
    let rtree_table_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'table' AND name = 'pts_geom_rtree'",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(rtree_table_count.val, Some(0));
    let trigger_count: I32Result = sql_query(
        "SELECT COUNT(*) AS val FROM sqlite_master \
         WHERE type = 'trigger' AND name LIKE 'pts_geom_%'",
    )
    .get_result(&mut c)
    .unwrap();
    assert_eq!(trigger_count.val, Some(0));

    // 7. After drop, subsequent writes on the base table must not error and
    // must not try to talk to the missing rtree.
    sql_query("INSERT INTO pts (id, geom) VALUES (9999, ST_Point(0, 0))")
        .execute(&mut c)
        .unwrap();
}

fn semantic_case_sql(sql: &str) -> String {
    let trimmed = sql.trim();
    let expr = trimmed
        .strip_prefix("SELECT ")
        .or_else(|| trimmed.strip_prefix("select "))
        .unwrap_or(trimmed);
    format!("SELECT ({expr}) AS val")
}

fn assert_semantic_case_via_diesel(
    c: &mut SqliteConnection,
    spec: &SqliteFunctionSpec,
    case: &SemanticCase,
) {
    let sql = semantic_case_sql(case.sql);
    match case.expected {
        SemanticExpectation::Null => {
            let row: TextResult = sql_query(&sql).get_result(c).unwrap_or_else(|e| {
                panic!(
                    "{}({}) case `{}` failed via `{}`: {e}",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            assert!(
                row.val.is_none(),
                "{}({}) case `{}` expected NULL via `{}`, got {:?}",
                spec.name,
                spec.n_arg,
                case.id,
                case.sql,
                row.val
            );
        }
        SemanticExpectation::NumericFinite => {
            let row: F64Result = sql_query(&sql).get_result(c).unwrap_or_else(|e| {
                panic!(
                    "{}({}) case `{}` failed via `{}`: {e}",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            let value = row.val.unwrap_or_else(|| {
                panic!(
                    "{}({}) case `{}` expected numeric via `{}`, got NULL",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            assert!(
                value.is_finite(),
                "{}({}) case `{}` expected finite numeric via `{}`, got {}",
                spec.name,
                spec.n_arg,
                case.id,
                case.sql,
                value
            );
        }
        SemanticExpectation::TextNonEmpty => {
            let row: TextResult = sql_query(&sql).get_result(c).unwrap_or_else(|e| {
                panic!(
                    "{}({}) case `{}` failed via `{}`: {e}",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            let value = row.val.unwrap_or_else(|| {
                panic!(
                    "{}({}) case `{}` expected text via `{}`, got NULL",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            assert!(
                !value.is_empty(),
                "{}({}) case `{}` expected non-empty text via `{}`",
                spec.name,
                spec.n_arg,
                case.id,
                case.sql
            );
        }
        SemanticExpectation::BlobNonEmpty => {
            let row: BlobResult = sql_query(&sql).get_result(c).unwrap_or_else(|e| {
                panic!(
                    "{}({}) case `{}` failed via `{}`: {e}",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            let value = row.val.unwrap_or_else(|| {
                panic!(
                    "{}({}) case `{}` expected blob via `{}`, got NULL",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            assert!(
                !value.is_empty(),
                "{}({}) case `{}` expected non-empty blob via `{}`",
                spec.name,
                spec.n_arg,
                case.id,
                case.sql
            );
        }
        SemanticExpectation::Bool01 => {
            let row: I32Result = sql_query(&sql).get_result(c).unwrap_or_else(|e| {
                panic!(
                    "{}({}) case `{}` failed via `{}`: {e}",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            let value = row.val.unwrap_or_else(|| {
                panic!(
                    "{}({}) case `{}` expected bool-as-int via `{}`, got NULL",
                    spec.name, spec.n_arg, case.id, case.sql
                )
            });
            assert!(
                value == 0 || value == 1,
                "{}({}) case `{}` expected bool-as-int via `{}`, got {}",
                spec.name,
                spec.n_arg,
                case.id,
                case.sql,
                value
            );
        }
        SemanticExpectation::ErrorContains(expected_substring) => {
            let err = sql_query(&sql)
                .get_result::<TextResult>(c)
                .expect_err("semantic case expected to fail");
            let msg = format!("{err}");
            assert!(
                msg.contains(expected_substring),
                "{}({}) case `{}` expected error containing `{}` via `{}`, got `{}`",
                spec.name,
                spec.n_arg,
                case.id,
                expected_substring,
                case.sql,
                msg
            );
        }
    }
}

#[test]
fn catalog_semantic_goldens_via_diesel_sqlite() {
    let mut c = conn();

    sql_query("CREATE TABLE _rt(geom BLOB)")
        .execute(&mut c)
        .expect("semantic goldens require direct-only helper table");

    for spec in SQLITE_DETERMINISTIC_FUNCTIONS {
        for case in spec.semantic_cases {
            assert_semantic_case_via_diesel(&mut c, spec, case);
        }
    }

    for spec in SQLITE_DIRECT_ONLY_FUNCTIONS {
        for case in spec.semantic_cases {
            assert_semantic_case_via_diesel(&mut c, spec, case);
        }
    }
}
