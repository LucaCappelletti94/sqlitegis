//! Dispatch to the SQLite that actually owns the connection.
//!
//! A loadable extension cannot assume it resolves `sqlite3_*` to the same
//! SQLite as the process that loaded it. A host with the amalgamation
//! compiled in, loading an extension that binds the shared `libsqlite3`, ends
//! up with two libraries in one process, and handing one library's `sqlite3`
//! handle to the other's entry points crashes.
//!
//! SQLite's answer is the `sqlite3_api_routines` table passed to the
//! extension entry point, which C extensions install with
//! `SQLITE_EXTENSION_INIT2`. Once [`install`] has run, every call below goes
//! through that table. Otherwise it goes straight to the linked symbol, which
//! is the in-process case where the caller shares our SQLite by construction.
//!
//! `libsqlite3-sys` declares `sqlite3_api_routines` as an opaque type unless
//! its `loadable_extension` feature is on, and that feature rewrites every
//! `sqlite3_*` into a table lookup that panics when no table is installed,
//! which would break in-process registration. So the prefix of the table we
//! call is declared here instead. The table is append-only, so each index is
//! fixed for all time. The layout below was cross-checked against SQLite
//! 3.45.1's `sqlite3ext.h` and the bindings `libsqlite3-sys` 0.37 ships for
//! 3.51.3, which agree field for field.

use crate::sqlite::sqlite_compat::*;
use std::os::raw::{c_char, c_int, c_uchar, c_void};

/// The `xFunc`/`xStep` slot of `sqlite3_create_function_v2`.
type SqliteScalarFn =
    Option<unsafe extern "C" fn(*mut sqlite3_context, c_int, *mut *mut sqlite3_value)>;
/// The `xFinal` slot of `sqlite3_create_function_v2`.
type SqliteFinalFn = Option<unsafe extern "C" fn(*mut sqlite3_context)>;
/// The `xDestroy` slot of `sqlite3_create_function_v2`.
type SqliteDestroyFn = Option<unsafe extern "C" fn(*mut c_void)>;

#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
mod table {
    use super::*;
    use core::mem::offset_of;
    use core::sync::atomic::{AtomicPtr, Ordering};

