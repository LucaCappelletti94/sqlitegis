#![cfg(not(target_arch = "wasm32"))]
// Helpers below (sqlite_errmsg, discover_extension_artifact, etc.) are
// only consumed by the `sqlite-extension`-gated load_extension test. Allow
// dead_code and unused imports at module scope so the file still builds
// cleanly when that feature is off.
#![allow(dead_code, unused_imports)]
//! Integration tests for the SQLite extension.

use libsqlite3_sys::*;
use std::ffi::{CStr, CString};
use std::path::PathBuf;
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
    let workspace_root = manifest_dir
        .parent()
        .expect("workspace root parent should exist");
    let mut roots = Vec::new();

    if let Ok(raw_target_dir) = std::env::var("CARGO_TARGET_DIR") {
        let target_dir = PathBuf::from(&raw_target_dir);
        if target_dir.is_absolute() {
            roots.push(target_dir);
        } else {
            if let Ok(cwd) = std::env::current_dir() {
                roots.push(cwd.join(&target_dir));
            }
            roots.push(workspace_root.join(&target_dir));
            roots.push(manifest_dir.join(&target_dir));
        }
    }

    roots.push(workspace_root.join("target"));
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
