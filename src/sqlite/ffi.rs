//! SQLite extension registration via raw FFI.
//!
//! Registers all SQLiteGIS functions on a raw `*mut sqlite3` handle.
//! On native targets also exports the `sqlite3_sqlitegis_init` C entry point
//! so SQLite can load this library as a loadable extension.
//!
//! # Safety
//!
//! All `unsafe` here sits behind the SQLite `xFunc` ABI:
//! - `argv` holds `argc` valid `*mut sqlite3_value`, and index helpers read only `< n_arg`.
//! - blob/text borrows live only until the callback returns, so every result is copied out first.
//! - `sqlite3_value_bytes` is read after `_blob`/`_text` so the length reflects coercion.
//! - `extern "C"` entry points `catch_unwind`, since unwinding across the ABI is UB.
//! - result lengths are range-checked into `c_int` via `checked_c_int_len`, buffers copied by `SQLITE_TRANSIENT`.

use super::host_api;
use super::sqlite_compat::sqlite_transient;
use super::sqlite_compat::*;
use std::ffi::{CStr, CString};
use std::os::raw::c_int;

use crate::core::function_catalog::{
    SqliteFunctionSpec, SQLITE_DETERMINISTIC_FUNCTIONS, SQLITE_DIRECT_ONLY_FUNCTIONS,
};
use crate::core::functions::accessors::*;
use crate::core::functions::constructors::*;
use crate::core::functions::io::*;
use crate::core::functions::linear_referencing::*;
use crate::core::functions::measurement::*;
use crate::core::functions::operations::*;
use crate::core::functions::predicates::*;

// Constants

const DET: c_int = SQLITE_UTF8 | SQLITE_DETERMINISTIC | SQLITE_INNOCUOUS;

/// `SQLITE_DIRECTONLY` (0x80000) prevents use from triggers/views.
/// Not yet exported by all `libsqlite3-sys` versions, so we define it here.
const SQLITE_DIRECTONLY_FLAG: c_int = 0x0008_0000;
const DIRECT: c_int = SQLITE_UTF8 | SQLITE_DIRECTONLY_FLAG;

// Argument-extraction helpers

/// Borrow argument `i` as a blob.
///
/// # Safety
/// `i < argc`, and the returned slice must not outlive the callback (see the
/// module `# Safety`).
unsafe fn get_blob<'a>(argv: *mut *mut sqlite3_value, i: usize) -> Option<&'a [u8]> {
    unsafe {
        let v = *argv.add(i);
        if host_api::value_type(v) == SQLITE_NULL {
            return None;
        }
        // `_blob` before `_bytes` so `len` reflects coercion. from_raw_parts runs
        // only with non-null `ptr` (zero len short-circuits above).
        let ptr = host_api::value_blob(v) as *const u8;
        let len = host_api::value_bytes(v) as usize;
        if len == 0 {
            return Some(&[]);
        }
        if ptr.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(ptr, len))
    }
}

enum SqlTextArg<'a> {
    Null,
    Value(&'a str),
    InvalidUtf8,
}

/// Borrow argument `i` as UTF-8 text. Same `# Safety` as [`get_blob`]. Non-UTF-8
/// or a null pointer yields `InvalidUtf8`.
unsafe fn get_text<'a>(argv: *mut *mut sqlite3_value, i: usize) -> SqlTextArg<'a> {
    unsafe {
        let v = *argv.add(i);
        if host_api::value_type(v) == SQLITE_NULL {
            return SqlTextArg::Null;
        }
        // `_text` before `_bytes` for a correct post-coercion `len`, null rejected
        // before from_raw_parts.
        let ptr = host_api::value_text(v);
        let len = host_api::value_bytes(v) as usize;
        if ptr.is_null() {
            return SqlTextArg::InvalidUtf8;
        }
        match std::str::from_utf8(std::slice::from_raw_parts(ptr as _, len)) {
            Ok(s) => SqlTextArg::Value(s),
            Err(_) => SqlTextArg::InvalidUtf8,
        }
    }
}

enum SqlArg<T> {
    Null,
    Value(T),
    InvalidType,
}

enum SqlI32Arg {
    Null,
    Value(i32),
    InvalidType,
    OutOfRange(i64),
}

unsafe fn get_f64_arg(argv: *mut *mut sqlite3_value, i: usize) -> SqlArg<f64> {
    unsafe {
        let v = *argv.add(i);
        match host_api::value_type(v) {
            SQLITE_NULL => SqlArg::Null,
            SQLITE_INTEGER | SQLITE_FLOAT => SqlArg::Value(host_api::value_double(v)),
            _ => SqlArg::InvalidType,
        }
    }
}

unsafe fn get_i32_arg(argv: *mut *mut sqlite3_value, i: usize) -> SqlI32Arg {
    unsafe {
        let v = *argv.add(i);
        match host_api::value_type(v) {
            SQLITE_NULL => SqlI32Arg::Null,
            SQLITE_INTEGER => {
                let raw = host_api::value_int64(v);
                match i32::try_from(raw) {
                    Ok(value) => SqlI32Arg::Value(value),
                    Err(_) => SqlI32Arg::OutOfRange(raw),
                }
            }
            _ => SqlI32Arg::InvalidType,
        }
    }
}

// Result-setting helpers

fn checked_c_int_len(len: usize) -> Option<c_int> {
    c_int::try_from(len).ok()
}

const ERROR_MSG_TOO_LARGE: &str = "internal error: error message too large";
const PANIC_IN_CALLBACK_MSG: &str = "panic in SQLite callback";

unsafe fn set_blob(ctx: *mut sqlite3_context, data: &[u8]) {
    unsafe {
        let Some(len) = checked_c_int_len(data.len()) else {
            set_error(ctx, "internal error: BLOB result too large");
            return;
        };
        host_api::result_blob(ctx, data.as_ptr().cast(), len, sqlite_transient());
    }
}

unsafe fn set_text(ctx: *mut sqlite3_context, s: &str) {
    unsafe {
        let Some(len) = checked_c_int_len(s.len()) else {
            set_error(ctx, "internal error: text result too large");
            return;
        };
        host_api::result_text(ctx, s.as_ptr().cast(), len, sqlite_transient());
    }
}

unsafe fn set_f64(ctx: *mut sqlite3_context, v: f64) {
    unsafe {
        host_api::result_double(ctx, v);
    }
}
unsafe fn set_i64(ctx: *mut sqlite3_context, v: i64) {
    unsafe {
        host_api::result_int64(ctx, v);
    }
}
unsafe fn set_i32(ctx: *mut sqlite3_context, v: i32) {
    unsafe {
        host_api::result_int(ctx, v);
    }
}
unsafe fn set_null(ctx: *mut sqlite3_context) {
    unsafe {
        host_api::result_null(ctx);
    }
}

unsafe fn set_error(ctx: *mut sqlite3_context, msg: &str) {
    unsafe {
        if let Some(len) = checked_c_int_len(msg.len()) {
            host_api::result_error(ctx, msg.as_ptr().cast(), len);
            return;
        }

        let len = c_int::try_from(ERROR_MSG_TOO_LARGE.len())
            .expect("fallback error length must fit in c_int");
        host_api::result_error(ctx, ERROR_MSG_TOO_LARGE.as_ptr().cast(), len);
    }
}

unsafe fn xfunc_guard<F>(ctx: *mut sqlite3_context, label: &str, f: F)
where
    F: FnOnce(),
{
    unsafe {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
        if result.is_err() {
            set_error(ctx, &format!("{label}: {PANIC_IN_CALLBACK_MSG}"));
        }
    }
}

unsafe fn require_f64_arg(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    i: usize,
    fn_name: &str,
    arg_name: &str,
) -> Option<f64> {
    unsafe {
        match get_f64_arg(argv, i) {
            SqlArg::Value(v) => Some(v),
            SqlArg::Null => {
                set_null(ctx);
                None
            }
            SqlArg::InvalidType => {
                set_error(ctx, &format!("{fn_name}: {arg_name} must be numeric"));
                None
            }
        }
    }
}

unsafe fn require_i32_arg(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    i: usize,
    fn_name: &str,
    arg_name: &str,
) -> Option<i32> {
    unsafe {
        match get_i32_arg(argv, i) {
            SqlI32Arg::Value(v) => Some(v),
            SqlI32Arg::Null => {
                set_null(ctx);
                None
            }
            SqlI32Arg::InvalidType => {
                set_error(ctx, &format!("{fn_name}: {arg_name} must be integer"));
                None
            }
            SqlI32Arg::OutOfRange(v) => {
                set_error(
                    ctx,
                    &format!("{fn_name}: {arg_name} out of range for i32: {v}"),
                );
                None
            }
        }
    }
}

unsafe fn require_text_arg<'a>(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    i: usize,
    fn_name: &str,
    arg_name: &str,
) -> Option<&'a str> {
    unsafe {
        match get_text(argv, i) {
            SqlTextArg::Value(v) => Some(v),
            SqlTextArg::Null => {
                set_null(ctx);
                None
            }
            SqlTextArg::InvalidUtf8 => {
                set_error(
                    ctx,
                    &format!("{fn_name}: {arg_name} must be valid UTF-8 text"),
                );
                None
            }
        }
    }
}

unsafe fn any_arg_is_null(argv: *mut *mut sqlite3_value, arg_count: usize) -> bool {
    unsafe {
        for i in 0..arg_count {
            if host_api::value_type(*argv.add(i)) == SQLITE_NULL {
                return true;
            }
        }
        false
    }
}

unsafe fn optional_srid_arg(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    with_srid: bool,
    index: usize,
    fn_name: &str,
) -> Option<Option<i32>> {
    unsafe {
        if with_srid {
            let srid = require_i32_arg(ctx, argv, index, fn_name, "srid")?;
            Some(Some(srid))
        } else {
            Some(None)
        }
    }
}

// Convenience setter wrappers

unsafe fn set_bool(ctx: *mut sqlite3_context, v: bool) {
    unsafe {
        set_i32(ctx, v as i32);
    }
}
unsafe fn set_blob_owned(ctx: *mut sqlite3_context, v: Vec<u8>) {
    unsafe {
        set_blob(ctx, &v);
    }
}
unsafe fn set_text_owned(ctx: *mut sqlite3_context, v: impl AsRef<str>) {
    unsafe {
        set_text(ctx, v.as_ref());
    }
}