    /// Prefix of `sqlite3_api_routines` up to the last entry we call.
    /// Untyped `_slots_*` runs stand in for entries we never touch and exist
    /// only to place the ones we do.
    #[repr(C)]
    pub(super) struct HostRoutines {
        pub(super) _slots_0: [*mut c_void; 36],
        pub(super) column_text:
            Option<unsafe extern "C" fn(stmt: *mut sqlite3_stmt, col: c_int) -> *const c_uchar>,
        pub(super) _slots_1: [*mut c_void; 1],
        pub(super) column_type:
            Option<unsafe extern "C" fn(stmt: *mut sqlite3_stmt, col: c_int) -> c_int>,
        pub(super) _slots_2: [*mut c_void; 14],
        pub(super) errmsg: Option<unsafe extern "C" fn(db: *mut sqlite3) -> *const c_char>,
        pub(super) _slots_3: [*mut c_void; 1],
        pub(super) exec: Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                sql: *const c_char,
                callback: sqlite3_callback,
                arg: *mut c_void,
                err: *mut *mut c_char,
            ) -> c_int,
        >,
        pub(super) _slots_4: [*mut c_void; 1],
        pub(super) finalize: Option<unsafe extern "C" fn(stmt: *mut sqlite3_stmt) -> c_int>,
        pub(super) free: Option<unsafe extern "C" fn(ptr: *mut c_void)>,
        pub(super) _slots_5: [*mut c_void; 19],
        pub(super) result_blob: Option<
            unsafe extern "C" fn(
                ctx: *mut sqlite3_context,
                blob: *const c_void,
                len: c_int,
                destructor: sqlite3_destructor_type,
            ),
        >,
        pub(super) result_double:
            Option<unsafe extern "C" fn(ctx: *mut sqlite3_context, value: f64)>,
        pub(super) result_error:
            Option<unsafe extern "C" fn(ctx: *mut sqlite3_context, msg: *const c_char, len: c_int)>,
        pub(super) _slots_6: [*mut c_void; 1],
        pub(super) result_int:
            Option<unsafe extern "C" fn(ctx: *mut sqlite3_context, value: c_int)>,
        pub(super) result_int64:
            Option<unsafe extern "C" fn(ctx: *mut sqlite3_context, value: sqlite_int64)>,
        pub(super) result_null: Option<unsafe extern "C" fn(ctx: *mut sqlite3_context)>,
        pub(super) result_text: Option<
            unsafe extern "C" fn(
                ctx: *mut sqlite3_context,
                text: *const c_char,
                len: c_int,
                destructor: sqlite3_destructor_type,
            ),
        >,
        pub(super) _slots_7: [*mut c_void; 8],
        pub(super) step: Option<unsafe extern "C" fn(stmt: *mut sqlite3_stmt) -> c_int>,
        pub(super) _slots_8: [*mut c_void; 7],
        pub(super) value_blob:
            Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> *const c_void>,
        pub(super) value_bytes: Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> c_int>,
        pub(super) _slots_9: [*mut c_void; 1],
        pub(super) value_double: Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> f64>,
        pub(super) _slots_10: [*mut c_void; 1],
        pub(super) value_int64:
            Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> sqlite_int64>,
        pub(super) _slots_11: [*mut c_void; 1],
        pub(super) value_text:
            Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> *const c_uchar>,
        pub(super) _slots_12: [*mut c_void; 3],
        pub(super) value_type: Option<unsafe extern "C" fn(value: *mut sqlite3_value) -> c_int>,
        pub(super) _slots_13: [*mut c_void; 2],
        pub(super) prepare_v2: Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                sql: *const c_char,
                n_bytes: c_int,
                stmt: *mut *mut sqlite3_stmt,
                tail: *mut *const c_char,
            ) -> c_int,
        >,
        pub(super) _slots_14: [*mut c_void; 32],
        pub(super) context_db_handle:
            Option<unsafe extern "C" fn(ctx: *mut sqlite3_context) -> *mut sqlite3>,
        pub(super) _slots_15: [*mut c_void; 12],
        pub(super) create_function_v2: Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                name: *const c_char,
                n_arg: c_int,
                flags: c_int,
                app: *mut c_void,
                x_func: SqliteScalarFn,
                x_step: SqliteScalarFn,
                x_final: SqliteFinalFn,
                x_destroy: SqliteDestroyFn,
            ) -> c_int,
        >,
    }

    const PTR: usize = size_of::<*mut c_void>();

    // Guards the `_slots_*` arithmetic above: a miscounted run would silently
    // shift every later entry onto the wrong routine.
    const _: () = {
        assert!(offset_of!(HostRoutines, column_text) == 36 * PTR);
        assert!(offset_of!(HostRoutines, column_type) == 38 * PTR);
        assert!(offset_of!(HostRoutines, errmsg) == 53 * PTR);
        assert!(offset_of!(HostRoutines, exec) == 55 * PTR);
        assert!(offset_of!(HostRoutines, finalize) == 57 * PTR);
        assert!(offset_of!(HostRoutines, free) == 58 * PTR);
        assert!(offset_of!(HostRoutines, result_blob) == 78 * PTR);
        assert!(offset_of!(HostRoutines, result_double) == 79 * PTR);
        assert!(offset_of!(HostRoutines, result_error) == 80 * PTR);
        assert!(offset_of!(HostRoutines, result_int) == 82 * PTR);
        assert!(offset_of!(HostRoutines, result_int64) == 83 * PTR);
        assert!(offset_of!(HostRoutines, result_null) == 84 * PTR);
        assert!(offset_of!(HostRoutines, result_text) == 85 * PTR);
        assert!(offset_of!(HostRoutines, step) == 94 * PTR);
        assert!(offset_of!(HostRoutines, value_blob) == 102 * PTR);
        assert!(offset_of!(HostRoutines, value_bytes) == 103 * PTR);
        assert!(offset_of!(HostRoutines, value_double) == 105 * PTR);
        assert!(offset_of!(HostRoutines, value_int64) == 107 * PTR);
        assert!(offset_of!(HostRoutines, value_text) == 109 * PTR);
        assert!(offset_of!(HostRoutines, value_type) == 113 * PTR);
        assert!(offset_of!(HostRoutines, prepare_v2) == 116 * PTR);
        assert!(offset_of!(HostRoutines, context_db_handle) == 149 * PTR);
        assert!(offset_of!(HostRoutines, create_function_v2) == 162 * PTR);
    };

    static HOST: AtomicPtr<HostRoutines> = AtomicPtr::new(core::ptr::null_mut());

    /// Adopt the routine table SQLite handed the extension entry point.
    ///
    /// # Safety
    ///
    /// `p_api` must be the pointer SQLite passed to a loadable-extension
    /// entry point, which SQLite keeps alive for the life of the process.
    pub(crate) unsafe fn install(p_api: *mut sqlite3_api_routines) {
        HOST.store(p_api.cast::<HostRoutines>(), Ordering::Release);
    }

    pub(super) fn host() -> Option<&'static HostRoutines> {
        // SAFETY: the only writer is `install`, whose contract is that the
        // pointer stays valid for the life of the process.
        unsafe { HOST.load(Ordering::Acquire).as_ref() }
    }
}

