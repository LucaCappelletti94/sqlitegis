//! UI components: schema editor, query editor with preset chips, results table.

use dioxus::prelude::*;
use dioxus_code::{CodeTheme, Theme};
use dioxus_code_editor::{CodeEditor, Language};

use crate::db;
use crate::runner::{self, QueryOutcome};
use crate::state::DEFAULT_SCHEMA_SQL;

fn sql_theme() -> CodeTheme {
    CodeTheme::fixed(Theme::TOKYO_NIGHT)
}

#[component]
pub fn SchemaPanel(on_reset: EventHandler<Result<(), String>>) -> Element {
    let mut sql = use_signal(|| DEFAULT_SCHEMA_SQL.to_string());
    let mut status = use_signal(|| String::new());

    rsx! {
        section {
            h2 { "Schema" }
            CodeEditor {
                value: sql(),
                language: Language::Sql,
                theme: sql_theme(),
                line_numbers: true,
                spellcheck: false,
                aria_label: "Schema SQL".to_string(),
                class: "editor".to_string(),
                oninput: move |v| sql.set(v),
            }
            div { class: "controls",
                button {
                    onclick: move |_| {
                        // Re-open the connection so DROP TABLE etc. start from
                        // a clean slate, then apply the (possibly edited) schema.
                        let s = sql.read().clone();
                        let result = (|| -> Result<(), String> {
                            db::reopen()?;
                            db::run_script(&s)?;
                            Ok(())
                        })();
                        match &result {
                            Ok(_) => status.set("Schema applied. Table recreated.".to_string()),
                            Err(e) => status.set(format!("Error: {e}")),
                        }
                        on_reset.call(result);
                    },
                    "Reset DB"
                }
                span { class: "meta", "{status}" }
            }
        }
    }
}

const PRESET_KNN: &str = "\
-- 10 nearest cities to your location by geodesic distance.
-- `lon` and `lat` columns are picked up by the map to highlight result rows.
SELECT name, country, population,
       ROUND(ST_DistanceSphere(geom, ST_Point(:lon, :lat, 4326)) / 1000.0, 1)
         AS km,
       ST_X(geom) AS lon,
       ST_Y(geom) AS lat
FROM places
ORDER BY km
LIMIT 10;
";

const PRESET_RADIUS: &str = "\
-- Cities within 200 km of your location.
SELECT name, country, population,
       ST_X(geom) AS lon,
       ST_Y(geom) AS lat
FROM places
WHERE ST_DWithinSphere(geom, ST_Point(:lon, :lat, 4326), 200000.0)
ORDER BY population DESC;
";

const PRESET_ENVELOPE: &str = "\
-- Cities inside a bounding box (Western Europe).
SELECT name, country, population,
       ST_X(geom) AS lon,
       ST_Y(geom) AS lat
FROM places
WHERE ST_Intersects(geom, ST_MakeEnvelope(-10.0, 35.0, 15.0, 60.0, 4326))
ORDER BY population DESC
LIMIT 200;
";

const PRESET_ASTEXT: &str = "\
-- Round-trip: BLOB geometry to human-readable WKT.
SELECT name, ST_AsText(geom) AS wkt
FROM places
ORDER BY name
LIMIT 8;
";

#[component]
pub fn QueryPanel(
    user_lon: ReadSignal<f64>,
    user_lat: ReadSignal<f64>,
    on_outcome: EventHandler<QueryOutcome>,
) -> Element {
    let mut sql = use_signal(|| PRESET_KNN.to_string());

    // Auto-re-run the current query whenever the user's position changes
    // (map click, random initial placement). The SQL itself is read via
    // `peek` so manual edits to the textarea don't trigger a re-run by
    // themselves -- only position changes do. NaN means the loader hasn't
    // picked a starting city yet, so skip until we have a real position.
    use_effect(move || {
        let lon = *user_lon.read();
        let lat = *user_lat.read();
        if !lon.is_finite() || !lat.is_finite() {
            return;
        }
        let current = sql.peek().clone();
        on_outcome.call(runner::run(&current, lon, lat));
    });

    rsx! {
        section {
            h2 { "Query" }
            CodeEditor {
                value: sql(),
                language: Language::Sql,
                theme: sql_theme(),
                line_numbers: true,
                spellcheck: false,
                aria_label: "Query SQL".to_string(),
                class: "editor".to_string(),
                oninput: move |v| sql.set(v),
            }
            div { class: "controls",
                button {
                    onclick: move |_| {
                        let outcome = runner::run(&sql.read(), *user_lon.read(), *user_lat.read());
                        on_outcome.call(outcome);
                    },
                    "Run"
                }
                PresetChip {
                    label: "KNN distance",
                    on_pick: move |_| {
                        let new_sql = PRESET_KNN.to_string();
                        sql.set(new_sql.clone());
                        on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                    },
                }
                PresetChip {
                    label: "Radius 200 km",
                    on_pick: move |_| {
                        let new_sql = PRESET_RADIUS.to_string();
                        sql.set(new_sql.clone());
                        on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                    },
                }
                PresetChip {
                    label: "Bounding box",
                    on_pick: move |_| {
                        let new_sql = PRESET_ENVELOPE.to_string();
                        sql.set(new_sql.clone());
                        on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                    },
                }
                PresetChip {
                    label: "ST_AsText",
                    on_pick: move |_| {
                        let new_sql = PRESET_ASTEXT.to_string();
                        sql.set(new_sql.clone());
                        on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                    },
                }
            }
            p { class: "meta",
                "`:lon` and `:lat` in the SQL bind to your current position "
                "(lon={user_lon}, lat={user_lat}). Click anywhere on the map "
                "to move the probe point and re-run the current query."
            }
        }
    }
}

#[component]
fn PresetChip(label: String, on_pick: EventHandler<()>) -> Element {
    rsx! {
        button {
            onclick: move |_| on_pick.call(()),
            "{label}"
        }
    }
}

#[component]
pub fn ResultsPanel(outcome: Option<QueryOutcome>) -> Element {
    let body = match outcome {
        None => rsx! { p { class: "meta", "Run a query to see results here." } },
        Some(QueryOutcome::Error(msg)) => rsx! { pre { class: "error", "{msg}" } },
        Some(QueryOutcome::Affected { rows, elapsed_ms }) => rsx! {
            p { class: "meta", "OK | {rows} row(s) affected | {elapsed_ms:.1} ms" }
        },
        Some(QueryOutcome::Rows { result, elapsed_ms }) => rsx! {
            p { class: "meta", "{result.rows.len()} row(s) | {elapsed_ms:.1} ms" }
            div { style: "max-height: 360px; overflow: auto;",
                table { class: "results",
                    thead {
                        tr {
                            for col in result.columns.iter() {
                                th { "{col}" }
                            }
                        }
                    }
                    tbody {
                        for row in result.rows.iter() {
                            tr {
                                for cell in row.iter() {
                                    td { "{cell}" }
                                }
                            }
                        }
                    }
                }
            }
        },
    };

    rsx! {
        section {
            h2 { "Results" }
            {body}
        }
    }
}

