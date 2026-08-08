#![cfg(not(target_arch = "wasm32"))]
// Helpers below (sqlite_errmsg, discover_extension_artifact, etc.) are
// only consumed by the `sqlite-extension`-gated load_extension test. Allow
// dead_code and unused imports at module scope so the file still builds
// cleanly when that feature is off.
#![allow(dead_code, unused_imports)]
//! Integration tests for the SQLite extension.

use libsqlite3_sys::*;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use std::ptr;

include!("sqlite_test_db_macro.rs");
define_test_db!(TestDb);
type ActiveTestDb = TestDb;

include!("support/shared_cases.rs");
define_shared_cases!(test);

#[cfg(feature = "sqlite-extension")]
#[test]
fn sqlite_runtime_load_extension_registers_spatial_functions() {
    let mut db = ptr::null_mut();
    let mem = CString::new(":memory:").expect("valid sqlite memory path");
    unsafe {
        assert_eq!(
            SQLITE_OK,
            sqlite3_open(mem.as_ptr(), &mut db),
            "sqlite3_open failed"
        );
    }

    struct DbGuard(*mut sqlite3);
    impl Drop for DbGuard {
        fn drop(&mut self) {
            unsafe {
                sqlite3_close(self.0);
            }
        }
    }
    let _db_guard = DbGuard(db);

    let ext_path = discover_extension_artifact();
    let ext_path_str = ext_path.to_string_lossy().into_owned();
    let ext_path_c =
        CString::new(ext_path_str.clone()).expect("extension path must not contain NUL");

    unsafe {
        let enable_rc = sqlite3_enable_load_extension(db, 1);
        assert_eq!(
            enable_rc,
            SQLITE_OK,
            "sqlite3_enable_load_extension(1) failed for {}: {}",
            ext_path.display(),
            sqlite_errmsg(db)
        );

        let mut load_err: *mut std::os::raw::c_char = ptr::null_mut();
        let load_rc = sqlite3_load_extension(db, ext_path_c.as_ptr(), ptr::null(), &mut load_err);
        let load_err_msg = take_sqlite_error_message(load_err);
        assert_eq!(
            load_rc,
            SQLITE_OK,
            "sqlite3_load_extension failed for {}: db_error={}, load_error={}",
            ext_path.display(),
            sqlite_errmsg(db),
            load_err_msg.unwrap_or_else(|| "<none>".to_string())
        );

        let disable_rc = sqlite3_enable_load_extension(db, 0);
        assert_eq!(
            disable_rc,
            SQLITE_OK,
            "sqlite3_enable_load_extension(0) failed: {}",
            sqlite_errmsg(db)
        );

        let sql = CString::new("SELECT ST_AsText(ST_GeomFromText('POINT(1 2)'))")
            .expect("valid SQL for extension smoke test");
        let mut stmt = ptr::null_mut();
        let prepare_rc = sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, ptr::null_mut());
        assert_eq!(
            prepare_rc,
            SQLITE_OK,
            "prepare failed after extension load: {}",
            sqlite_errmsg(db)
        );
        let step_rc = sqlite3_step(stmt);
        assert_eq!(step_rc, SQLITE_ROW, "step failed: {}", sqlite_errmsg(db));
        let out_ptr = sqlite3_column_text(stmt, 0);
        assert!(!out_ptr.is_null(), "ST_AsText returned NULL unexpectedly");
        let out = CStr::from_ptr(out_ptr as *const std::os::raw::c_char)
            .to_string_lossy()
            .into_owned();
        sqlite3_finalize(stmt);
        assert_eq!(out, "POINT(1 2)");

        // Everything above went through four of the routines `host_api`
        // dispatches. The rest have to be exercised through the loaded
        // extension too: a wrong offset in that table lands on some other
        // SQLite routine, and the only place that shows up is the call site.
        assert_eq!(
            ext_scalar(
                db,
                "SELECT ST_Area(ST_GeomFromText('POLYGON((0 0,1 0,1 1,0 1,0 0))'))"
            ),
            Ok(Some("1.0".to_string())),
            "result_double"
        );
        assert_eq!(
            ext_scalar(db, "SELECT ST_SRID(ST_SetSRID(ST_Point(1.5, 2.5), 4326))"),
            Ok(Some("4326".to_string())),
            "result_int, value_double and value_int64"
        );
        assert_eq!(
            ext_scalar(
                db,
                "SELECT ST_NPoints(ST_GeomFromText('LINESTRING(0 0,1 1,2 2)'))"
            ),
            Ok(Some("3".to_string())),
            "result_int64"
        );
        assert_eq!(
            ext_scalar(db, "SELECT ST_XMin(ST_GeomFromText('POINT EMPTY'))"),
            Ok(None),
            "result_null"
        );
        match ext_scalar(
            db,
            "SELECT ST_LengthSpheroid(ST_GeomFromText('LINESTRING(0 0,1 1)'))",
        ) {
            Err(msg) => assert!(msg.contains("requires SRID 4326"), "result_error: {msg}"),
            other => panic!("expected an error through the extension, got {other:?}"),
        }

        // CreateSpatialIndex runs statements on the host connection, which
        // covers context_db_handle, exec, prepare_v2, step, column_type,
        // column_text, finalize, errmsg and free in one go.
        assert_eq!(
            ext_scalar(
                db,
                "CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB)"
            ),
            Ok(None)
        );
        assert_eq!(
            ext_scalar(db, "SELECT CreateSpatialIndex('places', 'geom')"),
            Ok(Some("1".to_string())),
            "statement-execution routines"
        );
        assert_eq!(
            ext_scalar(db, "SELECT DropSpatialIndex('places', 'geom')"),
            Ok(Some("1".to_string())),
            "statement-execution routines"
        );
    }
}