// Callback macros
//
// Each xfunc_* macro below generates an `unsafe extern "C" fn` with the
// standard SQLite scalar-function signature. NULL blob/text inputs produce
// NULL output (PostGIS-compatible). Errors produce sqlite3_result_error.
//
// All eleven macros share two pieces of boilerplate that are factored out
// into the two inner helpers below:
//
// - `xfunc_decl!` emits the extern "C" fn signature and wraps the body in
//   the panic-catching `xfunc_guard`.
// - `xfunc_dispatch!` matches a `Result<T, _>` and routes the Ok arm to a
//   setter expression while the Err arm formats and calls `set_error`.

/// Emit an `unsafe extern "C" fn $name` with the SQLite scalar signature,
/// wrapping `$body` in `xfunc_guard`. `$ctx` and `$argv` are bound to the
/// pointer parameters so the body can name them.
macro_rules! xfunc_decl {
    ($name:ident, $label:expr, $ctx:ident, $argv:ident, $body:block) => {
        unsafe extern "C" fn $name(
            $ctx: *mut sqlite3_context,
            _n: c_int,
            $argv: *mut *mut sqlite3_value,
        ) {
            unsafe {
                xfunc_guard($ctx, $label, || $body);
            }
        }
    };
}

/// Match a `Result<T, _>` from a callback: route `Ok(v)` to `$set(ctx, v)`
/// and format `Err(e)` into a SQLite error message tagged with `$label`.
macro_rules! xfunc_dispatch {
    ($ctx:expr, $label:expr, $result:expr, $set:expr) => {
        match $result {
            Ok(v) => $set($ctx, v),
            Err(e) => set_error($ctx, &format!(concat!($label, ": {}"), e)),
        }
    };
}

/// 1 blob -> Result<T>, with a custom setter expression.
macro_rules! xfunc_blob {
    ($name:ident, $label:expr, $func:expr, $set:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(b), $set);
        });
    };
}

/// 2 blobs -> Result<T>, with a custom setter expression.
macro_rules! xfunc_blob2 {
    ($name:ident, $label:expr, $func:expr, $set:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(a) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(b) = get_blob(argv, 1) else {
                set_null(ctx);
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(a, b), $set);
        });
    };
}

/// 1 blob -> Result<Option<f64>>, where `None` maps to SQL NULL.
///
/// Has its own three-arm match (`Ok(Some)` / `Ok(None)` / `Err`), so it
/// uses `xfunc_decl!` for the signature but not `xfunc_dispatch!`.
macro_rules! xfunc_blob_opt_f64 {
    ($name:ident, $label:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(blob) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            match $func(blob) {
                Ok(Some(v)) => set_f64(ctx, v),
                Ok(None) => set_null(ctx),
                Err(e) => set_error(ctx, &format!(concat!($label, ": {}"), e)),
            }
        });
    };
}

/// blob + integer arg -> Result<Vec<u8>>.
macro_rules! xfunc_blob_i32_blob {
    ($name:ident, $label:expr, $arg_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(n) = require_i32_arg(ctx, argv, 1, $label, $arg_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(b, n), set_blob_owned);
        });
    };
}

/// blob + numeric arg -> Result<Vec<u8>>.
macro_rules! xfunc_blob_f64_blob {
    ($name:ident, $label:expr, $arg_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(v) = require_f64_arg(ctx, argv, 1, $label, $arg_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(b, v), set_blob_owned);
        });
    };
}

/// blob + numeric arg + numeric arg -> Result<Vec<u8>>.
macro_rules! xfunc_blob_f64_f64_blob {
    ($name:ident, $label:expr, $arg1_name:expr, $arg2_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(v1) = require_f64_arg(ctx, argv, 1, $label, $arg1_name) else {
                return;
            };
            let Some(v2) = require_f64_arg(ctx, argv, 2, $label, $arg2_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(b, v1, v2), set_blob_owned);
        });
    };
}

/// 2 blobs + numeric arg -> Result<bool>.
macro_rules! xfunc_blob2_f64_bool {
    ($name:ident, $label:expr, $arg_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(a) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(b) = get_blob(argv, 1) else {
                set_null(ctx);
                return;
            };
            let Some(v) = require_f64_arg(ctx, argv, 2, $label, $arg_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(a, b, v), set_bool);
        });
    };
}

/// 2 blobs + text arg -> Result<bool>.
macro_rules! xfunc_blob2_text_bool {
    ($name:ident, $label:expr, $arg_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(a) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(b) = get_blob(argv, 1) else {
                set_null(ctx);
                return;
            };
            let Some(v) = require_text_arg(ctx, argv, 2, $label, $arg_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(a, b, v), set_bool);
        });
    };
}

/// 2 text args -> Result<bool>.
macro_rules! xfunc_text2_bool {
    ($name:ident, $label:expr, $arg1_name:expr, $arg2_name:expr, $func:expr) => {
        xfunc_decl!($name, $label, ctx, argv, {
            let Some(a) = require_text_arg(ctx, argv, 0, $label, $arg1_name) else {
                return;
            };
            let Some(b) = require_text_arg(ctx, argv, 1, $label, $arg2_name) else {
                return;
            };
            xfunc_dispatch!(ctx, $label, ($func)(a, b), set_bool);
        });
    };
}

// I/O callbacks

/// text + optional SRID -> blob (generates two callbacks: 1-arg and 2-arg).
macro_rules! xfunc_text_optsrid_blob {
    ($name1:ident, $name2:ident, $label:expr, $func:expr) => {
        xfunc_decl!($name1, $label, ctx, argv, {
            let Some(t) = require_text_arg(ctx, argv, 0, $label, "wkt") else {
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(t, None), set_blob_owned);
        });
        xfunc_decl!($name2, $label, ctx, argv, {
            let Some(t) = require_text_arg(ctx, argv, 0, $label, "wkt") else {
                return;
            };
            let Some(srid) = require_i32_arg(ctx, argv, 1, $label, "srid") else {
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(t, Some(srid)), set_blob_owned);
        });
    };
}

/// blob + optional SRID -> blob (generates two callbacks: 1-arg and 2-arg).
macro_rules! xfunc_blob_optsrid_blob {
    ($name1:ident, $name2:ident, $label:expr, $func:expr) => {
        xfunc_decl!($name1, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(b, None), set_blob_owned);
        });
        xfunc_decl!($name2, $label, ctx, argv, {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(srid) = require_i32_arg(ctx, argv, 1, $label, "srid") else {
                return;
            };
            xfunc_dispatch!(ctx, $label, $func(b, Some(srid)), set_blob_owned);
        });
    };
}

xfunc_text_optsrid_blob!(
    st_geomfromtext_1_xfunc,
    st_geomfromtext_2_xfunc,
    "ST_GeomFromText",
    geom_from_text
);
xfunc_blob_optsrid_blob!(
    st_geomfromwkb_1_xfunc,
    st_geomfromwkb_2_xfunc,
    "ST_GeomFromWKB",
    geom_from_wkb
);
xfunc_blob!(
    st_geomfromewkb_xfunc,
    "ST_GeomFromEWKB",
    geom_from_ewkb,
    set_blob_owned
);

unsafe extern "C" fn st_geomfromgeojson_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_GeomFromGeoJSON", || {
            let Some(json) = require_text_arg(ctx, argv, 0, "ST_GeomFromGeoJSON", "json") else {
                return;
            };
            match geom_from_geojson(json, None) {
                Ok(v) => set_blob(ctx, &v),
                Err(e) => set_error(ctx, &format!("ST_GeomFromGeoJSON: {e}")),
            }
        });
    }
}

xfunc_blob!(st_astext_xfunc, "ST_AsText", as_text, set_text_owned);
xfunc_blob!(st_asewkt_xfunc, "ST_AsEWKT", as_ewkt, set_text_owned);
xfunc_blob!(st_asbinary_xfunc, "ST_AsBinary", as_binary, set_blob_owned);
xfunc_blob!(st_asewkb_xfunc, "ST_AsEWKB", as_ewkb, set_blob_owned);
xfunc_blob!(
    st_asgeojson_xfunc,
    "ST_AsGeoJSON",
    as_geojson,
    set_text_owned
);

// Constructor callbacks

unsafe fn st_point_impl(ctx: *mut sqlite3_context, argv: *mut *mut sqlite3_value, with_srid: bool) {
    unsafe {
        let arg_count = if with_srid { 3 } else { 2 };
        if any_arg_is_null(argv, arg_count) {
            set_null(ctx);
            return;
        }

        let Some(x) = require_f64_arg(ctx, argv, 0, "ST_Point", "x") else {
            return;
        };
        let Some(y) = require_f64_arg(ctx, argv, 1, "ST_Point", "y") else {
            return;
        };
        let Some(srid) = optional_srid_arg(ctx, argv, with_srid, 2, "ST_Point") else {
            return;
        };

        match st_point(x, y, srid) {
            Ok(v) => set_blob(ctx, &v),
            Err(e) => set_error(ctx, &format!("ST_Point: {e}")),
        }
    }
}

unsafe extern "C" fn st_point_2_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_Point", || {
            st_point_impl(ctx, argv, false);
        });
    }
}

unsafe extern "C" fn st_point_3_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_Point", || {
            st_point_impl(ctx, argv, true);
        });
    }
}

xfunc_blob2!(
    st_makeline_xfunc,
    "ST_MakeLine",
    st_make_line,
    set_blob_owned
);
xfunc_blob!(
    st_makepolygon_xfunc,
    "ST_MakePolygon",
    st_make_polygon,
    set_blob_owned
);

unsafe fn st_makeenvelope_impl(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    with_srid: bool,
) {
    unsafe {
        let arg_count = if with_srid { 5 } else { 4 };
        if any_arg_is_null(argv, arg_count) {
            set_null(ctx);
            return;
        }

        let Some(xmin) = require_f64_arg(ctx, argv, 0, "ST_MakeEnvelope", "xmin") else {
            return;
        };
        let Some(ymin) = require_f64_arg(ctx, argv, 1, "ST_MakeEnvelope", "ymin") else {
            return;
        };
        let Some(xmax) = require_f64_arg(ctx, argv, 2, "ST_MakeEnvelope", "xmax") else {
            return;
        };
        let Some(ymax) = require_f64_arg(ctx, argv, 3, "ST_MakeEnvelope", "ymax") else {
            return;
        };
        let Some(srid) = optional_srid_arg(ctx, argv, with_srid, 4, "ST_MakeEnvelope") else {
            return;
        };

        match st_make_envelope(xmin, ymin, xmax, ymax, srid) {
            Ok(v) => set_blob(ctx, &v),
            Err(e) => set_error(ctx, &format!("ST_MakeEnvelope: {e}")),
        }
    }
}

