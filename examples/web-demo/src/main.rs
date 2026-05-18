//! geolite web demo. Runs Diesel queries through geolite's SQLite extension
//! entirely in the browser via sqlite-wasm-rs.

mod components;
mod db;
mod loader;
mod runner;
mod state;
mod viz;

use dioxus::prelude::*;

use crate::components::{QueryPanel, ResultsPanel, SchemaPanel};
use crate::runner::QueryOutcome;
use crate::viz::WorldMap;

const APP_CSS: Asset = asset!("/assets/app.css");

fn main() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
    dioxus::launch(App);
}

#[derive(Debug, Clone, PartialEq)]
enum LoadStage {
    Booting,
    LoadingData { inserted: usize, total: usize },
    Ready { rows: usize, elapsed_ms: f64 },
    Error(String),
}

#[component]
fn App() -> Element {
    let mut stage = use_signal(|| LoadStage::Booting);
    let mut all_coords = use_signal::<Vec<(f64, f64)>>(Vec::new);
    let mut highlighted = use_signal::<Vec<(f64, f64)>>(Vec::new);
    // NaN sentinel: signals become real numbers after the loader picks a
    // random starting city. The query auto-rerun effect skips while NaN.
    let mut user_lon = use_signal(|| f64::NAN);
    let mut user_lat = use_signal(|| f64::NAN);
    let mut outcome = use_signal::<Option<QueryOutcome>>(|| None);

    use_effect(move || {
        spawn(async move {
            if let Err(e) = db::reopen() {
                stage.set(LoadStage::Error(format!("opening database: {e}")));
                return;
            }
            if let Err(e) = db::run_script(state::DEFAULT_SCHEMA_SQL) {
                stage.set(LoadStage::Error(format!("applying schema: {e}")));
                return;
            }
            stage.set(LoadStage::LoadingData { inserted: 0, total: 0 });
            let loader_result = loader::load_places(|n, total, coords| {
                stage.set(LoadStage::LoadingData { inserted: n, total });
                // Append the newly inserted batch to the live coord set so
                // the canvas paints them on the next frame.
                all_coords.write().extend(coords.iter().copied());
            })
            .await;
            match loader_result {
                Ok(report) => {
                    log::info!(
                        "loaded {} cities in {:.0} ms",
                        report.rows_inserted,
                        report.elapsed_ms
                    );
                    // Pick a random loaded city as the starting probe point.
                    let coords = all_coords.peek().clone();
                    if !coords.is_empty() {
                        let idx = (js_sys::Math::random() * coords.len() as f64) as usize;
                        let (lon, lat) = coords[idx.min(coords.len() - 1)];
                        user_lon.set(lon);
                        user_lat.set(lat);
                    }
                    stage.set(LoadStage::Ready {
                        rows: report.rows_inserted,
                        elapsed_ms: report.elapsed_ms,
                    });
                }
                Err(e) => stage.set(LoadStage::Error(format!("loading data: {e}"))),
            }
        });
    });

    rsx! {
        document::Stylesheet { href: APP_CSS }
        main {
            header {
                h1 { "geolite" }
                p {
                    "PostGIS-style spatial SQL on top of SQLite, running entirely in your "
                    "browser via Diesel + sqlite-wasm-rs."
                }
            }
            StatusBanner { stage: stage.read().clone() }

            // Map is visible as soon as the DB is open so cities pop in as
            // they get inserted during the load.
            if !matches!(*stage.read(), LoadStage::Booting | LoadStage::Error(_)) {
                WorldMap {
                    coords: all_coords,
                    highlighted,
                    user_lon,
                    user_lat,
                }
            }

            if matches!(*stage.read(), LoadStage::Ready { .. }) {
                SchemaPanel {
                    on_reset: move |result: Result<(), String>| {
                        match result {
                            Ok(_) => {
                                outcome.set(None);
                                highlighted.set(Vec::new());
                                all_coords.set(Vec::new());
                            }
                            Err(e) => outcome.set(Some(QueryOutcome::Error(e))),
                        }
                    }
                }
                QueryPanel {
                    user_lon,
                    user_lat,
                    on_outcome: move |o: QueryOutcome| {
                        if let QueryOutcome::Rows { ref result, .. } = o {
                            highlighted.set(runner::extract_lonlat(result));
                        } else {
                            highlighted.set(Vec::new());
                        }
                        outcome.set(Some(o));
                    },
                }
                ResultsPanel { outcome: outcome.read().clone() }
            }
        }
    }
}

#[component]
fn StatusBanner(stage: LoadStage) -> Element {
    rsx! {
        section { class: "status",
            match stage {
                LoadStage::Booting => rsx! {
                    p { "Initializing in-memory SQLite plus geolite extension..." }
                },
                LoadStage::LoadingData { inserted, total } => {
                    let pct = if total == 0 { 0.0 } else { (inserted as f64 / total as f64) * 100.0 };
                    rsx! {
                        p { "Loading cities5000 dataset... {inserted} / {total} rows" }
                        div { class: "progress",
                            div { class: "progress-fill", style: "width: {pct}%" }
                        }
                    }
                },
                LoadStage::Ready { rows, elapsed_ms } => rsx! {
                    p { "DB ready | {rows} cities loaded in {elapsed_ms:.0} ms." }
                },
                LoadStage::Error(msg) => rsx! { p { class: "error", "Error: {msg}" } },
            }
        }
    }
}
