/// Generate a TestDb helper struct that wraps a raw `*mut sqlite3`.
///
/// Both native and WASM integration tests include this macro to avoid
/// duplicating ~100 lines of test infrastructure. The only difference
/// between the two is the FFI crate (`libsqlite3_sys` vs `sqlite_wasm_rs`).
#[allow(unused_macros)]
macro_rules! define_test_db {
    ($name:ident) => {
        struct $name(*mut sqlite3);

        impl $name {
            fn open() -> Self {
                let mut db = std::ptr::null_mut();
                let path = CString::new(":memory:").unwrap();
                unsafe {
                    assert_eq!(SQLITE_OK, sqlite3_open(path.as_ptr(), &mut db));
                    assert_eq!(SQLITE_OK, geolite::sqlite::register_functions(db));
                }
                $name(db)
            }

            unsafe fn query_row<T, F: Fn(*mut sqlite3_stmt) -> T>(
                &self,
                sql: &str,
                extract: F,
            ) -> T {
                let sql_c = CString::new(sql).unwrap();
                let mut stmt = std::ptr::null_mut();
                let rc =
                    sqlite3_prepare_v2(self.0, sql_c.as_ptr(), -1, &mut stmt, std::ptr::null_mut());
                assert_eq!(SQLITE_OK, rc, "prepare failed for: {sql}");
                let step = sqlite3_step(stmt);
                assert_eq!(SQLITE_ROW, step, "step failed for: {sql}");
                let val = extract(stmt);
                sqlite3_finalize(stmt);
                val
            }

            fn query_text(&self, sql: &str) -> String {
                unsafe {
                    self.query_row(sql, |stmt| {
                        let ptr = sqlite3_column_text(stmt, 0);
                        CStr::from_ptr(ptr as _).to_string_lossy().into_owned()
                    })
                }
            }

            fn query_f64(&self, sql: &str) -> f64 {
                unsafe { self.query_row(sql, |stmt| sqlite3_column_double(stmt, 0)) }
            }

            fn query_i64(&self, sql: &str) -> i64 {
                unsafe { self.query_row(sql, |stmt| sqlite3_column_int64(stmt, 0)) }
            }

            fn query_is_null(&self, sql: &str) -> bool {
                unsafe { self.query_row(sql, |stmt| sqlite3_column_type(stmt, 0) == SQLITE_NULL) }
            }

            fn exec(&self, sql: &str) {
                let sql_c = CString::new(sql).unwrap();
                unsafe {
                    let rc = sqlite3_exec(
                        self.0,
                        sql_c.as_ptr(),
                        None,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                    );
                    assert_eq!(SQLITE_OK, rc, "exec failed for: {sql}");
                }
            }

            fn query_all_i64(&self, sql: &str) -> Vec<i64> {
                let sql_c = CString::new(sql).unwrap();
                unsafe {
                    let mut stmt = std::ptr::null_mut();
                    let rc = sqlite3_prepare_v2(
                        self.0,
                        sql_c.as_ptr(),
                        -1,
                        &mut stmt,
                        std::ptr::null_mut(),
                    );
                    assert_eq!(SQLITE_OK, rc, "prepare failed for: {sql}");
                    let mut vals = Vec::new();
                    while sqlite3_step(stmt) == SQLITE_ROW {
                        vals.push(sqlite3_column_int64(stmt, 0));
                    }
                    sqlite3_finalize(stmt);
                    vals
                }
            }

            fn try_query_i64(&self, sql: &str) -> Result<i64, String> {
                let sql_c = CString::new(sql).unwrap();
                unsafe {
                    let mut stmt = std::ptr::null_mut();
                    let rc = sqlite3_prepare_v2(
                        self.0,
                        sql_c.as_ptr(),
                        -1,
                        &mut stmt,
                        std::ptr::null_mut(),
                    );
                    if rc != SQLITE_OK {
                        let err = sqlite3_errmsg(self.0);
                        return Err(CStr::from_ptr(err).to_string_lossy().into_owned());
                    }
                    let step = sqlite3_step(stmt);
                    if step != SQLITE_ROW {
                        sqlite3_finalize(stmt);
                        let err = sqlite3_errmsg(self.0);
                        return Err(CStr::from_ptr(err).to_string_lossy().into_owned());
                    }
                    let val = sqlite3_column_int64(stmt, 0);
                    sqlite3_finalize(stmt);
                    Ok(val)
                }
            }

            fn try_query_i64_with_f64_param(&self, sql: &str, value: f64) -> Result<i64, String> {
                let sql_c = CString::new(sql).unwrap();
                unsafe {
                    let mut stmt = std::ptr::null_mut();
                    let rc = sqlite3_prepare_v2(
                        self.0,
                        sql_c.as_ptr(),
                        -1,
                        &mut stmt,
                        std::ptr::null_mut(),
                    );
                    if rc != SQLITE_OK {
                        let err = sqlite3_errmsg(self.0);
                        return Err(CStr::from_ptr(err).to_string_lossy().into_owned());
                    }

                    let bind_rc = sqlite3_bind_double(stmt, 1, value);
                    if bind_rc != SQLITE_OK {
                        sqlite3_finalize(stmt);
                        let err = sqlite3_errmsg(self.0);
                        return Err(CStr::from_ptr(err).to_string_lossy().into_owned());
                    }

                    let step = sqlite3_step(stmt);
                    if step != SQLITE_ROW {
                        sqlite3_finalize(stmt);
                        let err = sqlite3_errmsg(self.0);
                        return Err(CStr::from_ptr(err).to_string_lossy().into_owned());
                    }
                    let val = sqlite3_column_int64(stmt, 0);
                    sqlite3_finalize(stmt);
                    Ok(val)
                }
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                unsafe {
                    sqlite3_close(self.0);
                }
            }
        }
    };
}
