//! Head-to-head benchmark: sqlitegis vs SpatiaLite on identical workloads.
//!
//! Both extensions are loaded into separate in-memory SQLite connections
//! via `sqlite3_load_extension` so the comparison reflects the runtime
//! cost a real user would pay (api-routines indirection on both sides).
//!
//! Run with:
//!
//! ```sh
//! cargo build --release --features sqlite-extension,bundled-sqlite
//! cargo bench --features bench-spatialite spatialite_vs_sqlitegis
//! ```
//!
//! The first command produces the sqlitegis cdylib that the second
//! command then `load_extension`s alongside `mod_spatialite`.

#![cfg(all(feature = "bench-spatialite", not(target_arch = "wasm32")))]

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libsqlite3_sys::*;
use std::ffi::{CStr, CString};
use std::hint::black_box;
use std::ptr;

// Path to the sqlitegis cdylib produced by `cargo build --release
// --features sqlite-extension,bundled-sqlite`. Resolved relative to the
// crate root at compile time.
const SQLITEGIS_CDYLIB: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/release/libsqlitegis");
// SpatiaLite is loaded by its standard library name; the SQLite loader
// searches the platform's library path (LD_LIBRARY_PATH on Linux).
const SPATIALITE_LIB: &str = "mod_spatialite";

// Dataset size. Large enough for the unindexed scan to be measurable,
// small enough that the bench finishes in well under a minute.
const N_POINTS: usize = 50_000;

// PRNG seed so every bench iteration sees the same dataset.
const RNG_SEED: u64 = 0xC0FFEE;

