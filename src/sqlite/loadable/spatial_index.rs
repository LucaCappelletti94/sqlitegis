//! `CreateSpatialIndex` and `DropSpatialIndex` DDL callbacks.
//!
//! These mirror the in-process versions in `crate::sqlite::ffi` line-for-line
//! in terms of SQL emitted and ownership/catalog invariants enforced. Two
//! implementations exist because the FFI path differs: `ffi.rs` talks to the
//! linked libsqlite3 directly, while this module routes every SQLite call
//! through the host's `sqlite3_api_routines` table (via the `sqlite3ext_*`
//! indirection in `sqlite-loadable`) so the produced cdylib has no link-time
//! dependency on a specific libsqlite3.

use sqlite_loadable::ext::{
    sqlite3ext_column_bytes, sqlite3ext_column_text, sqlite3ext_context_db_handle,
    sqlite3ext_finalize, sqlite3ext_prepare_v2, sqlite3ext_step,
};
use sqlite_loadable::prelude::*;
use sqlite_loadable::{api, errors::Error, Result};
use std::ffi::CString;

use super::args::arg_text_or_null;

const SPATIAL_INDEX_CATALOG_TABLE: &str = "sqlitegis_spatial_index_catalog";
const SPATIAL_INDEX_CATALOG_REQUIRED_COLUMNS: [&str; 3] = ["prefix", "table_name", "column_name"];

const SQLITE_OK: i32 = 0;
const SQLITE_ROW: i32 = 100;
const SQLITE_DONE: i32 = 101;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpatialIndexOwnership {
    Owned,
    Absent,
}

fn validate_identifier(s: &str) -> Option<&str> {
    if s.is_empty() {
        return None;
    }
    if s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        Some(s)
    } else {
        None
    }
}

unsafe fn sl_prepare(
    db: *mut sqlite3,
    sql: &str,
) -> Result<*mut sqlite_loadable::ext::sqlite3_stmt> {
    let c_sql = CString::new(sql)
        .map_err(|_| Error::new_message("internal error: generated SQL contains NUL byte"))?;
    let mut stmt = std::ptr::null_mut();
    let rc = sqlite3ext_prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
    if rc != SQLITE_OK {
        return Err(Error::new_message(format!(
            "sqlite3_prepare_v2 failed (rc={rc}) for: {sql}"
        )));
    }
    Ok(stmt)
}

/// Run a single-statement SQL (DDL or DML) once and finalise. The caller is
/// expected to pass exactly one statement, terminated or not, with no
/// trailing SQL after the first semicolon: this mirrors how
/// `crate::sqlite::ffi::exec_sql` is invoked.
unsafe fn sl_exec(db: *mut sqlite3, sql: &str) -> Result<()> {
    let stmt = sl_prepare(db, sql)?;
    let rc = sqlite3ext_step(stmt);
    let _ = sqlite3ext_finalize(stmt);
    if rc == SQLITE_DONE || rc == SQLITE_ROW {
        Ok(())
    } else {
        Err(Error::new_message(format!(
            "sqlite3_step failed (rc={rc}) for: {sql}"
        )))
    }
}

/// Best-effort exec for rollback paths: swallows every failure.
unsafe fn sl_exec_silent(db: *mut sqlite3, sql: &str) {
    if let Ok(stmt) = sl_prepare(db, sql) {
        let _ = sqlite3ext_step(stmt);
        let _ = sqlite3ext_finalize(stmt);
    }
}

unsafe fn sl_rollback_savepoint(db: *mut sqlite3, savepoint: &str) {
    sl_exec_silent(db, &format!("ROLLBACK TO {savepoint}"));
    sl_exec_silent(db, &format!("RELEASE {savepoint}"));
}

/// Run a query that returns at most one row with a single TEXT column.
unsafe fn sl_lookup_text(db: *mut sqlite3, sql: &str) -> Result<Option<String>> {
    let stmt = sl_prepare(db, sql)?;
    let rc = sqlite3ext_step(stmt);
    let result = if rc == SQLITE_ROW {
        let ptr = sqlite3ext_column_text(stmt, 0);
        if ptr.is_null() {
            None
        } else {
            let n = sqlite3ext_column_bytes(stmt, 0) as usize;
            Some(String::from_utf8_lossy(std::slice::from_raw_parts(ptr, n)).into_owned())
        }
    } else if rc != SQLITE_DONE {
        let _ = sqlite3ext_finalize(stmt);
        return Err(Error::new_message(format!(
            "sqlite3_step failed (rc={rc}) for: {sql}"
        )));
    } else {
        None
    };
    let _ = sqlite3ext_finalize(stmt);
    Ok(result)
}

