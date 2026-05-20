//! SQLiteGIS web demo. Runs Diesel queries through SQLiteGIS's SQLite extension
//! entirely in the browser via sqlite-wasm-rs.

mod components;
mod db;
mod loader;
mod runner;
mod state;
mod viz;

use dioxus::prelude::*;
use dioxus_free_icons::icons::fa_solid_icons::{
    FaCircleNotch, FaHourglass, FaTriangleExclamation,
};
use dioxus_free_icons::Icon;

use crate::components::{QueryPanel, ResultsPanel, SchemaPanel};
use crate::runner::QueryOutcome;
use crate::viz::WorldMap;

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
            stage.set(LoadStage::LoadingData {
                inserted: 0,
                total: 0,
            });
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
        main {
            header {
                h1 {
                    img {
                        src: "/logo.svg",
                        alt: "SQLiteGIS logo",
                        width: 36,
                        height: 36,
                        class: "title-icon",
                    }
                    "SQLiteGIS"
                }
                p { class: "tagline",
                    "Spatial SQL for SQLite, the PostGIS way."
                }
                p { class: "intro",
                    "SQLiteGIS is a pure-Rust SQLite extension that ships over 100 "
                    "PostGIS-compatible spatial functions plus first-class "
                    "Diesel ORM bindings. Geometries are stored as EWKB BLOBs "
                    "(the PostGIS wire format), and queries support planar and "
                    "geodesic distance, DE-9IM predicates, and R-tree spatial "
                    "indexes. The full stack runs in this page through "
                    "WebAssembly, so you can experiment with real spatial SQL "
                    "without setting up Postgres."
                }
                div { class: "feature-pills",
                    span { class: "pill", "100+ ST_* functions" }
                    span { class: "pill", "Geodesic + planar distance" }
                    span { class: "pill", "R-tree spatial indexes" }
                    span { class: "pill", "DE-9IM predicates" }
                    span { class: "pill", "Diesel ORM" }
                    span { class: "pill", "Pure Rust" }
                    span { class: "pill", "Native + WASM" }
                }
                nav { class: "quick-links", aria_label: "Project resources",
                    a {
                        href: "https://github.com/LucaCappelletti94/sqlitegis",
                        rel: "noopener",
                        target: "_blank",
                        "GitHub"
                    }
                    a {
                        href: "https://docs.rs/sqlitegis",
                        rel: "noopener",
                        target: "_blank",
                        "docs.rs"
                    }
                    a {
                        href: "https://crates.io/crates/sqlitegis",
                        rel: "noopener",
                        target: "_blank",
                        "crates.io"
                    }
                }
            }
            StatusBanner { stage: stage.read().clone() }

            // Loading view: centered stack with the map on top and the
            // progress bar directly below it, both pinned to the same width.
            if matches!(*stage.read(), LoadStage::LoadingData { .. }) {
                div { class: "loading-stack",
                    WorldMap {
                        coords: all_coords,
                        highlighted,
                        user_lon,
                        user_lat,
                    }
                    if let LoadStage::LoadingData { inserted, total } = *stage.read() {
                        LoadingProgress { inserted, total }
                    }
                }
            }

            // Ready view: 2x2 grid. Top row: Query | Schema. Bottom row:
            // Results | Map. Two equally-sized columns so the map and
            // results sit side by side instead of the map swallowing the
            // viewport.
            if matches!(*stage.read(), LoadStage::Ready { .. }) {
                div { class: "panel-grid",
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
                    ResultsPanel { outcome: outcome.read().clone() }
                    WorldMap {
                        coords: all_coords,
                        highlighted,
                        user_lon,
                        user_lat,
                    }
                }
            }
        }
    }
}

#[component]
fn StatusBanner(stage: LoadStage) -> Element {
    let body = match stage {
        LoadStage::Booting => rsx! {
            p {
                Icon { width: 14, height: 14, icon: FaHourglass, class: "status-icon".to_string() }
                "Initializing in-memory SQLite plus SQLiteGIS extension..."
            }
        },
        LoadStage::Error(msg) => rsx! {
            p { class: "error",
                Icon { width: 14, height: 14, icon: FaTriangleExclamation, class: "status-icon err".to_string() }
                "Error: {msg}"
            }
        },
        // LoadingData renders below the map via LoadingProgress; Ready is
        // silent so the chrome gets out of the user's way once loading is done.
        LoadStage::LoadingData { .. } | LoadStage::Ready { .. } => return rsx! {},
    };
    rsx! {
        section { class: "status", {body} }
    }
}

#[component]
fn LoadingProgress(inserted: usize, total: usize) -> Element {
    let pct = if total == 0 {
        0.0
    } else {
        (inserted as f64 / total as f64) * 100.0
    };
    rsx! {
        section { class: "status",
            p {
                Icon { width: 14, height: 14, icon: FaCircleNotch, class: "status-icon spin".to_string() }
                "Loading cities5000 dataset... {inserted} / {total} rows"
            }
            div { class: "progress",
                div { class: "progress-fill", style: "width: {pct}%" }
            }
        }
    }
}
