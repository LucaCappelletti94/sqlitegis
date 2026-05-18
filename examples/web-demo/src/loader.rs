//! Fetches the slim GeoNames `cities5000.tsv` and bulk-INSERTs it into the
//! `places` table.
//!
//! Columns in the slim TSV are tab-separated, in this order:
//!     name<TAB>country_code<TAB>latitude<TAB>longitude<TAB>population
//!
//! Geometry is produced inside SQLite via `ST_Point(lon, lat, 4326)` so the
//! browser never has to construct EWKB itself.

use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;

use crate::db;

const ASSET_URL: &str = "/cities5000.tsv";
const BATCH_ROWS: usize = 1000;

#[derive(Clone, Debug, PartialEq)]
pub struct LoadReport {
    pub rows_inserted: usize,
    pub elapsed_ms: f64,
}

/// Fetch the dataset and apply it inside a single transaction.
///
/// `progress` is called after every batch with the cumulative row count, the
/// total expected rows, and the `(lon, lat)` pairs just inserted, so the UI
/// can draw an animated progress bar and paint the new points on the map.
pub async fn load_places(
    mut progress: impl FnMut(usize, usize, &[(f64, f64)]),
) -> Result<LoadReport, String> {
    let start = performance_now();

    let body = Request::get(ASSET_URL)
        .send()
        .await
        .map_err(|e| format!("fetch {ASSET_URL}: {e}"))?
        .text()
        .await
        .map_err(|e| format!("read body: {e}"))?;

    db::run_script("BEGIN;").map_err(|e| format!("BEGIN: {e}"))?;

    let lines: Vec<&str> = body.lines().collect();
    let total = lines.len();
    let mut inserted = 0usize;
    let mut batch_sql = String::with_capacity(BATCH_ROWS * 140);
    let mut batch_coords: Vec<(f64, f64)> = Vec::with_capacity(BATCH_ROWS);

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

        db::run_script(&batch_sql).map_err(|e| format!("batch INSERT: {e}"))?;
        progress(inserted, total, &batch_coords);
        // Yield to the event loop so Dioxus can re-render the bar and paint
        // the newly inserted points on the canvas.
        TimeoutFuture::new(0).await;
    }

    db::run_script("COMMIT;").map_err(|e| format!("COMMIT: {e}"))?;

    let elapsed_ms = performance_now() - start;
    Ok(LoadReport {
        rows_inserted: inserted,
        elapsed_ms,
    })
}

fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

fn performance_now() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}