unsafe fn sl_master_object_type(db: *mut sqlite3, name: &str) -> Result<Option<String>> {
    let sql = format!("SELECT type FROM sqlite_master WHERE name = '{name}' LIMIT 1");
    sl_lookup_text(db, &sql)
}

unsafe fn sl_inspect_catalog_columns(db: *mut sqlite3) -> Result<(bool, bool, bool)> {
    let sql = format!("PRAGMA table_info([{SPATIAL_INDEX_CATALOG_TABLE}])");
    let stmt = sl_prepare(db, &sql)?;
    let mut has_prefix = false;
    let mut has_table_name = false;
    let mut has_column_name = false;
    loop {
        let rc = sqlite3ext_step(stmt);
        if rc == SQLITE_DONE {
            break;
        }
        if rc != SQLITE_ROW {
            let _ = sqlite3ext_finalize(stmt);
            return Err(Error::new_message(format!(
                "PRAGMA table_info failed (rc={rc})"
            )));
        }
        // Column index 1 in PRAGMA table_info is the column name.
        let ptr = sqlite3ext_column_text(stmt, 1);
        if !ptr.is_null() {
            let n = sqlite3ext_column_bytes(stmt, 1) as usize;
            let name = std::str::from_utf8(std::slice::from_raw_parts(ptr, n)).unwrap_or("");
            match name {
                "prefix" => has_prefix = true,
                "table_name" => has_table_name = true,
                "column_name" => has_column_name = true,
                _ => {}
            }
        }
    }
    let _ = sqlite3ext_finalize(stmt);
    Ok((has_prefix, has_table_name, has_column_name))
}

unsafe fn sl_validate_catalog_shape(db: *mut sqlite3, label: &str) -> Result<()> {
    let object_type = sl_master_object_type(db, SPATIAL_INDEX_CATALOG_TABLE).map_err(|e| {
        Error::new_message(format!(
            "{label}: failed to inspect spatial index catalog metadata: {e}"
        ))
    })?;
    let Some(object_type) = object_type else {
        return Err(Error::new_message(format!(
            "{label}: failed to inspect spatial index catalog metadata: \
             missing sqlite_master entry for [{SPATIAL_INDEX_CATALOG_TABLE}]"
        )));
    };
    if object_type != "table" {
        return Err(Error::new_message(format!(
            "{label}: invalid spatial index catalog object type for \
             [{SPATIAL_INDEX_CATALOG_TABLE}] (expected table, found [{object_type}])"
        )));
    }
    let (has_prefix, has_table_name, has_column_name) =
        sl_inspect_catalog_columns(db).map_err(|e| {
            Error::new_message(format!(
                "{label}: failed to inspect spatial index catalog metadata: {e}"
            ))
        })?;
    let present = [has_prefix, has_table_name, has_column_name];
    for (i, required_column) in SPATIAL_INDEX_CATALOG_REQUIRED_COLUMNS.iter().enumerate() {
        if !present[i] {
            return Err(Error::new_message(format!(
                "{label}: invalid spatial index catalog schema for \
                 [{SPATIAL_INDEX_CATALOG_TABLE}] (missing required column [{required_column}])"
            )));
        }
    }
    Ok(())
}

unsafe fn sl_ensure_catalog_table(db: *mut sqlite3, label: &str) -> Result<()> {
    let object_type = sl_master_object_type(db, SPATIAL_INDEX_CATALOG_TABLE).map_err(|e| {
        Error::new_message(format!(
            "{label}: failed to inspect spatial index catalog metadata: {e}"
        ))
    })?;
    if let Some(ref object_type) = object_type {
        if object_type != "table" {
            return Err(Error::new_message(format!(
                "{label}: invalid spatial index catalog object type for \
                 [{SPATIAL_INDEX_CATALOG_TABLE}] (expected table, found [{object_type}])"
            )));
        }
    }
    let sql = format!(
        "CREATE TABLE IF NOT EXISTS [{SPATIAL_INDEX_CATALOG_TABLE}] (\
         prefix TEXT PRIMARY KEY, \
         table_name TEXT NOT NULL, \
         column_name TEXT NOT NULL, \
         UNIQUE(table_name, column_name)\
         )"
    );
    sl_exec(db, &sql).map_err(|e| {
        Error::new_message(format!(
            "{label}: failed to ensure spatial index catalog: {e}"
        ))
    })
}

