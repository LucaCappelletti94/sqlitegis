#![cfg(all(feature = "sqlite", not(target_arch = "wasm32")))]

use std::sync::Once;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use diesel::prelude::*;
use diesel::sql_query;
use geolite::diesel::prelude::*;
use std::hint::black_box;

diesel::table! {
    sw_grid (id) {
        id -> Integer,
        geom -> Nullable<geolite::diesel::Geometry>,
    }
}

diesel::table! {
    sk_grid (id) {
        id -> Integer,
        geom -> Nullable<geolite::diesel::Geometry>,
    }
}

diesel::table! { sw_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }
diesel::table! { sk_grid_geom_rtree (id) { id -> Integer, xmin -> Double, xmax -> Double, ymin -> Double, ymax -> Double, } }

diesel::allow_tables_to_appear_in_same_query!(sw_grid, sw_grid_geom_rtree);
diesel::allow_tables_to_appear_in_same_query!(sk_grid, sk_grid_geom_rtree);

static INIT: Once = Once::new();

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
    SqliteConnection::establish(":memory:").expect("failed to create in-memory sqlite connection")
}

fn seed_grid(c: &mut SqliteConnection, table: &str) {
    sql_query(format!(
        "CREATE TABLE {table} (id INTEGER PRIMARY KEY, geom BLOB)"
    ))
    .execute(c)
    .expect("failed to create bench table");

    sql_query("BEGIN")
        .execute(c)
        .expect("failed to start bench transaction");
    for x in 0..100 {
        for y in 0..100 {
            let id = x * 100 + y;
            sql_query(format!(
                "INSERT INTO {table} (id, geom) VALUES ({id}, ST_Point({x}, {y}))"
            ))
            .execute(c)
            .expect("failed to insert bench row");
        }
    }
    sql_query("COMMIT")
        .execute(c)
        .expect("failed to commit bench transaction");

    sql_query(format!("SELECT CreateSpatialIndex('{table}', 'geom')"))
        .execute(c)
        .expect("failed to create spatial index for bench");
}

fn run_full_intersects(c: &mut SqliteConnection) -> Vec<i32> {
    sw_grid::table
        .filter(
            sw_grid::geom
                .st_intersects(st_makeenvelope(10.0, 10.0, 20.0, 20.0).nullable())
                .eq(true),
        )
        .order(sw_grid::id.asc())
        .select(sw_grid::id)
        .load(c)
        .expect("full-scan intersects query failed")
}

fn run_full_knn(c: &mut SqliteConnection, x: f64, y: f64) -> Vec<i32> {
    sk_grid::table
        .order(sk_grid::geom.st_distance(st_point(x, y).nullable()))
        .then_order_by(sk_grid::id.asc())
        .select(sk_grid::id)
        .limit(5)
        .load(c)
        .expect("full-scan knn query failed")
}

fn run_indexed_intersects(c: &mut SqliteConnection) -> Vec<i32> {
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
        .order(sw_grid::id.asc())
        .select(sw_grid::id)
        .load(c)
        .expect("indexed intersects query failed")
}

fn run_indexed_knn(c: &mut SqliteConnection, qx: f64, qy: f64) -> Vec<i32> {
    let xmin = qx - 5.0;
    let xmax = qx + 5.0;
    let ymin = qy - 5.0;
    let ymax = qy + 5.0;

    sk_grid::table
        .inner_join(sk_grid_geom_rtree::table.on(sk_grid::id.eq(sk_grid_geom_rtree::id)))
        .filter(sk_grid_geom_rtree::xmax.ge(xmin))
        .filter(sk_grid_geom_rtree::xmin.le(xmax))
        .filter(sk_grid_geom_rtree::ymax.ge(ymin))
        .filter(sk_grid_geom_rtree::ymin.le(ymax))
        .order(sk_grid::geom.st_distance(st_point(qx, qy).nullable()))
        .then_order_by(sk_grid::id.asc())
        .select(sk_grid::id)
        .limit(5)
        .load(c)
        .expect("indexed knn query failed")
}

fn bench_intersects_window(c: &mut Criterion) {
    let mut conn = conn();
    seed_grid(&mut conn, "sw_grid");

    let full_rows = run_full_intersects(&mut conn);
    let indexed_rows = run_indexed_intersects(&mut conn);
    assert_eq!(indexed_rows, full_rows);
    assert_eq!(full_rows.len(), 121);

    let mut group = c.benchmark_group("diesel_spatial_index/intersects_window");
    group.throughput(Throughput::Elements(full_rows.len() as u64));
    group.bench_function("indexed_rtree_join", |b| {
        b.iter(|| black_box(run_indexed_intersects(&mut conn)));
    });
    group.bench_function("non_indexed_diesel", |b| {
        b.iter(|| black_box(run_full_intersects(&mut conn)));
    });
    group.finish();
}

fn bench_knn(c: &mut Criterion) {
    let mut conn = conn();
    seed_grid(&mut conn, "sk_grid");

    let qx = 50.25;
    let qy = 50.75;
    let full_rows = run_full_knn(&mut conn, qx, qy);
    let indexed_rows = run_indexed_knn(&mut conn, qx, qy);
    assert_eq!(indexed_rows, full_rows);
    assert_eq!(full_rows.len(), 5);

    let mut group = c.benchmark_group("diesel_spatial_index/knn");
    group.throughput(Throughput::Elements(full_rows.len() as u64));
    group.bench_function("indexed_rtree_join", |b| {
        b.iter(|| black_box(run_indexed_knn(&mut conn, qx, qy)));
    });
    group.bench_function("non_indexed_diesel", |b| {
        b.iter(|| black_box(run_full_knn(&mut conn, qx, qy)));
    });
    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(4))
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_intersects_window, bench_knn
}
criterion_main!(benches);
