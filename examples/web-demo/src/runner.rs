//! Execute user-typed SQL against the live Diesel connection and read rows
//! generically. We use `LoadConnection::load` to get a cursor of dynamic
//! `Row`s, then read each cell by inspecting its `SqliteType` and calling the
//! matching `SqliteValue::read_*` accessor.
//!
//! For statements that don't produce rows (CREATE, INSERT, UPDATE, and so on)
//! Diesel's `load` still works. It just yields zero rows, so we follow up with
//! a `changes()` query to report affected row count.

use diesel::connection::{LoadConnection, SimpleConnection};
use diesel::deserialize::QueryableByName;
use diesel::row::{Field, Row};
use diesel::sql_types::BigInt;
use diesel::sqlite::{Sqlite, SqliteType};
use diesel::RunQueryDsl;

use crate::db;

#[derive(Clone, Debug, PartialEq)]
pub struct QueryRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum QueryOutcome {
    Rows {
        result: QueryRows,
        elapsed_ms: f64,
    },
    Affected {
        rows: i64,
        elapsed_ms: f64,
    },
    Error(String),
}

/// Run one or more SQL statements. `:lon` and `:lat` in the SQL are replaced
/// with the user's current position before execution, so a single template
/// follows the user across map clicks and geolocation updates. If the *last*
/// statement is a SELECT we return its rows. Otherwise we report the
/// affected-row count.
pub fn run(sql: &str, user_lon: f64, user_lat: f64) -> QueryOutcome {
    let sql_owned = sql
        .replace(":lon", &format!("{user_lon}"))
        .replace(":lat", &format!("{user_lat}"));
    let sql = sql_owned.as_str();

    let start = performance_now();

    let outcome = db::with_conn(|conn| -> Result<QueryOutcome, String> {
        // `diesel::sql_query` carries the whole script (SQLite handles `;`
        // chaining). We try `load` first so SELECT rows come through; if the
        // script produced no column headers we treat it as DDL/DML and report
        // affected-row count via `changes()`.
        //
        // The cursor holds a `&mut` borrow on `conn`, so we drain it inside a
        // nested scope and drop it before touching `conn` again.
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
            return Ok(QueryOutcome::Rows {
                result: QueryRows { columns, rows: data },
                elapsed_ms,
            });
        }

        // No row-producing statement. Re-run as batch to apply effects, then
        // ask SQLite how many rows the last statement touched.
        conn.batch_execute(sql).map_err(|e| format!("{e}"))?;
        let changes: ChangesRow = diesel::sql_query("SELECT changes() AS n")
            .get_result(conn)
            .map_err(|e| format!("changes(): {e}"))?;
        let elapsed_ms = performance_now() - start;
        Ok(QueryOutcome::Affected {
            rows: changes.n,
            elapsed_ms,
        })
    });

    match outcome {
        Ok(o) => o,
        Err(e) => QueryOutcome::Error(e),
    }
}

#[derive(QueryableByName)]
struct ChangesRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Pull `(lon, lat)` tuples out of a query result, if it exposed columns
/// named `lon` and `lat`. Used to highlight rows on the canvas.
pub fn extract_lonlat(result: &QueryRows) -> Vec<(f64, f64)> {
    let lon_idx = result.columns.iter().position(|c| c.eq_ignore_ascii_case("lon"));
    let lat_idx = result.columns.iter().position(|c| c.eq_ignore_ascii_case("lat"));
    let (Some(lon_idx), Some(lat_idx)) = (lon_idx, lat_idx) else {
        return Vec::new();
    };
    result
        .rows
        .iter()
        .filter_map(|row| {
            let lon = row.get(lon_idx)?.parse::<f64>().ok()?;
            let lat = row.get(lat_idx)?.parse::<f64>().ok()?;
            Some((lon, lat))
        })
        .collect()
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

fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