unsafe fn open_with_extension(path: &str, post_load_sql: &[&str]) -> *mut sqlite3 {
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
        let cpath = CString::new(path).unwrap();
        let mut err: *mut std::os::raw::c_char = ptr::null_mut();
        let rc = sqlite3_load_extension(db, cpath.as_ptr(), ptr::null(), &mut err);
        if rc != SQLITE_OK {
            let msg = if err.is_null() {
                "(no message)".to_string()
            } else {
                CStr::from_ptr(err).to_string_lossy().into_owned()
            };
            panic!("load_extension({path}) failed: rc={rc} err={msg}");
        }
        for sql in post_load_sql {
            exec(db, sql);
        }
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

/// Returns the integer of the first column of the first row of the query.
/// Used for COUNT(*) workloads so the bench result depends on real data.
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

/// Walk all rows, applying ST_AsText to every geom. Returns the number of
/// rows visited so a bench iteration's work is observable.
unsafe fn drive_scalar(db: *mut sqlite3, sql: &str) -> i64 {
    unsafe {
        let csql = CString::new(sql).unwrap();
        let mut stmt = ptr::null_mut();
        let rc = sqlite3_prepare_v2(db, csql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        if rc != SQLITE_OK {
            let msg = CStr::from_ptr(sqlite3_errmsg(db)).to_string_lossy();
            panic!("prepare failed (rc={rc}): {sql}: {msg}");
        }
        let mut n = 0i64;
        loop {
            let step = sqlite3_step(stmt);
            if step == SQLITE_DONE {
                break;
            }
            assert_eq!(step, SQLITE_ROW, "unexpected step rc {step} for: {sql}");
            // Pull column 0 to force the function to run and the result
            // bytes to be materialised; sqlite is lazy otherwise.
            let _ = sqlite3_column_bytes(stmt, 0);
            let _ = sqlite3_column_text(stmt, 0);
            n += 1;
        }
        sqlite3_finalize(stmt);
        n
    }
}

/// Deterministically seed N random WGS84 points using a simple LCG so
/// both connections receive byte-identical input. Returns the rtree-name
/// to use for indexed queries on this connection (differs between the
/// two libraries).
struct Seeded {
    /// SQL fragment that joins the spatial index for the indexed-window
    /// bench. Differs per library because each names the rtree differently:
    /// SpatiaLite -> `idx_<table>_<column>`, sqlitegis -> `<table>_<column>_rtree`.
    indexed_join_clause: &'static str,
}

unsafe fn seed(db: *mut sqlite3, kind: Kind) -> Seeded {
    unsafe {
        match kind {
            Kind::Sqlitegis => {
                // sqlitegis stores SRID and type in the EWKB blob itself, so
                // a plain BLOB column is all the schema needs.
                exec(
                    db,
                    "CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB)",
                );
            }
            Kind::Spatialite => {
                // SpatiaLite tracks geometry columns in `geometry_columns`
                // metadata. `AddGeometryColumn` ADDS the column, so we
                // must NOT pre-declare it. Skipping this step would make
                // `ST_Intersects` short-circuit to NULL on every row (the
                // column would not be a typed geometry), producing a fake
                // win for SpatiaLite on the unindexed bench.
                exec(db, "CREATE TABLE places (id INTEGER PRIMARY KEY)");
                exec(
                    db,
                    "SELECT AddGeometryColumn('places', 'geom', 4326, 'POINT', 'XY')",
                );
            }
        }
        let mut state: u64 = RNG_SEED;
        exec(db, "BEGIN");
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
        exec(db, "COMMIT");

        // Build the spatial index. CreateSpatialIndex is the same
        // function name in both libraries; only the resulting rtree
        // table name differs.
        exec(db, "SELECT CreateSpatialIndex('places', 'geom')");

        Seeded {
            indexed_join_clause: match kind {
                Kind::Sqlitegis => "places p JOIN places_geom_rtree r ON p.rowid = r.id",
                Kind::Spatialite => "places p JOIN idx_places_geom r ON p.rowid = r.pkid",
            },
        }
    }
}

fn next_xy(state: &mut u64) -> (f64, f64) {
    // Tiny LCG (Numerical Recipes constants). Plenty for benchmark seeding.
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

#[derive(Copy, Clone)]
enum Kind {
    Sqlitegis,
    Spatialite,
}

fn open_sqlitegis() -> (*mut sqlite3, Seeded) {
    let db = unsafe { open_with_extension(SQLITEGIS_CDYLIB, &[]) };
    let s = unsafe { seed(db, Kind::Sqlitegis) };
    (db, s)
}

fn open_spatialite() -> (*mut sqlite3, Seeded) {
    let db = unsafe { open_with_extension(SPATIALITE_LIB, &["SELECT InitSpatialMetaData(1)"]) };
    let s = unsafe { seed(db, Kind::Spatialite) };
    (db, s)
}

// ---- benches ----

fn bench_scalar_astext(c: &mut Criterion) {
    let (db_g, _) = open_sqlitegis();
    let (db_s, _) = open_spatialite();
    let sql = "SELECT ST_AsText(geom) FROM places";

    let mut group = c.benchmark_group("ST_AsText scalar");
    group.throughput(Throughput::Elements(N_POINTS as u64));
    group.bench_function(BenchmarkId::new("sqlitegis", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(drive_scalar(db_g, sql)) })
    });
    group.bench_function(BenchmarkId::new("spatialite", N_POINTS), |b| {
        b.iter(|| unsafe { black_box(drive_scalar(db_s, sql)) })
    });
    group.finish();

    unsafe {
        sqlite3_close(db_g);
        sqlite3_close(db_s);
    }
}

fn bench_bulk_intersects_unindexed(c: &mut Criterion) {
    let (db_g, _) = open_sqlitegis();
    let (db_s, _) = open_spatialite();
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

fn bench_indexed_intersects(c: &mut Criterion) {
    let (db_g, s_g) = open_sqlitegis();
    let (db_s, s_s) = open_spatialite();
    let window = "POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))";
    let make_sql = |joinc: &str| {
        format!(
            "SELECT COUNT(*) FROM {joinc} \
             WHERE r.xmin <= 11 AND r.xmax >= 10 AND r.ymin <= 21 AND r.ymax >= 20 \
             AND ST_Intersects(p.geom, ST_GeomFromText('{window}', 4326))"
        )
    };
    let sql_g = make_sql(s_g.indexed_join_clause);
    let sql_s = make_sql(s_s.indexed_join_clause);

    let mut group = c.benchmark_group("R-tree-indexed ST_Intersects");
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

criterion_group!(
    benches,
    bench_scalar_astext,
    bench_bulk_intersects_unindexed,
    bench_indexed_intersects
);
criterion_main!(benches);