#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
use table::host;
#[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
pub(crate) use table::install;

/// Declare a wrapper per routine: through the host table when one is
/// installed, otherwise straight to the linked symbol. The wrapper name has
/// to match the table field it reads.
macro_rules! routed {
    ($( fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)? = $linked:ident; )*) => {$(
        #[inline]
        // Arity is SQLite's, not ours: these mirror the C signatures exactly.
        #[allow(clippy::too_many_arguments)]
        pub(crate) unsafe fn $name($($arg: $ty),*) $(-> $ret)? {
            #[cfg(all(feature = "sqlite-extension", not(target_arch = "wasm32")))]
            if let Some(routine) = host().and_then(|r| r.$name) {
                // SAFETY: the table entry has the signature SQLite declares
                // for it, and the caller upholds the routine's own contract.
                return unsafe { routine($($arg),*) };
            }
            // SAFETY: delegated to the caller, who upholds the same contract
            // the linked symbol requires.
            unsafe { $linked($($arg),*) }
        }
    )*};
}

routed! {
    fn column_text(stmt: *mut sqlite3_stmt, col: c_int) -> *const c_uchar = sqlite3_column_text;
    fn column_type(stmt: *mut sqlite3_stmt, col: c_int) -> c_int = sqlite3_column_type;
    fn errmsg(db: *mut sqlite3) -> *const c_char = sqlite3_errmsg;
    fn exec(
        db: *mut sqlite3,
        sql: *const c_char,
        callback: sqlite3_callback,
        arg: *mut c_void,
        err: *mut *mut c_char,
    ) -> c_int = sqlite3_exec;
    fn finalize(stmt: *mut sqlite3_stmt) -> c_int = sqlite3_finalize;
    fn free(ptr: *mut c_void) = sqlite3_free;
    fn result_blob(
        ctx: *mut sqlite3_context,
        blob: *const c_void,
        len: c_int,
        destructor: sqlite3_destructor_type,
    ) = sqlite3_result_blob;
    fn result_double(ctx: *mut sqlite3_context, value: f64) = sqlite3_result_double;
    fn result_error(ctx: *mut sqlite3_context, msg: *const c_char, len: c_int) = sqlite3_result_error;
    fn result_int(ctx: *mut sqlite3_context, value: c_int) = sqlite3_result_int;
    fn result_int64(ctx: *mut sqlite3_context, value: sqlite_int64) = sqlite3_result_int64;
    fn result_null(ctx: *mut sqlite3_context) = sqlite3_result_null;
    fn result_text(
        ctx: *mut sqlite3_context,
        text: *const c_char,
        len: c_int,
        destructor: sqlite3_destructor_type,
    ) = sqlite3_result_text;
    fn step(stmt: *mut sqlite3_stmt) -> c_int = sqlite3_step;
    fn value_blob(value: *mut sqlite3_value) -> *const c_void = sqlite3_value_blob;
    fn value_bytes(value: *mut sqlite3_value) -> c_int = sqlite3_value_bytes;
    fn value_double(value: *mut sqlite3_value) -> f64 = sqlite3_value_double;
    fn value_int64(value: *mut sqlite3_value) -> sqlite_int64 = sqlite3_value_int64;
    fn value_text(value: *mut sqlite3_value) -> *const c_uchar = sqlite3_value_text;
    fn value_type(value: *mut sqlite3_value) -> c_int = sqlite3_value_type;
    fn prepare_v2(
        db: *mut sqlite3,
        sql: *const c_char,
        n_bytes: c_int,
        stmt: *mut *mut sqlite3_stmt,
        tail: *mut *const c_char,
    ) -> c_int = sqlite3_prepare_v2;
    fn context_db_handle(ctx: *mut sqlite3_context) -> *mut sqlite3 = sqlite3_context_db_handle;
    fn create_function_v2(
        db: *mut sqlite3,
        name: *const c_char,
        n_arg: c_int,
        flags: c_int,
        app: *mut c_void,
        x_func: SqliteScalarFn,
        x_step: SqliteScalarFn,
        x_final: SqliteFinalFn,
        x_destroy: SqliteDestroyFn,
    ) -> c_int = sqlite3_create_function_v2;
}