unsafe extern "C" fn st_makeenvelope_4_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_MakeEnvelope", || {
            st_makeenvelope_impl(ctx, argv, false);
        });
    }
}

unsafe extern "C" fn st_makeenvelope_5_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_MakeEnvelope", || {
            st_makeenvelope_impl(ctx, argv, true);
        });
    }
}

xfunc_blob2!(st_collect_xfunc, "ST_Collect", st_collect, set_blob_owned);

unsafe extern "C" fn st_tileenvelope_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_TileEnvelope", || {
            let Some(zoom_i32) = require_i32_arg(ctx, argv, 0, "ST_TileEnvelope", "zoom") else {
                return;
            };
            let Some(tile_x_i32) = require_i32_arg(ctx, argv, 1, "ST_TileEnvelope", "tile x")
            else {
                return;
            };
            let Some(tile_y_i32) = require_i32_arg(ctx, argv, 2, "ST_TileEnvelope", "tile y")
            else {
                return;
            };

            if zoom_i32 < 0 {
                set_error(ctx, "ST_TileEnvelope: zoom must be non-negative");
                return;
            }
            if tile_x_i32 < 0 {
                set_error(ctx, "ST_TileEnvelope: tile x must be non-negative");
                return;
            }
            if tile_y_i32 < 0 {
                set_error(ctx, "ST_TileEnvelope: tile y must be non-negative");
                return;
            }

            let zoom = zoom_i32 as u32;
            let tile_x = tile_x_i32 as u32;
            let tile_y = tile_y_i32 as u32;
            match st_tile_envelope(zoom, tile_x, tile_y) {
                Ok(v) => set_blob(ctx, &v),
                Err(e) => set_error(ctx, &format!("ST_TileEnvelope: {e}")),
            }
        });
    }
}

// Accessor callbacks

xfunc_blob!(st_srid_xfunc, "ST_SRID", st_srid, set_i32);

unsafe extern "C" fn st_setsrid_xfunc(
    ctx: *mut sqlite3_context,
    _n: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        xfunc_guard(ctx, "ST_SetSRID", || {
            let Some(b) = get_blob(argv, 0) else {
                set_null(ctx);
                return;
            };
            let Some(srid) = require_i32_arg(ctx, argv, 1, "ST_SetSRID", "srid") else {
                return;
            };
            match st_set_srid(b, srid) {
                Ok(v) => set_blob(ctx, &v),
                Err(e) => set_error(ctx, &format!("ST_SetSRID: {e}")),
            }
        });
    }
}

xfunc_blob!(
    st_geometrytype_xfunc,
    "ST_GeometryType",
    st_geometry_type,
    set_text_owned
);
xfunc_blob!(st_ndims_xfunc, "ST_NDims", st_ndims, set_i32);
xfunc_blob!(st_coorddim_xfunc, "ST_CoordDim", st_coord_dim, set_i32);
xfunc_blob!(st_zmflag_xfunc, "ST_Zmflag", st_zmflag, set_i32);
xfunc_blob!(st_isempty_xfunc, "ST_IsEmpty", st_is_empty, set_bool);
xfunc_blob!(st_memsize_xfunc, "ST_MemSize", st_mem_size, set_i64);
xfunc_blob_opt_f64!(st_x_xfunc, "ST_X", st_x);
xfunc_blob_opt_f64!(st_y_xfunc, "ST_Y", st_y);
xfunc_blob_opt_f64!(st_z_xfunc, "ST_Z", st_z);

xfunc_blob!(st_numpoints_xfunc, "ST_NumPoints", st_num_points, set_i32);
xfunc_blob!(st_npoints_xfunc, "ST_NPoints", st_npoints, set_i32);
xfunc_blob!(
    st_numgeometries_xfunc,
    "ST_NumGeometries",
    st_num_geometries,
    set_i32
);
xfunc_blob!(
    st_numinteriorrings_xfunc,
    "ST_NumInteriorRings",
    st_num_interior_rings,
    set_i32
);
xfunc_blob!(st_numrings_xfunc, "ST_NumRings", st_num_rings, set_i32);
xfunc_blob_i32_blob!(st_pointn_xfunc, "ST_PointN", "n", |b, n| st_point_n(
    b, n, None
));

xfunc_blob!(
    st_startpoint_xfunc,
    "ST_StartPoint",
    st_start_point,
    set_blob_owned
);
xfunc_blob!(
    st_endpoint_xfunc,
    "ST_EndPoint",
    st_end_point,
    set_blob_owned
);
xfunc_blob!(
    st_exteriorring_xfunc,
    "ST_ExteriorRing",
    st_exterior_ring,
    set_blob_owned
);
xfunc_blob_i32_blob!(
    st_interiorringn_xfunc,
    "ST_InteriorRingN",
    "n",
    st_interior_ring_n
);
xfunc_blob_i32_blob!(st_geometryn_xfunc, "ST_GeometryN", "n", st_geometry_n);

xfunc_blob!(st_dimension_xfunc, "ST_Dimension", st_dimension, set_i32);
xfunc_blob!(
    st_envelope_xfunc,
    "ST_Envelope",
    st_envelope,
    set_blob_owned
);
xfunc_blob!(st_isvalid_xfunc, "ST_IsValid", st_is_valid, set_bool);
xfunc_blob!(
    st_isvalidreason_xfunc,
    "ST_IsValidReason",
    st_is_valid_reason,
    set_text_owned
);

// Measurement callbacks

xfunc_blob!(st_area_xfunc, "ST_Area", st_area, set_f64);
xfunc_blob!(st_length_xfunc, "ST_Length", st_length, set_f64);
xfunc_blob!(st_perimeter_xfunc, "ST_Perimeter", st_perimeter, set_f64);
xfunc_blob2!(st_distance_xfunc, "ST_Distance", st_distance, set_f64);
xfunc_blob!(
    st_centroid_xfunc,
    "ST_Centroid",
    st_centroid,
    set_blob_owned
);
xfunc_blob!(
    st_pointonsurface_xfunc,
    "ST_PointOnSurface",
    st_point_on_surface,
    set_blob_owned
);
xfunc_blob2!(
    st_hausdorffdistance_xfunc,
    "ST_HausdorffDistance",
    st_hausdorff_distance,
    set_f64
);
xfunc_blob_opt_f64!(st_xmin_xfunc, "ST_XMin", st_xmin);
xfunc_blob_opt_f64!(st_xmax_xfunc, "ST_XMax", st_xmax);
xfunc_blob_opt_f64!(st_ymin_xfunc, "ST_YMin", st_ymin);
xfunc_blob_opt_f64!(st_ymax_xfunc, "ST_YMax", st_ymax);
xfunc_blob2!(
    st_distancesphere_xfunc,
    "ST_DistanceSphere",
    st_distance_sphere,
    set_f64
);
xfunc_blob2!(
    st_distancespheroid_xfunc,
    "ST_DistanceSpheroid",
    st_distance_spheroid,
    set_f64
);
xfunc_blob!(
    st_lengthsphere_xfunc,
    "ST_LengthSphere",
    st_length_sphere,
    set_f64
);
xfunc_blob!(
    st_lengthspheroid_xfunc,
    "ST_LengthSpheroid",
    st_length_spheroid,
    set_f64
);
xfunc_blob!(
    st_areasphere_xfunc,
    "ST_AreaSphere",
    st_area_sphere,
    set_f64
);
xfunc_blob!(
    st_areaspheroid_xfunc,
    "ST_AreaSpheroid",
    st_area_spheroid,
    set_f64
);
xfunc_blob!(
    st_perimetersphere_xfunc,
    "ST_PerimeterSphere",
    st_perimeter_sphere,
    set_f64
);
xfunc_blob!(
    st_perimeterspheroid_xfunc,
    "ST_PerimeterSpheroid",
    st_perimeter_spheroid,
    set_f64
);
xfunc_blob2!(st_azimuth_xfunc, "ST_Azimuth", st_azimuth, set_f64);

xfunc_blob_f64_f64_blob!(
    st_project_xfunc,
    "ST_Project",
    "distance",
    "azimuth",
    st_project
);

xfunc_blob2!(
    st_closestpoint_xfunc,
    "ST_ClosestPoint",
    st_closest_point,
    set_blob_owned
);

// Operation callbacks

xfunc_blob2!(st_union_xfunc, "ST_Union", st_union, set_blob_owned);
xfunc_blob2!(
    st_intersection_xfunc,
    "ST_Intersection",
    st_intersection,
    set_blob_owned
);
xfunc_blob2!(
    st_difference_xfunc,
    "ST_Difference",
    st_difference,
    set_blob_owned
);
xfunc_blob2!(
    st_symdifference_xfunc,
    "ST_SymDifference",
    st_sym_difference,
    set_blob_owned
);

xfunc_blob_f64_blob!(st_buffer_xfunc, "ST_Buffer", "distance", st_buffer);

xfunc_blob_f64_blob!(
    st_segmentize_xfunc,
    "ST_Segmentize",
    "max_segment_length",
    st_segmentize
);
xfunc_blob_f64_blob!(
    st_segmentizesphere_xfunc,
    "ST_SegmentizeSphere",
    "max_segment_length",
    st_segmentize_sphere
);
xfunc_blob_f64_blob!(
    st_segmentizespheroid_xfunc,
    "ST_SegmentizeSpheroid",
    "max_segment_length",
    st_segmentize_spheroid
);

// Linear-referencing callbacks

