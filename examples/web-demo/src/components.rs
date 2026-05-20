//! UI components: schema editor, query editor with preset chips, results table.

use dioxus::prelude::*;
use dioxus_code::{CodeTheme, Theme};
use dioxus_code_editor::{CodeEditor, Language};
use dioxus_free_icons::icons::fa_solid_icons::{
    FaArrowsRotate, FaBolt, FaBullseye, FaCircleCheck, FaCircleDot, FaCode, FaCompass, FaDatabase,
    FaFileCode, FaFlag, FaFont, FaMagnifyingGlass, FaPlay, FaSitemap, FaTriangleExclamation,
    FaVectorSquare,
};
use dioxus_free_icons::Icon;

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
        section { class: "schema-panel",
            div { class: "panel-header",
                h2 {
                    Icon { width: 16, height: 16, icon: FaDatabase, class: "section-icon".to_string() }
                    "Schema"
                }
                button {
                    aria_label: "Reset database and reapply the schema",
                    title: "Reset database and reapply the schema",
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
                    Icon { width: 13, height: 13, icon: FaArrowsRotate, class: "btn-icon".to_string() }
                    "Reset DB"
                }
            }
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
            // Only render the status row when there's something to say --
            // an empty <p> would still reserve line-height + padding at the
            // bottom of the panel and reintroduce the gap we just removed.
            if !status.read().is_empty() {
                p { class: "meta", "{status}" }
            }
        }
    }
}

const PRESET_KNN: &str = "\
-- Nearest cities by geodesic distance from the probe lon/lat.
SELECT name, country, population,
  ROUND(ST_DistanceSphere(geom, ST_Point(:lon, :lat, 4326))/1000.0,
        1) AS km,
  ST_X(geom) AS lon, ST_Y(geom) AS lat
FROM places
ORDER BY km LIMIT 100;";

const PRESET_RADIUS: &str = "\
-- Cities within 1000 km of your location.
SELECT name, country, population,
  ST_X(geom) AS lon, ST_Y(geom) AS lat
FROM places
WHERE ST_DWithinSphere(geom, ST_Point(:lon, :lat, 4326), 1000000.0)
ORDER BY population DESC;";

const PRESET_RADIUS_IDX: &str = "\
-- Indexed radius: R-tree bbox prefilter then geodesic refinement.
-- dlon scales with 1/cos(lat) so the bound stays safe at the poles
-- where one degree of longitude shrinks.
SELECT p.name, p.country, p.population,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p
JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 1000000.0/(111320.0*cos(radians(:lat)))
  AND r.xmin <= :lon + 1000000.0/(111320.0*cos(radians(:lat)))
  AND r.ymax >= :lat - 1000000.0/111320.0
  AND r.ymin <= :lat + 1000000.0/111320.0
  AND ST_DWithinSphere(p.geom,
                       ST_Point(:lon, :lat, 4326), 1000000.0)
ORDER BY p.population DESC;";

const PRESET_ENVELOPE: &str = "\
-- Cities inside a 30x30 degree box centered on the probe point.
SELECT name, country, population,
  ST_X(geom) AS lon, ST_Y(geom) AS lat
FROM places
WHERE ST_Intersects(geom, ST_MakeEnvelope(
  :lon - 15.0, :lat - 15.0, :lon + 15.0, :lat + 15.0, 4326
))
ORDER BY population DESC LIMIT 100;";

const PRESET_ASTEXT: &str = "\
-- Round-trip: BLOB geometry to human-readable WKT.
SELECT name, ST_AsText(geom) AS wkt
FROM places
ORDER BY name LIMIT 100;";

const PRESET_RTREE: &str = "\
-- Nearest cities via R-tree prefilter then geodesic refinement.
-- The JOIN against places_geom_rtree narrows by bounding box in
-- O(log N). ST_DistanceSphere then refines to true geodesic order.
SELECT p.name, p.country, p.population,
  ROUND(ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326))/1000.0,
        1) AS km,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY km LIMIT 100;";

const PRESET_EXPLAIN: &str = "\
-- See how SQLite plans the index-aware lookup. Look for
-- `VIRTUAL TABLE INDEX places_geom_rtree` in the output.
EXPLAIN QUERY PLAN
SELECT p.name FROM places p
JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 5 AND r.xmin <= :lon + 5
  AND r.ymax >= :lat - 5 AND r.ymin <= :lat + 5;";

const PRESET_COUNTRIES: &str = "\
-- Top countries by city count within 3000 km of the probe.
SELECT country, COUNT(*) AS cities, SUM(population) AS total_pop
FROM places
WHERE ST_DWithinSphere(geom, ST_Point(:lon, :lat, 4326), 3000000.0)
GROUP BY country
ORDER BY cities DESC LIMIT 100;";

const PRESET_RINGS: &str = "\
-- Population in concentric rings around the probe (up to 3000 km).
SELECT
  CASE
    WHEN d <=  500000 THEN '0-500 km'
    WHEN d <= 1000000 THEN '500-1000 km'
    WHEN d <= 2000000 THEN '1000-2000 km'
    ELSE                   '2000-3000 km'
  END AS ring,
  COUNT(*) AS cities, SUM(population) AS total_pop