/// Run `sql` on `db` and return the first column of the first row as text,
/// `None` for SQL NULL or no rows, or the SQLite error message.
///
/// # Safety
///
/// `db` must be an open connection.
unsafe fn ext_scalar(db: *mut sqlite3, sql: &str) -> Result<Option<String>, String> {
    unsafe {
        let sql_c = CString::new(sql).expect("test SQL must not contain NUL");
        let mut stmt = ptr::null_mut();
        if sqlite3_prepare_v2(db, sql_c.as_ptr(), -1, &mut stmt, ptr::null_mut()) != SQLITE_OK {
            return Err(sqlite_errmsg(db));
        }
        let rc = sqlite3_step(stmt);
        let out = if rc == SQLITE_ROW {
            let ptr = sqlite3_column_text(stmt, 0);
            if ptr.is_null() {
                Ok(None)
            } else {
                Ok(Some(
                    CStr::from_ptr(ptr as *const std::os::raw::c_char)
                        .to_string_lossy()
                        .into_owned(),
                ))
            }
        } else if rc == SQLITE_DONE {
            Ok(None)
        } else {
            Err(sqlite_errmsg(db))
        };
        sqlite3_finalize(stmt);
        out
    }
}

fn sqlite_errmsg(db: *mut sqlite3) -> String {
    unsafe {
        CStr::from_ptr(sqlite3_errmsg(db))
            .to_string_lossy()
            .into_owned()
    }
}

fn take_sqlite_error_message(err: *mut std::os::raw::c_char) -> Option<String> {
    if err.is_null() {
        return None;
    }
    let msg = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        sqlite3_free(err as *mut std::os::raw::c_void);
    }
    Some(msg)
}

fn discover_extension_artifact() -> PathBuf {
    let mut attempted = Vec::new();
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".to_string());
    for target_root in candidate_target_roots() {
        let profile_dir = target_root.join(&profile);
        let deps_dir = profile_dir.join("deps");
        for dir in [&profile_dir, &deps_dir] {
            for lib_name in candidate_library_names() {
                let candidate = dir.join(lib_name);
                attempted.push(candidate.clone());
                if candidate.is_file() {
                    return canonical_or_original(candidate);
                }
            }
        }
    }
    panic!(
        "unable to locate SQLiteGIS sqlite shared library. looked in:\n{}",
        attempted
            .iter()
            .map(|p| format!("  - {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn candidate_target_roots() -> Vec<PathBuf> {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // Probe both the package directory and its parent. After the crate was
    // flattened to the repo root the package dir IS the workspace root, so
    // `target/` lives next to Cargo.toml. Pre-flatten layouts (or any future
    // workspace nesting) need the parent fallback.
    let parent_dir = manifest_dir.parent().map(Path::to_path_buf);
    let mut roots = Vec::new();

    if let Ok(raw_target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let target_dir = PathBuf::from(&raw_target_dir);
        if target_dir.is_absolute() {
            roots.push(target_dir);
        } else {
            if let Ok(cwd) = std::env::current_dir() {
                roots.push(cwd.join(&target_dir));
            }
            roots.push(manifest_dir.join(&target_dir));
            if let Some(parent) = &parent_dir {
                roots.push(parent.join(&target_dir));
            }
        }
    }

    roots.push(manifest_dir.join("target"));
    if let Some(parent) = &parent_dir {
        roots.push(parent.join("target"));
    }
    dedup_paths(roots)
}

fn dedup_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for path in paths {
        if !out.iter().any(|seen| seen == &path) {
            out.push(path);
        }
    }
    out
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn candidate_library_names() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["sqlitegis.dll"]
    }
    #[cfg(target_os = "macos")]
    {
        &["libsqlitegis.dylib"]
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        &["libsqlitegis.so"]
    }
}
