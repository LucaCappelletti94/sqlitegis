//! Head-to-head benchmark: sqlitegis vs SpatiaLite on identical workloads.
//!
//! sqlitegis is registered in-process on a `libsqlite3-sys` connection via
//! `register_functions(db)`. SpatiaLite is loaded as an external loadable
//! extension via `sqlite3_load_extension('mod_spatialite')`. Both sides
//! use the same underlying libsqlite3 (the one `libsqlite3-sys`'s bundled
//! C amalgamation produces), so the comparison isolates predicate-callback
//! cost from SQLite engine differences.
//!
//! Run with:
//!
//! ```sh
//! cargo bench --features bench-spatialite spatialite_vs_sqlitegis
//! ```
//!
//! Requires `libsqlite3-mod-spatialite` to be installed system-wide so the
//! SQLite loader can find `mod_spatialite`.

#![cfg(all(feature = "bench-spatialite", not(target_arch = "wasm32")))]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libsqlite3_sys::*;
use std::ffi::{CStr, CString};
use std::hint::black_box;
use std::ptr;

const SPATIALITE_LIB: &str = "mod_spatialite";

// Dataset size. Large enough for the unindexed scan to be measurable,
// small enough that the bench finishes in well under a minute.
const N_POINTS: usize = 50_000;

// PRNG seed so every bench iteration sees the same dataset.
const RNG_SEED: u64 = 0xC0FFEE;

unsafe fn open_sqlitegis_db() -> *mut sqlite3 {
    unsafe {
        let mut db = ptr::null_mut();
        let memdb = CString::new(":memory:").unwrap();
        assert_eq!(
            sqlite3_open(memdb.as_ptr(), &mut db),
            SQLITE_OK,
            "sqlite3_open failed"
        );
        let rc = sqlitegis::sqlite::register_functions(db);
        assert_eq!(rc, SQLITE_OK, "register_functions failed (rc={rc})");
        seed(db, Kind::Sqlitegis);
        db
    }
}