FROM (
  SELECT population,
    ST_DistanceSphere(geom, ST_Point(:lon, :lat, 4326)) AS d
  FROM places
  WHERE ST_DWithinSphere(geom, ST_Point(:lon, :lat, 4326), 3000000.0)
)
GROUP BY ring ORDER BY MIN(d);";

const PRESET_GEOJSON: &str = "\
-- Serialize geometries to GeoJSON, ready for Leaflet or Mapbox.
SELECT name, country, ST_AsGeoJSON(geom) AS geojson
FROM places
ORDER BY population DESC LIMIT 100;";

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
        section { class: "query-panel",
            div { class: "panel-header",
                div { class: "header-left",
                    h2 {
                        Icon { width: 16, height: 16, icon: FaCode, class: "section-icon".to_string() }
                        "Query"
                    }
                    div { class: "presets",
                        PresetChip {
                            label: "KNN",
                            description: "Find the 10 cities closest to the probe point, ranked by geodesic distance on the WGS84 ellipsoid",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaBullseye, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_KNN.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "Radius",
                            description: "List every city within 1000 km of the probe point, sorted by population",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaCompass, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_RADIUS.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "Radius+",
                            description: "Same radius search, but R-tree prefiltered so the engine touches O(log N) candidates instead of every row",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaBolt, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_RADIUS_IDX.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "BBOX",
                            description: "List cities inside a 30 by 30 degree box centered on the probe point, top 100 by population",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaVectorSquare, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_ENVELOPE.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "AsText",
                            description: "Convert the EWKB BLOB geometry column to readable WKT for the first 8 cities",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaFont, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_ASTEXT.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "RTree",
                            description: "Same KNN result, but with an explicit JOIN against the R-tree shadow table so the SQLite virtual-table index actually drives the prefilter",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaSitemap, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_RTREE.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "Explain",
                            description: "Show the query plan SQLite picks for an R-tree-backed lookup, useful to confirm the virtual table is in use",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaMagnifyingGlass, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_EXPLAIN.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "Countries",
                            description: "Group cities within 3000 km of the probe by country and rank by city count and total population",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaFlag, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_COUNTRIES.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "Rings",
                            description: "Aggregate city counts and total population into concentric distance bands around the probe",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaCircleDot, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_RINGS.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                        PresetChip {
                            label: "GeoJSON",
                            description: "Serialize the top 5 most populous cities as GeoJSON Point features",
                            icon: rsx! { Icon { width: 12, height: 12, icon: FaFileCode, class: "btn-icon".to_string() } },
                            on_pick: move |_| {
                                let new_sql = PRESET_GEOJSON.to_string();
                                sql.set(new_sql.clone());
                                on_outcome.call(runner::run(&new_sql, *user_lon.read(), *user_lat.read()));
                            },
                        }
                    }
                }
                button {
                    aria_label: "Run the query against the loaded database",
                    title: "Run the query against the loaded database",
                    onclick: move |_| {
                        let outcome = runner::run(&sql.read(), *user_lon.read(), *user_lat.read());
                        on_outcome.call(outcome);
                    },
                    Icon { width: 13, height: 13, icon: FaPlay, class: "btn-icon".to_string() }
                    "Run"
                }
            }
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
        }
    }
}

#[component]
fn PresetChip(
    label: String,
    description: String,
    icon: Element,
    on_pick: EventHandler<()>,
) -> Element {
    rsx! {
        button {
            aria_label: "{description}",
            title: "{description}",
            onclick: move |_| on_pick.call(()),
            {icon}
            "{label}"
        }
    }
}

#[component]
pub fn ResultsPanel(outcome: Option<QueryOutcome>) -> Element {
    let body = match outcome {
        None => rsx! { p { class: "meta", "Run a query to see results here." } },
        Some(QueryOutcome::Error(msg)) => rsx! {
            pre { class: "error",
                Icon { width: 13, height: 13, icon: FaTriangleExclamation, class: "status-icon err".to_string() }
                "{msg}"
            }
        },
        Some(QueryOutcome::Affected { rows, .. }) => rsx! {
            p { class: "meta",
                Icon { width: 13, height: 13, icon: FaCircleCheck, class: "status-icon ok".to_string() }
                "OK | {rows} row(s) affected"
            }
        },
        Some(QueryOutcome::Rows { result, .. }) => {
            let columns = result.columns.clone();
            let rows = result.rows.clone();
            rsx! {
                div { class: "results-scroll",
                    table { class: "results",
                        thead {
                            tr {
                                for col in columns.iter() {
                                    th { "{col}" }
                                }
                            }
                        }
                        tbody {
                            for row in rows.iter() {
                                tr {
                                    for cell in row.iter() {
                                        td { "{cell}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! { section { class: "results-panel", {body} } }
}