xfunc_blob_f64_blob!(
    st_lineinterpolatepoint_xfunc,
    "ST_LineInterpolatePoint",
    "fraction",
    st_line_interpolate_point
);
xfunc_blob_f64_blob!(
    st_lineinterpolatepointsphere_xfunc,
    "ST_LineInterpolatePointSphere",
    "fraction",
    st_line_interpolate_point_sphere
);
xfunc_blob_f64_blob!(
    st_lineinterpolatepointspheroid_xfunc,
    "ST_LineInterpolatePointSpheroid",
    "fraction",
    st_line_interpolate_point_spheroid
);
xfunc_blob_f64_blob!(
    st_lineinterpolatepoints_xfunc,
    "ST_LineInterpolatePoints",
    "fraction",
    st_line_interpolate_points
);
xfunc_blob_f64_blob!(
    st_lineinterpolatepointssphere_xfunc,
    "ST_LineInterpolatePointsSphere",
    "fraction",
    st_line_interpolate_points_sphere
);
xfunc_blob_f64_blob!(
    st_lineinterpolatepointsspheroid_xfunc,
    "ST_LineInterpolatePointsSpheroid",
    "fraction",
    st_line_interpolate_points_spheroid
);
xfunc_blob_f64_f64_blob!(
    st_linesubstring_xfunc,
    "ST_LineSubstring",
    "start fraction",
    "end fraction",
    st_line_substring
);
xfunc_blob_f64_f64_blob!(
    st_linesubstringsphere_xfunc,
    "ST_LineSubstringSphere",
    "start fraction",
    "end fraction",
    st_line_substring_sphere
);
xfunc_blob_f64_f64_blob!(
    st_linesubstringspheroid_xfunc,
    "ST_LineSubstringSpheroid",
    "start fraction",
    "end fraction",
    st_line_substring_spheroid
);

// Predicate callbacks

xfunc_blob2!(
    st_intersects_xfunc,
    "ST_Intersects",
    st_intersects,
    set_bool
);
xfunc_blob2!(st_contains_xfunc, "ST_Contains", st_contains, set_bool);
xfunc_blob2!(st_within_xfunc, "ST_Within", st_within, set_bool);
xfunc_blob2!(st_disjoint_xfunc, "ST_Disjoint", st_disjoint, set_bool);

xfunc_blob2_f64_bool!(st_dwithin_xfunc, "ST_DWithin", "distance", st_dwithin);
xfunc_blob2_f64_bool!(
    st_dwithinsphere_xfunc,
    "ST_DWithinSphere",
    "distance",
    st_dwithin_sphere
);
xfunc_blob2_f64_bool!(
    st_dwithinspheroid_xfunc,
    "ST_DWithinSpheroid",
    "distance",
    st_dwithin_spheroid
);

xfunc_blob2!(st_covers_xfunc, "ST_Covers", st_covers, set_bool);
xfunc_blob2!(st_coveredby_xfunc, "ST_CoveredBy", st_covered_by, set_bool);
xfunc_blob2!(st_equals_xfunc, "ST_Equals", st_equals, set_bool);
xfunc_blob2!(st_touches_xfunc, "ST_Touches", st_touches, set_bool);
xfunc_blob2!(st_crosses_xfunc, "ST_Crosses", st_crosses, set_bool);
xfunc_blob2!(st_overlaps_xfunc, "ST_Overlaps", st_overlaps, set_bool);

xfunc_blob2!(st_relate_2_xfunc, "ST_Relate", st_relate, set_text_owned);

xfunc_blob2_text_bool!(
    st_relate_3_xfunc,
    "ST_Relate",
    "pattern",
    st_relate_match_geoms
);
xfunc_text2_bool!(
    st_relatematch_xfunc,
    "ST_RelateMatch",
    "matrix",
    "pattern",
    st_relate_match
);

// Spatial index helpers

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

fn sql_to_cstring(sql: &str) -> std::result::Result<CString, std::ffi::NulError> {
    CString::new(sql)
}

/// Runs `sql` via `sqlite3_exec`, returning SQLite's error message on failure.
unsafe fn exec_sql(db: *mut sqlite3, sql: &str) -> std::result::Result<(), String> {
    unsafe {
        let c_sql = sql_to_cstring(sql)
            .map_err(|_| "internal error: generated SQL contains NUL byte".to_string())?;
        let mut err_msg: *mut std::ffi::c_char = std::ptr::null_mut();
        let rc = host_api::exec(db, c_sql.as_ptr(), None, std::ptr::null_mut(), &mut err_msg);
        let message = (!err_msg.is_null()).then(|| {
            let message = CStr::from_ptr(err_msg).to_string_lossy().into_owned();
            host_api::free(err_msg.cast());
            message
        });
        if rc == SQLITE_OK {
            return Ok(());
        }
        Err(message.unwrap_or_else(|| "exec_sql failed".to_owned()))
    }
}

/// Runs `body` inside `savepoint`, rolling every change back when it fails.
unsafe fn run_in_savepoint<T>(
    db: *mut sqlite3,
    savepoint: &str,
    body: impl FnOnce() -> std::result::Result<T, SpatialIndexError>,
) -> std::result::Result<T, SpatialIndexError> {
    unsafe {
        exec_sql(db, &format!("SAVEPOINT {savepoint}"))?;
        match body() {
            Ok(value) => {
                exec_sql(db, &format!("RELEASE {savepoint}"))?;
                Ok(value)
            }
            Err(e) => {
                let _ = exec_sql(db, &format!("ROLLBACK TO {savepoint}"));
                let _ = exec_sql(db, &format!("RELEASE {savepoint}"));
                Err(e)
            }
        }
    }
}

unsafe fn sqlite_master_lookup_text(
    db: *mut sqlite3,
    sql: &str,
) -> std::result::Result<Option<String>, String> {
    unsafe {
        let c_sql = sql_to_cstring(sql)
            .map_err(|_| "internal error: generated SQL contains NUL byte".to_string())?;
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc = host_api::prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
        if rc != SQLITE_OK {
            return Err(CStr::from_ptr(host_api::errmsg(db))
                .to_string_lossy()
                .into_owned());
        }

        let mut result = None;
        let step = host_api::step(stmt);
        if step == SQLITE_ROW {
            if host_api::column_type(stmt, 0) != SQLITE_NULL {
                let ptr = host_api::column_text(stmt, 0);
                if !ptr.is_null() {
                    result = Some(CStr::from_ptr(ptr.cast()).to_string_lossy().into_owned());
                }
            }
        } else if step != SQLITE_DONE {
            let err = CStr::from_ptr(host_api::errmsg(db))
                .to_string_lossy()
                .into_owned();
            let _ = host_api::finalize(stmt);
            return Err(err);
        }

        let _ = host_api::finalize(stmt);
        Ok(result)
    }
}

const SPATIAL_INDEX_CATALOG_TABLE: &str = "sqlitegis_spatial_index_catalog";
const SPATIAL_INDEX_CATALOG_REQUIRED_COLUMNS: [&str; 3] = ["prefix", "table_name", "column_name"];
const SPATIAL_INDEX_TRIGGER_SUFFIXES: [&str; 3] = ["_insert", "_update", "_delete"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpatialIndexOwnership {
    Owned,
    Absent,
}

/// Why a `CreateSpatialIndex` or `DropSpatialIndex` call failed.
#[derive(Debug, thiserror::Error)]
enum SpatialIndexError {
    #[error("unknown schema [{0}]")]
    UnknownSchema(String),
    #[error("no such table [{0}]")]
    NoSuchTable(String),
    #[error("no such table [{schema}].[{table}]")]
    NoSuchQualifiedTable { schema: String, table: String },
    #[error("[{schema}].[{table}] is not a table (found {object_type})")]
    NotATable {
        schema: String,
        table: String,
        object_type: String,
    },
    #[error(
        "table [{schema}].[{table}] has no rowid column. WITHOUT ROWID tables are not \
         supported. Recreate the table without the WITHOUT ROWID clause."
    )]
    WithoutRowid { schema: String, table: String },
    #[error(
        "invalid spatial index catalog object type for [{schema}].[{catalog}] \
         (expected table, found [{object_type}])",
        catalog = SPATIAL_INDEX_CATALOG_TABLE
    )]
    CatalogNotATable { schema: String, object_type: String },
    #[error(
        "invalid spatial index catalog schema for [{schema}].[{catalog}] \
         (missing required column [{column}])",
        catalog = SPATIAL_INDEX_CATALOG_TABLE
    )]
    CatalogMissingColumn {
        schema: String,
        column: &'static str,
    },
    #[error("catalog row for prefix [{0}] is missing column_name")]
    CatalogRowIncomplete(String),
    #[error("unexpected sqlite_master entry for [{schema}].[{name}] (type [{object_type}]); expected table")]
    RtreeNotATable {
        schema: String,
        name: String,
        object_type: String,
    },
    #[error(
        "existing table [{schema}].[{name}] is not an R-tree index managed by SQLiteGIS \
         (missing shadow table [{shadow}])"
    )]
    RtreeMissingShadow {
        schema: String,
        name: String,
        shadow: String,
    },
    #[error("naming collision for trigger [{schema}].[{trigger}] between tables [{owner}] and [{table}]")]
    TriggerCollision {
        schema: String,
        trigger: String,
        owner: String,
        table: String,
    },
    #[error("naming collision for managed prefix [{schema}].[{prefix}] between [{owner}] and [{requested}]")]
    PrefixCollision {
        schema: String,
        prefix: String,
        owner: String,
        requested: String,
    },
    #[error(
        "cannot prove ownership for [{schema}].[{prefix}] because managed objects exist \
         without an ownership marker"
    )]
    UnmarkedObjects { schema: String, prefix: String },
    #[error("{0}")]
    Sqlite(String),
}

impl From<String> for SpatialIndexError {
    fn from(message: String) -> Self {
        Self::Sqlite(message)
    }
}

