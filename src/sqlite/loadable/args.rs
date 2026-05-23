//! Argument-extraction and result-setting helpers for the loadable path.
//!
//! Every helper here wraps a `sqlite-loadable::api::*` call so the scalar
//! callbacks compose them in a uniform shape: extractors return
//! `Option<T>` (`None` for SQL NULL) or `Option<Result<T>>` (UTF-8 failures
//! surfaced as `Err`), and setters return `Result<()>` so they slot into a
//! callback's `?` chain.

use sqlite_loadable::prelude::*;
use sqlite_loadable::{api, errors::Error, Result};

// Argument extractors.

pub(super) fn arg_blob_or_null(values: &[*mut sqlite3_value], i: usize) -> Option<&[u8]> {
    let v = &values[i];
    if api::value_is_null(v) {
        return None;
    }
    Some(api::value_blob(v))
}

pub(super) fn arg_text_or_null(values: &[*mut sqlite3_value], i: usize) -> Option<Result<&str>> {
    let v = &values[i];
    if api::value_is_null(v) {
        return None;
    }
    Some(api::value_text(v).map_err(|e| Error::new_message(format!("invalid utf-8: {e}"))))
}

pub(super) fn arg_double(values: &[*mut sqlite3_value], i: usize) -> Option<f64> {
    let v = &values[i];
    if api::value_is_null(v) {
        return None;
    }
    Some(api::value_double(v))
}

pub(super) fn arg_i32(values: &[*mut sqlite3_value], i: usize) -> Option<i32> {
    let v = &values[i];
    if api::value_is_null(v) {
        return None;
    }
    Some(api::value_int(v))
}

pub(super) fn any_null(values: &[*mut sqlite3_value]) -> bool {
    values.iter().any(api::value_is_null)
}

pub(super) fn mk_err<E: std::fmt::Display>(label: &str, e: E) -> Error {
    Error::new_message(format!("{label}: {e}"))
}

// Result setters.

pub(super) fn set_blob_vec(ctx: *mut sqlite3_context, v: Vec<u8>) -> Result<()> {
    api::result_blob(ctx, &v);
    Ok(())
}

pub(super) fn set_text_str(ctx: *mut sqlite3_context, v: impl AsRef<str>) -> Result<()> {
    api::result_text(ctx, v.as_ref())
}

pub(super) fn set_f64(ctx: *mut sqlite3_context, v: f64) -> Result<()> {
    api::result_double(ctx, v);
    Ok(())
}

pub(super) fn set_i32(ctx: *mut sqlite3_context, v: i32) -> Result<()> {
    api::result_int(ctx, v);
    Ok(())
}

pub(super) fn set_i64(ctx: *mut sqlite3_context, v: i64) -> Result<()> {
    api::result_int64(ctx, v);
    Ok(())
}

pub(super) fn set_bool(ctx: *mut sqlite3_context, v: bool) -> Result<()> {
    api::result_bool(ctx, v);
    Ok(())
}
