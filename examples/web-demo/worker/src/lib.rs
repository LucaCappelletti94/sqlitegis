//! Dedicated wasm worker entrypoint for the SQLiteGIS browser demo.
//!
//! Owns the in-memory `SqliteConnection`, registers SQLiteGIS via
//! `sqlite3_auto_extension`, streams the cities5000 dataset into the
//! `places` table, and runs every chip query off the main thread so the
//! UI stays responsive.
//!
//! Message protocol lives in `sqlitegis_web_demo_protocol`. Encoding is
//! `serde_wasm_bindgen` (JS object <-> Rust struct).

#[cfg(target_arch = "wasm32")]
mod worker {
    use std::cell::RefCell;
    use std::sync::Once;

    use diesel::connection::{LoadConnection, SimpleConnection};
    use diesel::deserialize::QueryableByName;
    use diesel::prelude::*;
    use diesel::row::{Field, Row};
    use diesel::sql_types::BigInt;
    use diesel::sqlite::{Sqlite, SqliteConnection, SqliteType};
    use diesel::RunQueryDsl;
    use gloo_net::http::Request;
    use gloo_timers::future::TimeoutFuture;
    use js_sys::global;
    use sqlitegis_web_demo_protocol::{
        LoadReport, LonLat, QueryRows, WorkerRequest, WorkerResponse,
    };
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::prelude::wasm_bindgen;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::spawn_local;
    use web_sys::{DedicatedWorkerGlobalScope, MessageEvent};

    const BATCH_ROWS: usize = 1000;

    static INIT_AUTO_EXTENSION: Once = Once::new();

    thread_local! {
        static CONN: RefCell<Option<SqliteConnection>> = const { RefCell::new(None) };
    }

    /// Auto-extension callback. Registers every SQLiteGIS scalar /
    /// virtual-table-helper function on a freshly-opened connection.
    unsafe extern "C" fn sqlitegis_init(
        db: *mut sqlite_wasm_rs::sqlite3,
        _pz_err_msg: *mut *mut std::ffi::c_char,
        _p_api: *const sqlite_wasm_rs::sqlite3_api_routines,
    ) -> std::ffi::c_int {
        sqlitegis::sqlite::register_functions(db)
    }

    fn ensure_auto_extension() {
        INIT_AUTO_EXTENSION.call_once(|| unsafe {
            sqlite_wasm_rs::sqlite3_auto_extension(Some(sqlitegis_init));
        });
    }

    fn reopen() -> Result<(), String> {
        ensure_auto_extension();
        let conn = SqliteConnection::establish(":memory:").map_err(|e| e.to_string())?;
        CONN.with(|cell| *cell.borrow_mut() = Some(conn));
        Ok(())
    }

    fn with_conn<R>(f: impl FnOnce(&mut SqliteConnection) -> R) -> R {
        CONN.with(|cell| {
            let mut borrow = cell.borrow_mut();
            let conn = borrow.as_mut().expect("worker connection not opened yet");
            f(conn)
        })
    }

    fn run_script(sql: &str) -> Result<(), String> {
        with_conn(|c| c.batch_execute(sql).map_err(|e| e.to_string()))
    }

    fn performance_now() -> f64 {
        worker_scope().performance().map(|p| p.now()).unwrap_or(0.0)
    }

    fn worker_scope() -> DedicatedWorkerGlobalScope {
        global().unchecked_into::<DedicatedWorkerGlobalScope>()
    }

    fn post(response: &WorkerResponse) -> Result<(), JsValue> {
        let payload = serde_wasm_bindgen::to_value(response)
            .map_err(|error| JsValue::from_str(&format!("invalid worker response: {error}")))?;
        worker_scope().post_message(&payload)
    }

    fn post_error(token: u64, message: String) {
        let _ = post(&WorkerResponse::Error { token, message });
    }

    /// Worker entry. Wires up the message loop and announces `Ready`.
    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let _ = console_log::init_with_level(log::Level::Info);