/// Reads column `column` of every row `sql` returns, skipping NULLs.
unsafe fn query_text_column(
    db: *mut sqlite3,
    sql: &str,
    column: c_int,
) -> std::result::Result<Vec<String>, String> {
    unsafe {
        let c_sql = sql_to_cstring(sql)
            .map_err(|_| "internal error: generated SQL contains NUL byte".to_string())?;
        let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
        let rc = host_api::prepare_v2(db, c_sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
        if rc != SQLITE_OK {
            return Err(CStr::from_ptr(host_api::errmsg(db))
                .to_string_lossy()
                .into_owned());
        }

        let mut values = Vec::new();
        loop {
            let step = host_api::step(stmt);
            if step == SQLITE_ROW {
                // `sqlite3_column_text` returns a null pointer for SQL NULL.
                let ptr = host_api::column_text(stmt, column);
                if !ptr.is_null() {
                    values.push(CStr::from_ptr(ptr.cast()).to_string_lossy().into_owned());
                }
                continue;
            }
            if step == SQLITE_DONE {
                break;
            }
            let err = CStr::from_ptr(host_api::errmsg(db))
                .to_string_lossy()
                .into_owned();
            let _ = host_api::finalize(stmt);
            return Err(err);
        }

        let _ = host_api::finalize(stmt);
        Ok(values)
    }
}

/// Schemas in the order SQLite searches them for an unqualified name: temp,
/// main, then attached schemas in attach order.
unsafe fn schema_search_order(db: *mut sqlite3) -> std::result::Result<Vec<String>, String> {
    unsafe {
        let attached = query_text_column(db, "PRAGMA database_list", 1)?;
        let mut order = vec!["temp".to_owned(), "main".to_owned()];
        order.extend(
            attached
                .into_iter()
                .filter(|name| name != "main" && name != "temp"),
        );
        Ok(order)
    }
}

/// Returns the attached spelling of `schema`, which SQLite matches
/// case-insensitively.
unsafe fn canonical_schema(
    db: *mut sqlite3,
    schema: &str,
) -> std::result::Result<String, SpatialIndexError> {
    unsafe {
        schema_search_order(db)?
            .into_iter()
            .find(|name| name.eq_ignore_ascii_case(schema))
            .ok_or_else(|| SpatialIndexError::UnknownSchema(schema.to_owned()))
    }
}

/// Whether `schema` holds a table named `table`. A view of that name is an
/// error, so a bare name never skips past the relation SQLite would read.
unsafe fn schema_holds_table(
    db: *mut sqlite3,
    schema: &str,
    table: &str,
) -> std::result::Result<bool, SpatialIndexError> {
    unsafe {
        let sql = format!(
            "SELECT type FROM [{schema}].sqlite_master \
             WHERE type IN ('table', 'view') AND name = '{table}' COLLATE NOCASE LIMIT 1"
        );
        match sqlite_master_lookup_text(db, &sql)? {
            None => Ok(false),
            Some(object_type) if object_type == "table" => Ok(true),
            Some(object_type) => Err(SpatialIndexError::NotATable {
                schema: schema.to_owned(),
                table: table.to_owned(),
                object_type,
            }),
        }
    }
}

/// Picks the schema whose `table` a `CreateSpatialIndex` call indexes: the
/// named schema, or the first schema in SQLite's search order holding a
/// relation of that name.
unsafe fn resolve_create_owner(
    db: *mut sqlite3,
    schema: Option<&str>,
    table: &str,
) -> std::result::Result<String, SpatialIndexError> {
    unsafe {
        if let Some(schema) = schema {
            let schema = canonical_schema(db, schema)?;
            if schema_holds_table(db, &schema, table)? {
                return Ok(schema);
            }
            return Err(SpatialIndexError::NoSuchQualifiedTable {
                schema,
                table: table.to_owned(),
            });
        }
        for candidate in schema_search_order(db)? {
            if schema_holds_table(db, &candidate, table)? {
                return Ok(candidate);
            }
        }
        Err(SpatialIndexError::NoSuchTable(table.to_owned()))
    }
}

/// Picks the schema a `DropSpatialIndex` call clears: the named schema, or
/// the first schema in SQLite's search order still holding a catalogue row
/// or a managed object for `prefix`. `None` means no schema holds any.
unsafe fn resolve_drop_owner(
    db: *mut sqlite3,
    schema: Option<&str>,
    prefix: &str,
) -> std::result::Result<Option<String>, SpatialIndexError> {
    unsafe {
        if let Some(schema) = schema {
            return canonical_schema(db, schema).map(Some);
        }
        for candidate in schema_search_order(db)? {
            if managed_spatial_index_objects_exist(db, &candidate, prefix)?
                || lookup_spatial_index_catalog_owner(db, &candidate, prefix)?.is_some()
            {
                return Ok(Some(candidate));
            }
        }
        Ok(None)
    }
}

unsafe fn lookup_sqlite_master_object_type(
    db: *mut sqlite3,
    schema: &str,
    object_name: &str,
) -> std::result::Result<Option<String>, String> {
    unsafe {
        let sql = format!(
            "SELECT type FROM [{schema}].sqlite_master WHERE name = '{object_name}' LIMIT 1"
        );
        sqlite_master_lookup_text(db, &sql)
    }
}

/// Checks the catalogue of `schema`, which must be absent or a table with the
/// required columns. An absent catalogue is created when `create_missing` is
/// set.
unsafe fn check_spatial_index_catalog(
    db: *mut sqlite3,
    schema: &str,
    create_missing: bool,
) -> std::result::Result<(), SpatialIndexError> {
    unsafe {
        let object_type =
            lookup_sqlite_master_object_type(db, schema, SPATIAL_INDEX_CATALOG_TABLE)?;
        match object_type.as_deref() {
            None if create_missing => {
                let sql = format!(
                    "CREATE TABLE [{schema}].[{SPATIAL_INDEX_CATALOG_TABLE}] (\
                     prefix TEXT PRIMARY KEY, \
                     table_name TEXT NOT NULL, \
                     column_name TEXT NOT NULL, \
                     UNIQUE(table_name, column_name)\
                     )"
                );
                exec_sql(db, &sql)?;
                return Ok(());
            }
            None => return Ok(()),
            Some("table") => {}
            Some(object_type) => {
                return Err(SpatialIndexError::CatalogNotATable {
                    schema: schema.to_owned(),
                    object_type: object_type.to_owned(),
                });
            }
        }

        let sql = format!("PRAGMA [{schema}].table_info([{SPATIAL_INDEX_CATALOG_TABLE}])");
        let columns = query_text_column(db, &sql, 1)?;
        for required_column in SPATIAL_INDEX_CATALOG_REQUIRED_COLUMNS {
            if !columns.iter().any(|column| column == required_column) {
                return Err(SpatialIndexError::CatalogMissingColumn {
                    schema: schema.to_owned(),
                    column: required_column,
                });
            }
        }
        Ok(())
    }
}

/// Reads the `(table, column)` owning `prefix` from the catalogue of
/// `schema`. A schema without a catalogue owns nothing.
unsafe fn lookup_spatial_index_catalog_owner(
    db: *mut sqlite3,
    schema: &str,
    prefix: &str,
) -> std::result::Result<Option<(String, String)>, SpatialIndexError> {
    unsafe {
        if lookup_sqlite_master_object_type(db, schema, SPATIAL_INDEX_CATALOG_TABLE)?.is_none() {
            return Ok(None);
        }

        let sql = format!(
            "SELECT table_name FROM [{schema}].[{SPATIAL_INDEX_CATALOG_TABLE}] \
             WHERE prefix = '{prefix}' LIMIT 1"
        );
        let Some(owner_table) = sqlite_master_lookup_text(db, &sql)? else {
            return Ok(None);
        };

        let sql = format!(
            "SELECT column_name FROM [{schema}].[{SPATIAL_INDEX_CATALOG_TABLE}] \
             WHERE prefix = '{prefix}' LIMIT 1"
        );
        let owner_column = sqlite_master_lookup_text(db, &sql)?
            .ok_or_else(|| SpatialIndexError::CatalogRowIncomplete(prefix.to_owned()))?;
        Ok(Some((owner_table, owner_column)))
    }
}

unsafe fn managed_spatial_index_objects_exist(
    db: *mut sqlite3,
    schema: &str,
    prefix: &str,
) -> std::result::Result<bool, String> {
    unsafe {
        let rtree_name = format!("{prefix}_rtree");
        let sql = format!(
            "SELECT name FROM [{schema}].sqlite_master WHERE name IN (\
             '{rtree_name}', \
             '{rtree_name}_node', \
             '{rtree_name}_parent', \
             '{rtree_name}_rowid', \
             '{prefix}_insert', \
             '{prefix}_update', \
             '{prefix}_delete'\
             ) LIMIT 1"
        );
        Ok(sqlite_master_lookup_text(db, &sql)?.is_some())
    }
}

/// The managed object `{prefix}_rtree` must be either absent or a real
/// SQLite table backed by the expected R-tree shadow tables.
unsafe fn ensure_spatial_index_table_shape(
    db: *mut sqlite3,
    schema: &str,
    prefix: &str,
) -> std::result::Result<(), SpatialIndexError> {
    unsafe {
        let rtree_name = format!("{prefix}_rtree");
        let Some(object_type) = lookup_sqlite_master_object_type(db, schema, &rtree_name)? else {
            return Ok(());
        };
        if object_type != "table" {
            return Err(SpatialIndexError::RtreeNotATable {
                schema: schema.to_owned(),
                name: rtree_name,
                object_type,
            });
        }

        for shadow_suffix in ["_node", "_parent", "_rowid"] {
            let shadow_name = format!("{rtree_name}{shadow_suffix}");
            let sql = format!(
                "SELECT name FROM [{schema}].sqlite_master \
                 WHERE type = 'table' AND name = '{shadow_name}' LIMIT 1"
            );
            if sqlite_master_lookup_text(db, &sql)?.is_none() {
                return Err(SpatialIndexError::RtreeMissingShadow {
                    schema: schema.to_owned(),
                    name: rtree_name,
                    shadow: shadow_name,
                });
            }
        }
        Ok(())
    }
}

unsafe fn ensure_spatial_index_objects_owned_by_table(
    db: *mut sqlite3,
    schema: &str,
    table: &str,
    column: &str,
) -> std::result::Result<SpatialIndexOwnership, SpatialIndexError> {
    unsafe {
        // Object names are built as `{table}_{column}_...`. Different input pairs
        // can collide (e.g. `a_b`+`c` vs `a`+`b_c`). Detect and fail fast instead
        // of silently reusing another table's triggers/index objects.
        let prefix = format!("{table}_{column}");
        for suffix in SPATIAL_INDEX_TRIGGER_SUFFIXES {
            let trigger = format!("{prefix}{suffix}");
            let sql = format!(
                "SELECT tbl_name FROM [{schema}].sqlite_master \
                 WHERE type = 'trigger' AND name = '{trigger}' LIMIT 1"
            );
            if let Some(owner) = sqlite_master_lookup_text(db, &sql)? {
                if owner != table {
                    return Err(SpatialIndexError::TriggerCollision {
                        schema: schema.to_owned(),
                        trigger,
                        owner,
                        table: table.to_owned(),
                    });
                }
            }
        }

        ensure_spatial_index_table_shape(db, schema, &prefix)?;

        if let Some((owner_table, owner_column)) =
            lookup_spatial_index_catalog_owner(db, schema, &prefix)?
        {
            if owner_table == table && owner_column == column {
                return Ok(SpatialIndexOwnership::Owned);
            }
            return Err(SpatialIndexError::PrefixCollision {
                schema: schema.to_owned(),
                prefix,
                owner: format!("{owner_table}.{owner_column}"),
                requested: format!("{table}.{column}"),
            });
        }

        if managed_spatial_index_objects_exist(db, schema, &prefix)? {
            return Err(SpatialIndexError::UnmarkedObjects {
                schema: schema.to_owned(),
                prefix,
            });
        }

        Ok(SpatialIndexOwnership::Absent)
    }
}

/// Indexes `table.column` in the schema `resolve_create_owner` picks,
/// replacing the R-tree contents when the index already exists.
unsafe fn create_spatial_index(
    db: *mut sqlite3,
    args: SpatialIndexArgs<'_>,
) -> std::result::Result<(), SpatialIndexError> {
    unsafe {
        let SpatialIndexArgs {
            schema,
            table,
            column,
        } = args;
        let schema = resolve_create_owner(db, schema, table)?;
        let prefix = format!("{table}_{column}");
        let rtree = format!("{prefix}_rtree");

        check_spatial_index_catalog(db, &schema, true)?;
        ensure_spatial_index_objects_owned_by_table(db, &schema, table, column)?;

        // Reject WITHOUT ROWID tables before creating any state. The
        // maintenance triggers reference NEW.rowid and OLD.rowid, which only
        // exist on regular rowid tables. SQLite refuses to prepare a SELECT
        // of rowid against a WITHOUT ROWID table, so the probe fails cleanly
        // at parse time.
        let probe = format!("SELECT rowid FROM [{schema}].[{table}] LIMIT 0");
        if exec_sql(db, &probe).is_err() {
            return Err(SpatialIndexError::WithoutRowid {
                schema,
                table: table.to_owned(),
            });
        }

        // 1. Create the R-tree virtual table if missing.
        let sql = format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS [{schema}].[{rtree}] \
             USING rtree(id, xmin, xmax, ymin, ymax)"
        );
        exec_sql(db, &sql)?;

        // 2. Rebuild index contents from the base table (idempotent on repeated calls).
        exec_sql(db, &format!("DELETE FROM [{schema}].[{rtree}]"))?;
        let sql = format!(
            "INSERT INTO [{schema}].[{rtree}] \
             SELECT rowid, ST_XMin([{column}]), ST_XMax([{column}]), \
             ST_YMin([{column}]), ST_YMax([{column}]) \
             FROM [{schema}].[{table}] \
             WHERE [{column}] IS NOT NULL AND ST_IsEmpty([{column}]) = 0"
        );
        exec_sql(db, &sql)?;

        // The trigger name carries the schema, and SQLite binds the ON
        // table to it. Trigger bodies cannot qualify table names. A
        // non-TEMP trigger body only reaches its own schema, and a TEMP
        // trigger body resolves temp first, where the R-tree was just
        // created, so every trigger writes the R-tree of its own schema.

        // 3. AFTER INSERT trigger
        let sql = format!(
            "CREATE TRIGGER IF NOT EXISTS [{schema}].[{prefix}_insert] \
             AFTER INSERT ON [{table}] \
             WHEN NEW.[{column}] IS NOT NULL AND ST_IsEmpty(NEW.[{column}]) = 0 \
             BEGIN \
               INSERT INTO [{rtree}] VALUES ( \
                 NEW.rowid, \
                 ST_XMin(NEW.[{column}]), ST_XMax(NEW.[{column}]), \
                 ST_YMin(NEW.[{column}]), ST_YMax(NEW.[{column}]) \
               ); \
             END"
        );
        exec_sql(db, &sql)?;

        // 4. AFTER UPDATE trigger. Broad UPDATE so that rowid changes
        // (UPDATE ... SET rowid = ... or via INTEGER PRIMARY KEY rewrite)
        // still propagate to the index. The WHEN clause skips the DELETE
        // plus INSERT when neither the geometry blob nor the rowid changed,
        // which is the common case for UPDATEs that only touch unrelated
        // columns.
        let sql = format!(
            "CREATE TRIGGER IF NOT EXISTS [{schema}].[{prefix}_update] \
             AFTER UPDATE ON [{table}] \
             WHEN OLD.[{column}] IS NOT NEW.[{column}] OR OLD.rowid IS NOT NEW.rowid \
             BEGIN \
               DELETE FROM [{rtree}] WHERE id = OLD.rowid; \
               INSERT INTO [{rtree}] \
                 SELECT NEW.rowid, \
                   ST_XMin(NEW.[{column}]), ST_XMax(NEW.[{column}]), \
                   ST_YMin(NEW.[{column}]), ST_YMax(NEW.[{column}]) \
                 WHERE NEW.[{column}] IS NOT NULL AND ST_IsEmpty(NEW.[{column}]) = 0; \
             END"
        );
        exec_sql(db, &sql)?;

        // 5. AFTER DELETE trigger
        let sql = format!(
            "CREATE TRIGGER IF NOT EXISTS [{schema}].[{prefix}_delete] \
             AFTER DELETE ON [{table}] \
             BEGIN \
               DELETE FROM [{rtree}] WHERE id = OLD.rowid; \
             END"
        );
        exec_sql(db, &sql)?;

        let sql = format!(
            "INSERT INTO [{schema}].[{SPATIAL_INDEX_CATALOG_TABLE}] \
             (prefix, table_name, column_name) \
             VALUES ('{prefix}', '{table}', '{column}') \
             ON CONFLICT(prefix) DO UPDATE SET \
             table_name = excluded.table_name, \
             column_name = excluded.column_name"
        );
        exec_sql(db, &sql)?;
        Ok(())
    }
}