unsafe fn sl_lookup_catalog_owner(
    db: *mut sqlite3,
    prefix: &str,
) -> Result<Option<(String, String)>> {
    let sql = format!(
        "SELECT table_name FROM [{SPATIAL_INDEX_CATALOG_TABLE}] \
         WHERE prefix = '{prefix}' LIMIT 1"
    );
    let Some(owner_table) = sl_lookup_text(db, &sql)? else {
        return Ok(None);
    };
    let sql = format!(
        "SELECT column_name FROM [{SPATIAL_INDEX_CATALOG_TABLE}] \
         WHERE prefix = '{prefix}' LIMIT 1"
    );
    let Some(owner_column) = sl_lookup_text(db, &sql)? else {
        return Err(Error::new_message(format!(
            "internal error: catalog row for prefix [{prefix}] is missing column_name"
        )));
    };
    Ok(Some((owner_table, owner_column)))
}

unsafe fn sl_managed_objects_exist(db: *mut sqlite3, prefix: &str) -> Result<bool> {
    let rtree_name = format!("{prefix}_rtree");
    let sql = format!(
        "SELECT name FROM sqlite_master WHERE name IN (\
         '{rtree_name}', \
         '{rtree_name}_node', \
         '{rtree_name}_parent', \
         '{rtree_name}_rowid', \
         '{prefix}_insert', \
         '{prefix}_update', \
         '{prefix}_delete'\
         ) LIMIT 1"
    );
    Ok(sl_lookup_text(db, &sql)?.is_some())
}

unsafe fn sl_ensure_table_shape(db: *mut sqlite3, prefix: &str, label: &str) -> Result<()> {
    let rtree_name = format!("{prefix}_rtree");
    let sql = format!("SELECT type FROM sqlite_master WHERE name = '{rtree_name}' LIMIT 1");
    let object_type = sl_lookup_text(db, &sql).map_err(|e| {
        Error::new_message(format!("{label}: failed to inspect sqlite_master: {e}"))
    })?;
    let Some(object_type) = object_type else {
        return Ok(());
    };
    if object_type != "table" {
        return Err(Error::new_message(format!(
            "{label}: unexpected sqlite_master entry for [{rtree_name}] \
             (type [{object_type}]); expected table"
        )));
    }
    for shadow_suffix in &["_node", "_parent", "_rowid"] {
        let shadow_name = format!("{rtree_name}{shadow_suffix}");
        let sql = format!(
            "SELECT name FROM sqlite_master \
             WHERE type = 'table' AND name = '{shadow_name}' LIMIT 1"
        );
        let exists = sl_lookup_text(db, &sql).map_err(|e| {
            Error::new_message(format!("{label}: failed to inspect sqlite_master: {e}"))
        })?;
        if exists.is_none() {
            return Err(Error::new_message(format!(
                "{label}: existing table [{rtree_name}] is not an R-tree index \
                 managed by SQLiteGIS (missing shadow table [{shadow_name}])"
            )));
        }
    }
    Ok(())
}

unsafe fn sl_ensure_objects_owned_by_table(
    db: *mut sqlite3,
    table: &str,
    column: &str,
    label: &str,
) -> Result<SpatialIndexOwnership> {
    let prefix = format!("{table}_{column}");
    for suffix in &["_insert", "_update", "_delete"] {
        let trigger_name = format!("{prefix}{suffix}");
        let sql = format!(
            "SELECT tbl_name FROM sqlite_master \
             WHERE type = 'trigger' AND name = '{trigger_name}' LIMIT 1"
        );
        let owner = sl_lookup_text(db, &sql).map_err(|e| {
            Error::new_message(format!("{label}: failed to inspect sqlite_master: {e}"))
        })?;
        if let Some(owner) = owner {
            if owner != table {
                return Err(Error::new_message(format!(
                    "{label}: naming collision for trigger [{trigger_name}] \
                     between tables [{owner}] and [{table}]"
                )));
            }
        }
    }
    sl_ensure_table_shape(db, &prefix, label)?;
    let owner = sl_lookup_catalog_owner(db, &prefix)
        .map_err(|e| Error::new_message(format!("{label}: failed to inspect catalog: {e}")))?;
    if let Some((owner_table, owner_column)) = owner {
        if owner_table == table && owner_column == column {
            return Ok(SpatialIndexOwnership::Owned);
        }
        return Err(Error::new_message(format!(
            "{label}: naming collision for managed prefix [{prefix}] \
             between [{owner_table}.{owner_column}] and [{table}.{column}]"
        )));
    }
    let objects_exist = sl_managed_objects_exist(db, &prefix).map_err(|e| {
        Error::new_message(format!("{label}: failed to inspect sqlite_master: {e}"))
    })?;
    if objects_exist {
        return Err(Error::new_message(format!(
            "{label}: cannot prove ownership for [{prefix}] because managed objects exist \
             without an ownership marker"
        )));
    }
    Ok(SpatialIndexOwnership::Absent)
}