        let onmessage = Closure::wrap(Box::new(move |event: MessageEvent| {
            let request = match serde_wasm_bindgen::from_value::<WorkerRequest>(event.data()) {
                Ok(request) => request,
                Err(error) => {
                    post_error(0, format!("invalid worker request: {error}"));
                    return;
                }
            };
            dispatch(request);
        }) as Box<dyn FnMut(MessageEvent)>);

        worker_scope().set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        let _ = post(&WorkerResponse::Ready);
        onmessage.forget();
    }

    fn dispatch(request: WorkerRequest) {
        match request {
            WorkerRequest::ApplySchema { token, sql } => {
                spawn_local(async move { handle_schema(token, sql).await });
            }
            WorkerRequest::LoadDataset { token, tsv_url } => {
                spawn_local(async move { handle_load(token, tsv_url).await });
            }
            WorkerRequest::RunQuery {
                token,
                sql,
                lon,
                lat,
            } => {
                spawn_local(async move { handle_query(token, sql, lon, lat).await });
            }
        }
    }

    async fn handle_schema(token: u64, sql: String) {
        if let Err(message) = reopen() {
            post_error(token, format!("opening database: {message}"));
            return;
        }
        if let Err(message) = run_script(&sql) {
            post_error(token, format!("applying schema: {message}"));
            return;
        }
        let _ = post(&WorkerResponse::SchemaApplied { token });
    }

    async fn handle_load(token: u64, tsv_url: String) {
        let start = performance_now();

        let body = match Request::get(&tsv_url).send().await {
            Ok(response) => match response.text().await {
                Ok(text) => text,
                Err(error) => {
                    post_error(token, format!("read dataset body: {error}"));
                    return;
                }
            },
            Err(error) => {
                post_error(token, format!("fetch {tsv_url}: {error}"));
                return;
            }
        };

        if let Err(message) = run_script("BEGIN;") {
            post_error(token, format!("BEGIN: {message}"));
            return;
        }

        let lines: Vec<&str> = body.lines().collect();
        let total = lines.len();
        let mut inserted = 0usize;
        let mut batch_sql = String::with_capacity(BATCH_ROWS * 140);
        let mut batch_coords: Vec<LonLat> = Vec::with_capacity(BATCH_ROWS);

        for chunk in lines.chunks(BATCH_ROWS) {
            batch_sql.clear();
            batch_coords.clear();
            for line in chunk {
                let mut cols = line.split('\t');
                let (Some(name), Some(country), Some(lat_s), Some(lon_s), Some(pop_s)) = (
                    cols.next(),
                    cols.next(),
                    cols.next(),
                    cols.next(),
                    cols.next(),
                ) else {
                    continue;
                };
                let (Ok(lat), Ok(lon)) = (lat_s.parse::<f64>(), lon_s.parse::<f64>()) else {
                    continue;
                };
                if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
                    continue;
                }
                let pop: i64 = pop_s.parse().unwrap_or(0);
                let name_esc = sql_escape(name);
                let country_lit = if country.is_empty() {
                    "NULL".to_string()
                } else {
                    format!("'{}'", sql_escape(country))
                };

                batch_sql.push_str(&format!(
                    "INSERT INTO places (name, country, population, geom) VALUES ('{name_esc}', {country_lit}, {pop}, ST_Point({lon}, {lat}, 4326));\n",
                ));
                batch_coords.push((lon, lat));
                inserted += 1;
            }

            if let Err(message) = run_script(&batch_sql) {
                post_error(token, format!("batch INSERT: {message}"));
                return;
            }
            let _ = post(&WorkerResponse::LoadProgress {
                token,
                inserted,
                total,
                batch: batch_coords.clone(),
            });
            // Yield so the worker's microtask queue stays responsive (lets
            // any incoming message land between batches).
            TimeoutFuture::new(0).await;
        }

        if let Err(message) = run_script("COMMIT;") {
            post_error(token, format!("COMMIT: {message}"));
            return;
        }

        let elapsed_ms = performance_now() - start;
        let _ = post(&WorkerResponse::LoadComplete {
            token,
            report: LoadReport {
                rows_inserted: inserted,
                elapsed_ms,
            },
        });
    }

    async fn handle_query(token: u64, sql: String, lon: f64, lat: f64) {
        let sql_owned = sql
            .replace(":lon", &format!("{lon}"))
            .replace(":lat", &format!("{lat}"));
        let sql = sql_owned.as_str();

        let start = performance_now();

        let outcome: Result<WorkerResponse, String> = with_conn(|conn| {
            // Try the row-producing path first so SELECTs come through.
            let captured: Result<(Option<Vec<String>>, Vec<Vec<String>>), String> = {
                let cursor = LoadConnection::<diesel::connection::DefaultLoadingMode>::load(
                    conn,
                    diesel::sql_query(sql),
                )
                .map_err(|e| format!("{e}"))?;

                let mut columns: Option<Vec<String>> = None;
                let mut data: Vec<Vec<String>> = Vec::new();
                for row_result in cursor {
                    let row = row_result.map_err(|e| format!("{e}"))?;
                    let count = <_ as Row<Sqlite>>::field_count(&row);

                    if columns.is_none() {
                        let mut hdr = Vec::with_capacity(count);
                        for i in 0..count {
                            let name = row
                                .get(i)
                                .and_then(|f| f.field_name().map(|s| s.to_string()))
                                .unwrap_or_default();
                            hdr.push(name);
                        }
                        columns = Some(hdr);
                    }

                    let mut row_strs = Vec::with_capacity(count);
                    for i in 0..count {
                        row_strs.push(render_field(&row, i));
                    }
                    data.push(row_strs);
                }
                Ok((columns, data))
            };

            let (columns, data) = captured?;

            if let Some(columns) = columns {
                let elapsed_ms = performance_now() - start;
                return Ok(WorkerResponse::QueryRows {
                    token,
                    result: QueryRows {
                        columns,
                        rows: data,
                    },
                    elapsed_ms,
                });
            }

            // DDL/DML path: apply effects, report affected-row count.
            conn.batch_execute(sql).map_err(|e| format!("{e}"))?;
            let changes: ChangesRow = diesel::sql_query("SELECT changes() AS n")
                .get_result(conn)
                .map_err(|e| format!("changes(): {e}"))?;
            let elapsed_ms = performance_now() - start;
            Ok(WorkerResponse::QueryAffected {
                token,
                rows: changes.n,
                elapsed_ms,
            })
        });

        match outcome {
            Ok(response) => {
                let _ = post(&response);
            }
            Err(message) => post_error(token, message),
        }
    }

    #[derive(QueryableByName)]
    struct ChangesRow {
        #[diesel(sql_type = BigInt)]
        n: i64,
    }

    fn render_field<'a, R: Row<'a, Sqlite>>(row: &R, idx: usize) -> String {
        let Some(field) = row.get(idx) else {
            return "NULL".into();
        };
        let Some(mut val) = field.value() else {
            return "NULL".into();
        };

        match val.value_type() {
            None => "NULL".into(),
            Some(SqliteType::Text) => val.read_text().to_string(),
            Some(SqliteType::Long | SqliteType::Integer | SqliteType::SmallInt) => {
                val.read_long().to_string()
            }
            Some(SqliteType::Double | SqliteType::Float) => format_double(val.read_double()),
            Some(SqliteType::Binary) => {
                let bytes = val.read_blob();
                format!("BLOB({} bytes)", bytes.len())
            }
        }
    }

    fn format_double(v: f64) -> String {
        if v.fract() == 0.0 && v.abs() < 1e16 {
            format!("{v:.0}")
        } else {
            format!("{v}")
        }
    }

    fn sql_escape(s: &str) -> String {
        s.replace('\'', "''")
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod worker {}