/// Removes the index of `table.column` from the schema `resolve_drop_owner`
/// picks. Succeeds without changes when no schema holds the index.
unsafe fn drop_spatial_index(
    db: *mut sqlite3,
    args: SpatialIndexArgs<'_>,
) -> std::result::Result<(), SpatialIndexError> {
    unsafe {
        let SpatialIndexArgs {
            schema,
            table,
            column,
        } = args;
        let prefix = format!("{table}_{column}");
        let Some(schema) = resolve_drop_owner(db, schema, &prefix)? else {
            return Ok(());
        };

        check_spatial_index_catalog(db, &schema, false)?;
        if ensure_spatial_index_objects_owned_by_table(db, &schema, table, column)?
            == SpatialIndexOwnership::Absent
        {
            return Ok(());
        }

        // Drop triggers first, then the R-tree table. Ownership has already
        // been verified to avoid cross-table collisions on derived names.
        for suffix in SPATIAL_INDEX_TRIGGER_SUFFIXES {
            let sql = format!("DROP TRIGGER IF EXISTS [{schema}].[{prefix}{suffix}]");
            exec_sql(db, &sql)?;
        }
        let sql = format!("DROP TABLE IF EXISTS [{schema}].[{prefix}_rtree]");
        exec_sql(db, &sql)?;
        let sql = format!(
            "DELETE FROM [{schema}].[{SPATIAL_INDEX_CATALOG_TABLE}] WHERE prefix = '{prefix}'"
        );
        exec_sql(db, &sql)?;
        Ok(())
    }
}

// Spatial index callbacks

/// Arguments of `CreateSpatialIndex` and `DropSpatialIndex`, called as
/// `(table, column)` or `(schema, table, column)`.
#[derive(Clone, Copy)]
struct SpatialIndexArgs<'a> {
    schema: Option<&'a str>,
    table: &'a str,
    column: &'a str,
}

/// Extracts the identifier argument `i`, named `kind` in error messages.
/// On failure, sets an error on `ctx` and returns `None`.
unsafe fn get_identifier_arg<'a>(
    ctx: *mut sqlite3_context,
    argv: *mut *mut sqlite3_value,
    i: usize,
    label: &str,
    kind: &str,
) -> Option<&'a str> {
    unsafe {
        let value = match get_text(argv, i) {
            SqlTextArg::Value(v) => v,
            SqlTextArg::Null => {
                set_error(ctx, &format!("{label}: {kind} name must not be NULL"));
                return None;
            }
            SqlTextArg::InvalidUtf8 => {
                set_error(
                    ctx,
                    &format!("{label}: {kind} name must be valid UTF-8 text"),
                );
                return None;
            }
        };
        let Some(value) = validate_identifier(value) else {
            set_error(
                ctx,
                &format!("{label}: invalid {kind} name (only [a-zA-Z0-9_] allowed)"),
            );
            return None;
        };
        Some(value)
    }
}

/// Extracts and validates the spatial index arguments for an `n_args` call.
/// On failure, sets an error on `ctx` and returns `None`.
unsafe fn get_spatial_index_args<'a>(
    ctx: *mut sqlite3_context,
    n_args: c_int,
    argv: *mut *mut sqlite3_value,
    label: &str,
) -> Option<SpatialIndexArgs<'a>> {
    unsafe {
        let (schema, first) = if n_args == 3 {
            (Some(get_identifier_arg(ctx, argv, 0, label, "schema")?), 1)
        } else {
            (None, 0)
        };
        let table = get_identifier_arg(ctx, argv, first, label, "table")?;
        let column = get_identifier_arg(ctx, argv, first + 1, label, "column")?;
        Some(SpatialIndexArgs {
            schema,
            table,
            column,
        })
    }
}

/// Runs one spatial index lifecycle call inside `savepoint`, reporting `1` or
/// the error prefixed with `label`.
unsafe fn spatial_index_xfunc(
    ctx: *mut sqlite3_context,
    n_args: c_int,
    argv: *mut *mut sqlite3_value,
    label: &str,
    savepoint: &str,
    body: unsafe fn(
        *mut sqlite3,
        SpatialIndexArgs<'_>,
    ) -> std::result::Result<(), SpatialIndexError>,
) {
    unsafe {
        xfunc_guard(ctx, label, || {
            let Some(args) = get_spatial_index_args(ctx, n_args, argv, label) else {
                return;
            };
            let db = host_api::context_db_handle(ctx);
            match run_in_savepoint(db, savepoint, || body(db, args)) {
                Ok(()) => set_i32(ctx, 1),
                Err(e) => set_error(ctx, &format!("{label}: {e}")),
            }
        });
    }
}