fn extract_table_column<'a>(
    values: &'a [*mut sqlite3_value],
    label: &str,
) -> Result<(&'a str, &'a str)> {
    let table = match arg_text_or_null(values, 0) {
        Some(Ok(v)) => v,
        Some(Err(_)) => {
            return Err(Error::new_message(format!(
                "{label}: table name must be valid UTF-8 text"
            )));
        }
        None => {
            return Err(Error::new_message(format!(
                "{label}: table name must not be NULL"
            )));
        }
    };
    let column = match arg_text_or_null(values, 1) {
        Some(Ok(v)) => v,
        Some(Err(_)) => {
            return Err(Error::new_message(format!(
                "{label}: column name must be valid UTF-8 text"
            )));
        }
        None => {
            return Err(Error::new_message(format!(
                "{label}: column name must not be NULL"
            )));
        }
    };
    let Some(table) = validate_identifier(table) else {
        return Err(Error::new_message(format!(
            "{label}: invalid table name (only [a-zA-Z0-9_] allowed)"
        )));
    };
    let Some(column) = validate_identifier(column) else {
        return Err(Error::new_message(format!(
            "{label}: invalid column name (only [a-zA-Z0-9_] allowed)"
        )));
    };
    Ok((table, column))
}

pub(super) fn create_spatial_index_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let (table, column) = extract_table_column(values, "CreateSpatialIndex")?;
    let db = unsafe { sqlite3ext_context_db_handle(ctx) };
    let prefix = format!("{table}_{column}");
    let rtree = format!("{prefix}_rtree");
    let savepoint = "sqlitegis_create_spatial_index";

    unsafe {
        sl_exec(db, &format!("SAVEPOINT {savepoint}"))?;
    }

    let run = || -> Result<()> {
        unsafe {
            sl_ensure_catalog_table(db, "CreateSpatialIndex")?;
            sl_validate_catalog_shape(db, "CreateSpatialIndex")?;
            sl_ensure_objects_owned_by_table(db, table, column, "CreateSpatialIndex")?;
        }

        // Reject WITHOUT ROWID tables before creating any state. The
        // maintenance triggers reference NEW.rowid and OLD.rowid, which only
        // exist on regular rowid tables. SQLite refuses to prepare a SELECT
        // of rowid against a WITHOUT ROWID table, so the probe fails cleanly
        // at parse time. A successful probe also incidentally proves the
        // table exists.
        let probe = format!("SELECT rowid FROM [{table}] LIMIT 0");
        if unsafe { sl_exec(db, &probe) }.is_err() {
            return Err(Error::new_message(format!(
                "CreateSpatialIndex: table [{table}] has no rowid column. \
                 WITHOUT ROWID tables are not supported. Recreate the table \
                 without the WITHOUT ROWID clause, or verify the table exists."
            )));
        }

        unsafe {
            sl_exec(
                db,
                &format!(
                    "CREATE VIRTUAL TABLE IF NOT EXISTS [{rtree}] \
                     USING rtree(id, xmin, xmax, ymin, ymax)"
                ),
            )?;
            sl_exec(db, &format!("DELETE FROM [{rtree}]"))?;
            sl_exec(
                db,
                &format!(
                    "INSERT INTO [{rtree}] \
                     SELECT rowid, ST_XMin([{column}]), ST_XMax([{column}]), \
                     ST_YMin([{column}]), ST_YMax([{column}]) \
                     FROM [{table}] WHERE [{column}] IS NOT NULL AND ST_IsEmpty([{column}]) = 0"
                ),
            )?;

            let trigger_insert = format!("{prefix}_insert");
            sl_exec(
                db,
                &format!(
                    "CREATE TRIGGER IF NOT EXISTS [{trigger_insert}] AFTER INSERT ON [{table}] \
                     WHEN NEW.[{column}] IS NOT NULL AND ST_IsEmpty(NEW.[{column}]) = 0 \
                     BEGIN \
                       INSERT INTO [{rtree}] VALUES ( \
                         NEW.rowid, \
                         ST_XMin(NEW.[{column}]), ST_XMax(NEW.[{column}]), \
                         ST_YMin(NEW.[{column}]), ST_YMax(NEW.[{column}]) \
                       ); \
                     END"
                ),
            )?;

            let trigger_update = format!("{prefix}_update");
            sl_exec(
                db,
                &format!(
                    "CREATE TRIGGER IF NOT EXISTS [{trigger_update}] AFTER UPDATE ON [{table}] \
                     WHEN OLD.[{column}] IS NOT NEW.[{column}] OR OLD.rowid IS NOT NEW.rowid \
                     BEGIN \
                       DELETE FROM [{rtree}] WHERE id = OLD.rowid; \
                       INSERT INTO [{rtree}] \
                         SELECT NEW.rowid, \
                           ST_XMin(NEW.[{column}]), ST_XMax(NEW.[{column}]), \
                           ST_YMin(NEW.[{column}]), ST_YMax(NEW.[{column}]) \
                         WHERE NEW.[{column}] IS NOT NULL AND ST_IsEmpty(NEW.[{column}]) = 0; \
                     END"
                ),
            )?;

            let trigger_delete = format!("{prefix}_delete");
            sl_exec(
                db,
                &format!(
                    "CREATE TRIGGER IF NOT EXISTS [{trigger_delete}] AFTER DELETE ON [{table}] \
                     BEGIN \
                       DELETE FROM [{rtree}] WHERE id = OLD.rowid; \
                     END"
                ),
            )?;

            sl_exec(
                db,
                &format!(
                    "INSERT INTO [{SPATIAL_INDEX_CATALOG_TABLE}] (prefix, table_name, column_name) \
                     VALUES ('{prefix}', '{table}', '{column}') \
                     ON CONFLICT(prefix) DO UPDATE SET \
                     table_name = excluded.table_name, \
                     column_name = excluded.column_name"
                ),
            )?;
        }
        Ok(())
    };

    match run() {
        Ok(()) => {
            unsafe { sl_exec(db, &format!("RELEASE {savepoint}"))? };
            api::result_int(ctx, 1);
            Ok(())
        }
        Err(e) => {
            unsafe { sl_rollback_savepoint(db, savepoint) };
            Err(e)
        }
    }
}