unsafe fn open_spatialite_db() -> *mut sqlite3 {
    unsafe {
        let mut db = ptr::null_mut();
        let memdb = CString::new(":memory:").unwrap();
        assert_eq!(
            sqlite3_open(memdb.as_ptr(), &mut db),
            SQLITE_OK,
            "sqlite3_open failed"
        );
        assert_eq!(
            sqlite3_enable_load_extension(db, 1),
            SQLITE_OK,
            "enable_load_extension failed"
        );
        let cpath = CString::new(SPATIALITE_LIB).unwrap();
        let mut err: *mut std::os::raw::c_char = ptr::null_mut();
        let rc = sqlite3_load_extension(db, cpath.as_ptr(), ptr::null(), &mut err);
        if rc != SQLITE_OK {
            let msg = if err.is_null() {
                "(no message)".to_string()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            panic!("load_extension({SPATIALITE_LIB}) failed: rc={rc} err={msg}");
        }
        exec(db, "SELECT InitSpatialMetaData(1)");
        seed(db, Kind::Spatialite);
        db
    }
}

unsafe fn exec(db: *mut sqlite3, sql: &str) {
    unsafe {
        let csql = CString::new(sql).unwrap();
        let mut err: *mut std::os::raw::c_char = ptr::null_mut();
        let rc = sqlite3_exec(db, csql.as_ptr(), None, ptr::null_mut(), &mut err);
        if rc != SQLITE_OK {
            let msg = if err.is_null() {
                "(no message)".to_string()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            sqlite3_free(err.cast());
            panic!("exec failed (rc={rc}): {sql}: {msg}");
        }
        if !err.is_null() {
            sqlite3_free(err.cast());
        }
    }
}

unsafe fn query_count(db: *mut sqlite3, sql: &str) -> i64 {
    unsafe {
        let csql = CString::new(sql).unwrap();
        let mut stmt = ptr::null_mut();
        let rc = sqlite3_prepare_v2(db, csql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        if rc != SQLITE_OK {
            let msg = CStr::from_ptr(sqlite3_errmsg(db)).to_string_lossy();
            panic!("prepare failed (rc={rc}): {sql}: {msg}");
        }
        let step_rc = sqlite3_step(stmt);
        assert_eq!(step_rc, SQLITE_ROW, "expected a row from: {sql}");
        let v = sqlite3_column_int64(stmt, 0);
        sqlite3_finalize(stmt);
        v
    }
}

#[derive(Copy, Clone)]
enum Kind {
    Sqlitegis,
    Spatialite,
}

/// Seed N random WGS84 points into a `places(id, geom)` table and N small
/// axis-aligned polygons (0.1 deg squares centred on independently-drawn
/// coordinates) into a `regions(id, geom)` table. SpatiaLite needs its
/// geometry columns declared via `AddGeometryColumn`; sqlitegis is fine
/// with plain BLOB columns. Both DBs see identical bytes because the RNG
/// sequence is deterministic and reset to `RNG_SEED` at the top.
unsafe fn seed(db: *mut sqlite3, kind: Kind) {
    unsafe {
        match kind {
            Kind::Sqlitegis => {
                exec(
                    db,
                    "CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB)",
                );
                exec(
                    db,
                    "CREATE TABLE regions (id INTEGER PRIMARY KEY, geom BLOB)",
                );
            }
            Kind::Spatialite => {
                exec(db, "CREATE TABLE places (id INTEGER PRIMARY KEY)");
                exec(
                    db,
                    "SELECT AddGeometryColumn('places', 'geom', 4326, 'POINT', 'XY')",
                );
                exec(db, "CREATE TABLE regions (id INTEGER PRIMARY KEY)");
                exec(
                    db,
                    "SELECT AddGeometryColumn('regions', 'geom', 4326, 'POLYGON', 'XY')",
                );
            }
        }
        let mut state: u64 = RNG_SEED;
        exec(db, "BEGIN");

        // Points into `places`.
        let insert_sql =
            CString::new("INSERT INTO places(geom) VALUES (ST_GeomFromText(?, 4326))").unwrap();
        let mut stmt = ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(db, insert_sql.as_ptr(), -1, &mut stmt, ptr::null_mut()),
            SQLITE_OK,
        );
        for _ in 0..N_POINTS {
            let (x, y) = next_xy(&mut state);
            let wkt = format!("POINT({x} {y})");
            let cwkt = CString::new(wkt).unwrap();
            sqlite3_bind_text(stmt, 1, cwkt.as_ptr(), -1, SQLITE_TRANSIENT());
            let step = sqlite3_step(stmt);
            assert_eq!(step, SQLITE_DONE, "INSERT step rc {step}");
            sqlite3_reset(stmt);
        }
        sqlite3_finalize(stmt);

        // Polygons into `regions`: 0.1 deg axis-aligned squares centred on
        // continuation of the same RNG stream.
        let insert_poly_sql =
            CString::new("INSERT INTO regions(geom) VALUES (ST_GeomFromText(?, 4326))").unwrap();
        let mut pstmt = ptr::null_mut();
        assert_eq!(
            sqlite3_prepare_v2(
                db,
                insert_poly_sql.as_ptr(),
                -1,
                &mut pstmt,
                ptr::null_mut()
            ),
            SQLITE_OK,
        );
        const HALF: f64 = 0.05;
        for _ in 0..N_POINTS {
            let (cx, cy) = next_xy(&mut state);
            let (x0, y0, x1, y1) = (cx - HALF, cy - HALF, cx + HALF, cy + HALF);
            let wkt = format!("POLYGON(({x0} {y0},{x1} {y0},{x1} {y1},{x0} {y1},{x0} {y0}))");
            let cwkt = CString::new(wkt).unwrap();
            sqlite3_bind_text(pstmt, 1, cwkt.as_ptr(), -1, SQLITE_TRANSIENT());
            let step = sqlite3_step(pstmt);
            assert_eq!(step, SQLITE_DONE, "regions INSERT step rc {step}");
            sqlite3_reset(pstmt);
        }
        sqlite3_finalize(pstmt);

        exec(db, "COMMIT");
    }
}

/// Tiny LCG (Numerical Recipes constants) for deterministic point coords.
/// Plenty good for bench seeding.
fn next_xy(state: &mut u64) -> (f64, f64) {
    let x = {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((*state >> 33) as f64) / (u32::MAX as f64) * 360.0 - 180.0
    };
    let y = {
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        ((*state >> 33) as f64) / (u32::MAX as f64) * 180.0 - 90.0
    };
    (x, y)
}

// ---- benches ----

fn bench_bulk_intersects_unindexed(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let window = "POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))";
    let sql = format!(
        "SELECT COUNT(*) FROM places WHERE ST_Intersects(geom, ST_GeomFromText('{window}', 4326))"
    );

    let mut group = c.benchmark_group("Unindexed ST_Intersects bulk");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, &sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, &sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// R-tree-indexed window query: same 50k points, same constant window
/// polygon, but routed through `CreateSpatialIndex` so the planner uses
/// the rtree virtual table for the bbox prefilter and `ST_Intersects`
/// runs only on the candidates. The two libraries name their rtree
/// objects differently (`places_geom_rtree` vs `idx_places_geom`) and
/// pick the join column differently (`id` vs `pkid`), so the SQL is
/// per-library, but the data and the window are identical.
fn bench_indexed_intersects(c: &mut Criterion) {
    let db_g = unsafe {
        let db = open_sqlitegis_db();
        exec(db, "SELECT CreateSpatialIndex('places', 'geom')");
        db
    };
    let db_s = unsafe {
        let db = open_spatialite_db();
        exec(db, "SELECT CreateSpatialIndex('places', 'geom')");
        db
    };
    let window = "POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))";
    let sql_g = format!(
        "SELECT COUNT(*) FROM places p \
         JOIN places_geom_rtree r ON p.rowid = r.id \
         WHERE r.xmin <= 11 AND r.xmax >= 10 AND r.ymin <= 21 AND r.ymax >= 20 \
         AND ST_Intersects(p.geom, ST_GeomFromText('{window}', 4326))"
    );
    let sql_s = format!(
        "SELECT COUNT(*) FROM places p \
         JOIN idx_places_geom r ON p.rowid = r.pkid \
         WHERE r.xmin <= 11 AND r.xmax >= 10 AND r.ymin <= 21 AND r.ymax >= 20 \
         AND ST_Intersects(p.geom, ST_GeomFromText('{window}', 4326))"
    );

    let mut group = c.benchmark_group("Indexed ST_Intersects window");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, &sql_g)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, &sql_s)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// Geodesic (sphere-Haversine) distance throughput: count how many of
/// the 50k points lie within 1000 km of (0, 0). Pure float math, no
/// spatial filtering tricks, runs once per row. The two libraries spell
/// the same Haversine algorithm differently: sqlitegis follows PostGIS
/// with a dedicated `ST_DistanceSphere`; SpatiaLite 5.1 uses the 3-arg
/// `ST_Distance(g1, g2, use_ellipsoid)` form with `use_ellipsoid=0` for
/// the sphere variant.
fn bench_distance_sphere(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql_g = "SELECT COUNT(*) FROM places \
                 WHERE ST_DistanceSphere(geom, ST_GeomFromText('POINT(0 0)', 4326)) < 1000000.0";
    let sql_s = "SELECT COUNT(*) FROM places \
                 WHERE ST_Distance(geom, ST_GeomFromText('POINT(0 0)', 4326), 0) < 1000000.0";

    let mut group = c.benchmark_group("Geodesic distance bulk");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql_g)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql_s)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// `ST_AsText` scalar throughput: walk all 50k rows projecting
/// `ST_AsText(geom)`. `COUNT()` over the projection forces the
/// serializer to fire on every row but only carries one scalar back to
/// the bench harness. Pure EWKB-to-WKT throughput, no predicates.
fn bench_astext_throughput(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT COUNT(ST_AsText(geom)) FROM places";

    let mut group = c.benchmark_group("ST_AsText scalar throughput");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// GEOS-heavy workload: buffer a small polygon by 0.1 degrees, then
/// take its per-row intersection with the point column. The buffer step
/// is the GEOS-heavy work (we use `geo::Buffer`, SpatiaLite uses
/// `GEOSBufferWithParams`); SQLite folds it to a constant subexpression
/// so the buffer runs once per query, then `ST_Intersection(buffer, geom)`
/// produces the per-row intersected geometry, which `ST_IsEmpty` filters
/// for non-empty results. This exercises the full intersection pipeline
/// on a mixed Polygon-vs-Point input shape (now supported in sqlitegis
/// after the decompose/intersect/pack dispatch landed).
fn bench_buffer_intersection(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT COUNT(*) FROM places \
               WHERE NOT ST_IsEmpty(ST_Intersection( \
                   ST_Buffer(ST_GeomFromText('POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))', 4326), 0.1), \
                   geom \
               ))";

    let mut group = c.benchmark_group("ST_Buffer + ST_Intersection bulk");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// `ST_Contains` on a large constant LHS polygon against every point in
/// `places`. The window is wide enough that ~1% of points fall inside,
/// so the negative-row path is the hot path. Validates whether an MBR
/// fastpath on `ST_Contains` would yield the same kind of speedup we
/// got for `ST_Intersects`.
fn bench_bulk_contains_unindexed(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT COUNT(*) FROM places \
               WHERE ST_Contains(ST_GeomFromText('POLYGON((0 0, 36 0, 36 18, 0 18, 0 0))', 4326), geom)";

    let mut group = c.benchmark_group("Unindexed ST_Contains bulk");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// Indexed variant of the Contains bench. Uses the R-tree to prefilter
/// the candidate rows before evaluating `ST_Contains` per row. Mirrors
/// the existing indexed Intersects bench but with Contains semantics.
fn bench_indexed_contains(c: &mut Criterion) {
    let db_g = unsafe {
        let db = open_sqlitegis_db();
        exec(db, "SELECT CreateSpatialIndex('places', 'geom')");
        db
    };
    let db_s = unsafe {
        let db = open_spatialite_db();
        exec(db, "SELECT CreateSpatialIndex('places', 'geom')");
        db
    };
    let sql_g = "SELECT COUNT(*) FROM places p \
                 JOIN places_geom_rtree r ON p.rowid = r.id \
                 WHERE r.xmin >= 0 AND r.xmax <= 36 AND r.ymin >= 0 AND r.ymax <= 18 \
                 AND ST_Contains(ST_GeomFromText('POLYGON((0 0, 36 0, 36 18, 0 18, 0 0))', 4326), p.geom)";
    let sql_s = "SELECT COUNT(*) FROM places p \
                 JOIN idx_places_geom r ON p.rowid = r.pkid \
                 WHERE r.xmin >= 0 AND r.xmax <= 36 AND r.ymin >= 0 AND r.ymax <= 18 \
                 AND ST_Contains(ST_GeomFromText('POLYGON((0 0, 36 0, 36 18, 0 18, 0 0))', 4326), p.geom)";

    let mut group = c.benchmark_group("Indexed ST_Contains window");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql_g)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql_s)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// Isolated `ST_Buffer` throughput on point inputs. The buffer distance
/// is constant; the input geometry varies per row. We consume the result
/// via `SUM(ST_Area(...))` so SQLite does not skip the call. This isolates
/// the per-row buffer cost from the surrounding intersection logic that
/// the existing `ST_Buffer + ST_Intersection` bench fuses together.
fn bench_buffer_throughput(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT SUM(ST_Area(ST_Buffer(geom, 0.01))) FROM places";

    let mut group = c.benchmark_group("ST_Buffer scalar throughput");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// `ST_Centroid` scalar throughput on polygon inputs. Pure unary work
/// per row, no MBR shortcut possible. Measures the per-callback overhead
/// (EWKB decode + centroid algo + EWKB write) on a representative
/// polygon workload.
fn bench_centroid_throughput(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT COUNT(ST_Centroid(geom)) FROM regions";

    let mut group = c.benchmark_group("ST_Centroid scalar throughput");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

/// `ST_Difference` against a constant far-away polygon: the LHS region
/// and the RHS window are always MBR-disjoint, so the geometrically
/// correct answer is "LHS unchanged" for every row. This isolates the
/// disjoint-MBR fastpath opportunity for set operations (sqlitegis does
/// not have one today; SpatiaLite/GEOS may or may not short-circuit).
fn bench_difference_disjoint(c: &mut Criterion) {
    let db_g = unsafe { open_sqlitegis_db() };
    let db_s = unsafe { open_spatialite_db() };
    let sql = "SELECT COUNT(*) FROM regions \
               WHERE NOT ST_IsEmpty(ST_Difference( \
                   geom, \
                   ST_GeomFromText('POLYGON((-179.9 -89.9, -179 -89.9, -179 -89, -179.9 -89, -179.9 -89.9))', 4326) \
               ))";

    let mut group = c.benchmark_group("ST_Difference disjoint bulk");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(query_count(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

criterion_group!(
    benches,
    bench_bulk_intersects_unindexed,
    bench_indexed_intersects,
    bench_distance_sphere,
    bench_astext_throughput,
    bench_buffer_intersection,
    bench_bulk_contains_unindexed,
    bench_indexed_contains,
    bench_buffer_throughput,
    bench_centroid_throughput,
    bench_difference_disjoint,
);
criterion_main!(benches);