unsafe extern "C" fn create_spatial_index_xfunc(
    ctx: *mut sqlite3_context,
    n_args: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        spatial_index_xfunc(
            ctx,
            n_args,
            argv,
            "CreateSpatialIndex",
            "sqlitegis_create_spatial_index",
            create_spatial_index,
        );
    }
}

unsafe extern "C" fn drop_spatial_index_xfunc(
    ctx: *mut sqlite3_context,
    n_args: c_int,
    argv: *mut *mut sqlite3_value,
) {
    unsafe {
        spatial_index_xfunc(
            ctx,
            n_args,
            argv,
            "DropSpatialIndex",
            "sqlitegis_drop_spatial_index",
            drop_spatial_index,
        );
    }
}

// Registration

type XFunc = unsafe extern "C" fn(*mut sqlite3_context, c_int, *mut *mut sqlite3_value);

#[derive(Clone, Copy)]
struct SqliteCallbackSpec {
    name: &'static str,
    n_arg: i32,
    xfunc: XFunc,
}

macro_rules! callback_spec {
    ($name:literal, $n_arg:literal, $xfunc:ident) => {
        SqliteCallbackSpec {
            name: $name,
            n_arg: $n_arg,
            xfunc: $xfunc,
        }
    };
}

include!("deterministic_callbacks.rs");
include!("direct_only_callbacks.rs");

const fn const_str_eq(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }

    let mut i = 0;
    while i < a_bytes.len() {
        if a_bytes[i] != b_bytes[i] {
            return false;
        }
        i += 1;
    }
    true
}

const fn assert_catalog_callback_parity(
    catalog: &[SqliteFunctionSpec],
    callbacks: &[SqliteCallbackSpec],
) {
    assert!(catalog.len() == callbacks.len());
    let mut i = 0;
    while i < callbacks.len() {
        assert!(const_str_eq(callbacks[i].name, catalog[i].name));
        assert!(callbacks[i].n_arg == catalog[i].n_arg);
        i += 1;
    }
}

const _: () = assert_catalog_callback_parity(
    SQLITE_DETERMINISTIC_FUNCTIONS,
    SQLITE_DETERMINISTIC_CALLBACKS,
);
const _: () =
    assert_catalog_callback_parity(SQLITE_DIRECT_ONLY_FUNCTIONS, SQLITE_DIRECT_ONLY_CALLBACKS);

unsafe fn reg(db: *mut sqlite3, name: &str, n_arg: c_int, flags: c_int, xfunc: XFunc) -> c_int {
    unsafe {
        let c_name = match CString::new(name) {
            Ok(v) => v,
            Err(_) => return SQLITE_ERROR,
        };
        host_api::create_function_v2(
            db,
            c_name.as_ptr(),
            n_arg,
            flags,
            std::ptr::null_mut(),
            Some(xfunc),
            None,
            None,
            None,
        )
    }
}

/// Register all SQLiteGIS spatial functions into an open SQLite database.
///
/// Returns `SQLITE_OK` (0) on success, or the first error code on failure.
///
/// # Safety
/// `db` must be a valid, open SQLite database handle for the lifetime of the call.
///
/// # Example
///
/// Open an in-memory connection via the raw `libsqlite3-sys` FFI, register
/// the SQLiteGIS spatial functions, then call one from SQL:
///
/// ```
/// use std::ffi::{CStr, CString};
/// use std::ptr;
/// use libsqlite3_sys::{
///     sqlite3, sqlite3_close, sqlite3_column_text, sqlite3_finalize,
///     sqlite3_open, sqlite3_prepare_v2, sqlite3_step,
///     SQLITE_OK, SQLITE_ROW,
/// };
///
/// unsafe {
///     let mut db: *mut sqlite3 = ptr::null_mut();
///     let path = CString::new(":memory:").unwrap();
///     assert_eq!(sqlite3_open(path.as_ptr(), &mut db), SQLITE_OK);
///
///     // Wire SQLiteGIS's spatial functions into this connection.
///     assert_eq!(sqlitegis::sqlite::register_functions(db), SQLITE_OK);
///
///     // ST_AsText, ST_Point, and the rest of the catalogue are now
///     // callable from any prepared statement on this handle.
///     let sql = CString::new("SELECT ST_AsText(ST_Point(1.0, 2.0, 4326))").unwrap();
///     let mut stmt = ptr::null_mut();
///     assert_eq!(
///         sqlite3_prepare_v2(db, sql.as_ptr(), -1, &mut stmt, ptr::null_mut()),
///         SQLITE_OK,
///     );
///     assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
///     let text = CStr::from_ptr(sqlite3_column_text(stmt, 0).cast());
///     assert_eq!(text.to_str().unwrap(), "POINT(1 2)");
///     sqlite3_finalize(stmt);
///     sqlite3_close(db);
/// }
/// ```
pub unsafe fn register_functions(db: *mut sqlite3) -> c_int {
    unsafe {
        for callback in SQLITE_DETERMINISTIC_CALLBACKS {
            let rc = reg(
                db,
                callback.name,
                callback.n_arg as c_int,
                DET,
                callback.xfunc,
            );
            if rc != SQLITE_OK {
                return rc;
            }
        }

        for callback in SQLITE_DIRECT_ONLY_CALLBACKS {
            let rc = reg(
                db,
                callback.name,
                callback.n_arg as c_int,
                DIRECT,
                callback.xfunc,
            );
            if rc != SQLITE_OK {
                return rc;
            }
        }

        SQLITE_OK
    }
}

/// Register SQLiteGIS as a SQLite auto-extension: from the next call onward,
/// every new SQLite connection opened in this process has the SQLiteGIS
/// spatial functions registered on it automatically.
///
/// This is the recommended way to wire SQLiteGIS into [Diesel]'s
/// `SqliteConnection::establish(...)` flow, which does not give the caller a
/// raw `*mut sqlite3` to pass to [`register_functions`] directly.
///
/// Calling the function more than once is a no-op: a `std::sync::Once`
/// guarantees the underlying `sqlite3_auto_extension` registration happens
/// at most once per process.
///
/// Native targets only. On wasm32 the connection lifecycle is owned by
/// `sqlite-wasm-rs` and you should call [`register_functions`] from your
/// own `auto_extension` shim once per connection instead.
///
/// Returns `SQLITE_OK` on the first call, and `SQLITE_OK` on every
/// subsequent call too. A non-zero return from this function should be
/// treated as a SQLite-internal failure.
///
/// [Diesel]: https://diesel.rs/
///
/// ```
/// use diesel::{Connection, RunQueryDsl, sqlite::SqliteConnection};
/// use diesel::deserialize::QueryableByName;
/// use diesel::sql_types::Text;
///
/// sqlitegis::sqlite::register_on_every_new_connection();
/// let mut c = SqliteConnection::establish(":memory:").unwrap();
///
/// #[derive(QueryableByName)]
/// struct R { #[diesel(sql_type = Text)] wkt: String }
///
/// let row: R = diesel::sql_query(
///     "SELECT ST_AsText(ST_Point(1.0, 2.0, 4326)) AS wkt",
/// ).get_result(&mut c).unwrap();
/// assert_eq!(row.wkt, "POINT(1 2)");
/// ```
pub fn register_on_every_new_connection() -> c_int {
    use std::sync::Once;
    static INIT: Once = Once::new();
    // Direct call, not routed through `host_api`: this registers against the
    // SQLite this library is linked to, for Rust callers who share it. A
    // loadable extension never reaches here.
    INIT.call_once(|| unsafe {
        sqlite3_auto_extension(Some(sqlitegis_auto_extension_init));
    });
    SQLITE_OK
}

unsafe extern "C" fn sqlitegis_auto_extension_init(
    db: *mut sqlite3,
    _pz_err_msg: *mut *mut std::os::raw::c_char,
    _p_api: *const sqlite3_api_routines,
) -> c_int {
    unsafe { register_functions(db) }
}

// C entry point for loadable extension (native only)
//
// Gated on feature = "sqlite-extension" so consumers that enable only the
// in-process `sqlite` feature do not silently re-export `sqlite3_sqlitegis_init`
// from their own binaries. That would collide with anyone else embedding
// the SQLiteGIS extension at the C ABI level.