pub(super) fn drop_spatial_index_cb(
    ctx: *mut sqlite3_context,
    values: &[*mut sqlite3_value],
) -> Result<()> {
    let (table, column) = extract_table_column(values, "DropSpatialIndex")?;
    let db = unsafe { sqlite3ext_context_db_handle(ctx) };
    let prefix = format!("{table}_{column}");
    let savepoint = "sqlitegis_drop_spatial_index";

    unsafe { sl_exec(db, &format!("SAVEPOINT {savepoint}"))? };

    let run = || -> Result<bool> {
        unsafe {
            sl_ensure_catalog_table(db, "DropSpatialIndex")?;
            sl_validate_catalog_shape(db, "DropSpatialIndex")?;
        }
        let ownership =
            unsafe { sl_ensure_objects_owned_by_table(db, table, column, "DropSpatialIndex")? };
        if ownership == SpatialIndexOwnership::Absent {
            return Ok(false);
        }
        unsafe {
            for suffix in &["_insert", "_update", "_delete"] {
                sl_exec(db, &format!("DROP TRIGGER IF EXISTS [{prefix}{suffix}]"))?;
            }
            sl_exec(db, &format!("DROP TABLE IF EXISTS [{prefix}_rtree]"))?;
            sl_exec(
                db,
                &format!("DELETE FROM [{SPATIAL_INDEX_CATALOG_TABLE}] WHERE prefix = '{prefix}'"),
            )?;
        }
        Ok(true)
    };

    match run() {
        Ok(_) => {
            unsafe { sl_exec(db, &format!("RELEASE {savepoint}"))? };
            api::result_int(ctx, 1);
            Ok(())
        }
        Err(e) => {
            unsafe { sl_rollback_savepoint(db, savepoint) };
            Err(e)
        }
    }
}