/// `sqlite3_sqlitegis_init` is the entry point called by SQLite when loading
/// this library as a loadable extension (`SELECT load_extension('libsqlitegis')`).
#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
#[no_mangle]
pub unsafe extern "C" fn sqlite3_sqlitegis_init(
    db: *mut sqlite3,
    _pz_err_msg: *mut *mut std::ffi::c_char,
    p_api: *mut sqlite3_api_routines,
) -> c_int {
    unsafe {
        // Adopt the loader's own SQLite before touching `db`, which belongs
        // to it and not to whatever `libsqlite3` this library links.
        host_api::install(p_api);
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| register_functions(db))) {
            Ok(rc) => rc,
            Err(_) => SQLITE_ERROR,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::function_catalog::{SemanticCase, SemanticExpectation};
    use std::ffi::{CStr, CString};
    use std::ptr;

    unsafe extern "C" fn guarded_constant_xfunc(
        ctx: *mut sqlite3_context,
        _n: c_int,
        _argv: *mut *mut sqlite3_value,
    ) {
        unsafe {
            xfunc_guard(ctx, "GuardedConstant", || {
                set_i32(ctx, 7);
            });
        }
    }

    unsafe extern "C" fn guarded_panic_xfunc(
        ctx: *mut sqlite3_context,
        _n: c_int,
        _argv: *mut *mut sqlite3_value,
    ) {
        unsafe {
            xfunc_guard(ctx, "GuardedPanic", || {
                panic!("boom");
            });
        }
    }

    unsafe fn open_db() -> *mut sqlite3 {
        unsafe {
            let mut db = ptr::null_mut();
            let path = CString::new(":memory:").expect("valid sqlite path");
            assert_eq!(sqlite3_open(path.as_ptr(), &mut db), SQLITE_OK);
            db
        }
    }

    unsafe fn close_db(db: *mut sqlite3) {
        unsafe {
            assert_eq!(sqlite3_close(db), SQLITE_OK);
        }
    }

    unsafe fn query_i64(db: *mut sqlite3, sql: &str) -> Result<i64, String> {
        unsafe {
            let sql_c = CString::new(sql).expect("valid SQL");
            let mut stmt = ptr::null_mut();
            let rc = sqlite3_prepare_v2(db, sql_c.as_ptr(), -1, &mut stmt, ptr::null_mut());
            if rc != SQLITE_OK {
                let err = CStr::from_ptr(sqlite3_errmsg(db))
                    .to_string_lossy()
                    .into_owned();
                return Err(err);
            }

            let step = sqlite3_step(stmt);
            if step != SQLITE_ROW {
                sqlite3_finalize(stmt);
                let err = CStr::from_ptr(sqlite3_errmsg(db))
                    .to_string_lossy()
                    .into_owned();
                return Err(err);
            }

            let value = sqlite3_column_int64(stmt, 0);
            sqlite3_finalize(stmt);
            Ok(value)
        }
    }

    #[derive(Debug)]
    enum QueryValue {
        Null,
        Integer(i64),
        Float(f64),
        Text(String),
        Blob(Vec<u8>),
    }

    unsafe fn query_value(db: *mut sqlite3, sql: &str) -> Result<QueryValue, String> {
        unsafe {
            let sql_c = CString::new(sql).expect("valid SQL");
            let mut stmt = ptr::null_mut();
            let rc = sqlite3_prepare_v2(db, sql_c.as_ptr(), -1, &mut stmt, ptr::null_mut());
            if rc != SQLITE_OK {
                let err = CStr::from_ptr(sqlite3_errmsg(db))
                    .to_string_lossy()
                    .into_owned();
                return Err(err);
            }

            let step = sqlite3_step(stmt);
            if step != SQLITE_ROW {
                sqlite3_finalize(stmt);
                let err = CStr::from_ptr(sqlite3_errmsg(db))
                    .to_string_lossy()
                    .into_owned();
                return Err(err);
            }

            let value = match sqlite3_column_type(stmt, 0) {
                SQLITE_NULL => QueryValue::Null,
                SQLITE_INTEGER => QueryValue::Integer(sqlite3_column_int64(stmt, 0)),
                SQLITE_FLOAT => QueryValue::Float(sqlite3_column_double(stmt, 0)),
                SQLITE_TEXT => {
                    let ptr = sqlite3_column_text(stmt, 0);
                    if ptr.is_null() {
                        sqlite3_finalize(stmt);
                        return Err("unexpected NULL text pointer for SQLITE_TEXT".to_string());
                    }
                    QueryValue::Text(CStr::from_ptr(ptr.cast()).to_string_lossy().into_owned())
                }
                SQLITE_BLOB => {
                    let len = sqlite3_column_bytes(stmt, 0) as usize;
                    let ptr = sqlite3_column_blob(stmt, 0) as *const u8;
                    if len == 0 {
                        QueryValue::Blob(Vec::new())
                    } else if ptr.is_null() {
                        sqlite3_finalize(stmt);
                        return Err("unexpected NULL blob pointer for SQLITE_BLOB".to_string());
                    } else {
                        QueryValue::Blob(std::slice::from_raw_parts(ptr, len).to_vec())
                    }
                }
                other => {
                    sqlite3_finalize(stmt);
                    return Err(format!("unexpected SQLite type code: {other}"));
                }
            };

            sqlite3_finalize(stmt);
            Ok(value)
        }
    }

    fn assert_semantic_expectation(
        spec: &SqliteFunctionSpec,
        case: &SemanticCase,
        result: Result<QueryValue, String>,
    ) {
        match case.expected {
            SemanticExpectation::Null => match result {
                Ok(QueryValue::Null) => {}
                Ok(got) => panic!(
                    "expected NULL for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
                Err(err) => panic!(
                    "expected NULL for {}({}) case `{}` via `{}`, got error: {err}",
                    spec.name, spec.n_arg, case.id, case.sql
                ),
            },
            SemanticExpectation::NumericFinite => match result {
                Ok(QueryValue::Integer(_)) => {}
                Ok(QueryValue::Float(v)) => {
                    assert!(
                        v.is_finite(),
                        "non-finite numeric for {}({}) case `{}` via `{}`",
                        spec.name,
                        spec.n_arg,
                        case.id,
                        case.sql
                    );
                }
                Ok(got) => panic!(
                    "expected numeric for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
                Err(err) => panic!(
                    "expected numeric for {}({}) case `{}` via `{}`, got error: {err}",
                    spec.name, spec.n_arg, case.id, case.sql
                ),
            },
            SemanticExpectation::TextNonEmpty => match result {
                Ok(QueryValue::Text(v)) => {
                    assert!(
                        !v.is_empty(),
                        "empty text result for {}({}) case `{}` via `{}`",
                        spec.name,
                        spec.n_arg,
                        case.id,
                        case.sql
                    );
                }
                Ok(got) => panic!(
                    "expected non-empty text for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
                Err(err) => panic!(
                    "expected non-empty text for {}({}) case `{}` via `{}`, got error: {err}",
                    spec.name, spec.n_arg, case.id, case.sql
                ),
            },
            SemanticExpectation::BlobNonEmpty => match result {
                Ok(QueryValue::Blob(v)) => {
                    assert!(
                        !v.is_empty(),
                        "empty blob result for {}({}) case `{}` via `{}`",
                        spec.name,
                        spec.n_arg,
                        case.id,
                        case.sql
                    );
                }
                Ok(got) => panic!(
                    "expected non-empty blob for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
                Err(err) => panic!(
                    "expected non-empty blob for {}({}) case `{}` via `{}`, got error: {err}",
                    spec.name, spec.n_arg, case.id, case.sql
                ),
            },
            SemanticExpectation::Bool01 => match result {
                Ok(QueryValue::Integer(v)) => {
                    assert!(
                        v == 0 || v == 1,
                        "bool result must be 0/1 for {}({}) case `{}` via `{}`, got {v}",
                        spec.name,
                        spec.n_arg,
                        case.id,
                        case.sql
                    );
                }
                Ok(got) => panic!(
                    "expected bool-as-int for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
                Err(err) => panic!(
                    "expected bool-as-int for {}({}) case `{}` via `{}`, got error: {err}",
                    spec.name, spec.n_arg, case.id, case.sql
                ),
            },
            SemanticExpectation::ErrorContains(expected_substring) => match result {
                Err(err) => assert!(
                    err.contains(expected_substring),
                    "error mismatch for {}({}) case `{}` via `{}`: expected substring `{}`, got `{}`",
                    spec.name,
                    spec.n_arg,
                    case.id,
                    case.sql,
                    expected_substring,
                    err
                ),
                Ok(got) => panic!(
                    "expected SQL error for {}({}) case `{}` via `{}`, got {:?}",
                    spec.name, spec.n_arg, case.id, case.sql, got
                ),
            },
        }
    }

    #[test]
    fn checked_c_int_len_accepts_small_and_boundary_values() {
        assert_eq!(checked_c_int_len(0), Some(0));
        assert_eq!(checked_c_int_len(1), Some(1));
        assert_eq!(checked_c_int_len(c_int::MAX as usize), Some(c_int::MAX));
    }

    #[test]
    fn checked_c_int_len_rejects_values_larger_than_c_int() {
        assert_eq!(checked_c_int_len((c_int::MAX as usize) + 1), None);
        assert_eq!(checked_c_int_len(usize::MAX), None);
    }

    #[test]
    fn sql_to_cstring_accepts_sql_without_nul() {
        let c_sql = sql_to_cstring("SELECT 1").expect("valid SQL should convert to CString");
        assert_eq!(c_sql.as_c_str().to_bytes(), b"SELECT 1");
    }

    #[test]
    fn sql_to_cstring_rejects_sql_with_nul() {
        assert!(sql_to_cstring("SELECT\0 1").is_err());
    }

    #[test]
    fn xfunc_guard_allows_normal_execution() {
        unsafe {
            let db = open_db();

            let func_name = CString::new("GuardedConstant").expect("valid function name");
            let rc = sqlite3_create_function_v2(
                db,
                func_name.as_ptr(),
                0,
                SQLITE_UTF8,
                ptr::null_mut(),
                Some(guarded_constant_xfunc),
                None,
                None,
                None,
            );
            assert_eq!(rc, SQLITE_OK, "function registration should succeed");

            let value = query_i64(db, "SELECT GuardedConstant()").expect("query should succeed");
            assert_eq!(value, 7);

            close_db(db);
        }
    }

    #[test]
    fn xfunc_guard_converts_panic_into_sqlite_error() {
        unsafe {
            let db = open_db();

            let func_name = CString::new("GuardedPanic").expect("valid function name");
            let rc = sqlite3_create_function_v2(
                db,
                func_name.as_ptr(),
                0,
                SQLITE_UTF8,
                ptr::null_mut(),
                Some(guarded_panic_xfunc),
                None,
                None,
                None,
            );
            assert_eq!(rc, SQLITE_OK, "function registration should succeed");

            let err = query_i64(db, "SELECT GuardedPanic()")
                .expect_err("panic should be surfaced as SQL error");
            assert!(
                err.contains("panic in SQLite callback"),
                "unexpected error message: {err}"
            );

            close_db(db);
        }
    }

    #[test]
    fn register_functions_semantic_smoke_covers_full_catalog() {
        unsafe {
            let db = open_db();

            let rc = register_functions(db);
            assert_eq!(rc, SQLITE_OK, "register_functions should succeed");

            for spec in SQLITE_DETERMINISTIC_FUNCTIONS {
                for case in spec.semantic_cases {
                    let result = query_value(db, case.sql);
                    assert_semantic_expectation(spec, case, result);
                }
            }

            close_db(db);
        }
    }

    #[test]
    fn register_functions_direct_only_semantic_smoke() {
        unsafe {
            let db = open_db();

            let rc = register_functions(db);
            assert_eq!(rc, SQLITE_OK, "register_functions should succeed");

            assert_eq!(exec_sql(db, "CREATE TABLE _rt(geom BLOB)"), Ok(()));

            for spec in SQLITE_DIRECT_ONLY_FUNCTIONS {
                for case in spec.semantic_cases {
                    let result = query_value(db, case.sql);
                    assert_semantic_expectation(spec, case, result);
                }
            }

            close_db(db);
        }
    }
}
