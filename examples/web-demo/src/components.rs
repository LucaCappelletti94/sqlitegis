//! UI components: schema editor, query editor with preset chips, results table.

use dioxus::prelude::*;
use dioxus_code::{CodeTheme, Theme};
use dioxus_code_editor::{CodeEditor, Language};
use dioxus_free_icons::icons::fa_solid_icons::{
    FaArrowsRotate, FaArrowsToCircle, FaBullseye, FaCircleCheck, FaCircleDot, FaCode, FaCompass,
    FaDatabase, FaDrawPolygon, FaFileCode, FaFlag, FaFont, FaLocationArrow, FaMagnifyingGlass,
    FaPlay, FaRoute, FaScaleBalanced, FaTriangleExclamation, FaVectorSquare,
};
use dioxus_free_icons::Icon;

use sqlitegis_web_demo_protocol::QueryOutcome;

use crate::state::DEFAULT_SCHEMA_SQL;
use crate::worker_handle;

fn sql_theme() -> CodeTheme {
    CodeTheme::fixed(Theme::GITHUB_LIGHT)
}

#[component]
pub fn SchemaPanel(on_reset: EventHandler<String>) -> Element {
    let mut sql = use_signal(|| DEFAULT_SCHEMA_SQL.to_string());
    let status = use_signal(String::new);

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
                        // Emit the current (possibly edited) schema upstream;
                        // main.rs will spawn the worker calls so DROP TABLE
                        // and the cities5000 reload run off the main thread.
                        on_reset.call(sql.read().clone());
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
-- Nearest cities by geodesic distance from the probe lon/lat. R-tree-prefiltered
-- with a generous 10-degree bbox, ORDER BY refines candidates by exact distance.
SELECT p.name, p.country, p.population,
  ROUND(ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) / 1000.0, 1) AS km,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY km LIMIT 100;";

const PRESET_RADIUS: &str = "\
-- The 100 cities closest to the 1000 km boundary around the probe. Same R-tree
-- bbox prefilter (dlon scales with 1/cos(lat) so it stays safe near the poles)
-- and ST_DWithinSphere circle test, but ordered by geodesic distance
-- DESCENDING and capped, so the highlighted points trace the edge of the
-- radius instead of filling its dense interior.
SELECT p.name, p.country, p.population,
  ROUND(ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) / 1000.0, 1) AS km,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 1000000.0 / (111320.0 * cos(radians(:lat)))
  AND r.xmin <= :lon + 1000000.0 / (111320.0 * cos(radians(:lat)))
  AND r.ymax >= :lat - 1000000.0 / 111320.0
  AND r.ymin <= :lat + 1000000.0 / 111320.0
  AND ST_DWithinSphere(p.geom, ST_Point(:lon, :lat, 4326), 1000000.0)
ORDER BY km DESC LIMIT 100;";

const PRESET_ENVELOPE: &str = "\
-- Cities inside a 30x30 degree box centered on the probe. For a point-only
-- dataset the R-tree bbox check IS the intersection test, so no ST_Intersects
-- refinement is needed.
SELECT p.name, p.country, p.population,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 15.0 AND r.xmin <= :lon + 15.0
  AND r.ymax >= :lat - 15.0 AND r.ymin <= :lat + 15.0
ORDER BY p.population DESC LIMIT 100;";

const PRESET_BEARING: &str = "\
-- Geodesic bearing from the probe to each of the 100 nearest cities. ST_Azimuth
-- returns radians (0 = north, clockwise). degrees() converts to a familiar
-- 0..360 compass reading.
SELECT p.name, p.country,
  ROUND(ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) / 1000.0, 1) AS km,
  ROUND(degrees(ST_Azimuth(ST_Point(:lon, :lat, 4326), p.geom)), 1) AS bearing_deg,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY km LIMIT 100;";

const PRESET_COMPASS: &str = "\
-- Project the probe 5000 km in 8 cardinal and intercardinal directions via
-- ST_Project. Bearing is in radians (0 = north, clockwise). The result is a
-- destination Point per compass arm.
WITH RECURSIVE compass(i) AS (
  SELECT 0 UNION ALL SELECT i + 1 FROM compass WHERE i < 7
)
SELECT
  CASE i
    WHEN 0 THEN 'N'  WHEN 1 THEN 'NE' WHEN 2 THEN 'E'  WHEN 3 THEN 'SE'
    WHEN 4 THEN 'S'  WHEN 5 THEN 'SW' WHEN 6 THEN 'W'  ELSE 'NW'
  END AS direction,
  ROUND(ST_X(ST_Project(ST_Point(:lon, :lat, 4326), 5000000.0,
                        radians(i * 45.0))), 4) AS lon,
  ROUND(ST_Y(ST_Project(ST_Point(:lon, :lat, 4326), 5000000.0,
                        radians(i * 45.0))), 4) AS lat
FROM compass;";

const PRESET_POLYLINE: &str = "\
-- A west-to-east tour through the 15 most populous cities within 3000 km of
-- the probe. ST_DWithinSphere selects the region on the WGS84 sphere, then the
-- cities' coordinates are stitched (ordered by longitude) into a GeoJSON
-- LineString the map draws as one connected route. The two group_concat calls
-- build the readable name list and the coordinate array in a single pass.
WITH tour AS (
  SELECT p.name, ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
  FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
  WHERE r.xmax >= :lon - 3000000.0 / (111320.0 * cos(radians(:lat)))
    AND r.xmin <= :lon + 3000000.0 / (111320.0 * cos(radians(:lat)))
    AND r.ymax >= :lat - 3000000.0 / 111320.0
    AND r.ymin <= :lat + 3000000.0 / 111320.0
    AND ST_DWithinSphere(p.geom, ST_Point(:lon, :lat, 4326), 3000000.0)
  ORDER BY p.population DESC LIMIT 15
)
SELECT
  COUNT(*) AS stops,
  group_concat(name, ' -> ' ORDER BY lon) AS route,
  '{\"type\":\"LineString\",\"coordinates\":[' ||
    group_concat('[' || ROUND(lon, 5) || ',' || ROUND(lat, 5) || ']' ORDER BY lon) ||
  ']}' AS geojson
FROM tour;";

const PRESET_SPHEROID: &str = "\
-- Compare ST_DistanceSphere (Haversine) and ST_DistanceSpheroid (Karney) for
-- the 50 nearest cities. The delta column shows the ~0.5%-scale precision gap
-- between spherical and ellipsoidal earth models. Gap grows with latitude.
SELECT p.name, p.country,
  ROUND(ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)), 0) AS m_sphere,
  ROUND(ST_DistanceSpheroid(p.geom, ST_Point(:lon, :lat, 4326)), 0) AS m_spheroid,
  ROUND(ST_DistanceSpheroid(p.geom, ST_Point(:lon, :lat, 4326))
        - ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)), 1) AS delta_m,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY m_spheroid LIMIT 50;";

const PRESET_ASTEXT: &str = "\
-- BLOB to WKT for the 100 cities nearest the probe lon/lat. R-tree-prefiltered
-- with a 10-degree bbox, ORDER BY refines.
SELECT p.name, ST_AsText(p.geom) AS wkt,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) LIMIT 100;";

const PRESET_EXPLAIN: &str = "\
-- Query plan for the indexed nearest-cities lookup. Look for
-- `VIRTUAL TABLE INDEX places_geom_rtree` in the output.
EXPLAIN QUERY PLAN
SELECT p.name, p.country, p.population,
  ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) AS d
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY d LIMIT 100;";

const PRESET_COUNTRIES: &str = "\
-- Top countries by city count within 3000 km of the probe. R-tree bbox
-- prefilter (dlon scales with 1/cos(lat)) then exact ST_DWithinSphere for
-- the 3000 km circle.
SELECT p.country, COUNT(*) AS cities, SUM(p.population) AS total_pop
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 3000000.0 / (111320.0 * cos(radians(:lat)))
  AND r.xmin <= :lon + 3000000.0 / (111320.0 * cos(radians(:lat)))
  AND r.ymax >= :lat - 3000000.0 / 111320.0
  AND r.ymin <= :lat + 3000000.0 / 111320.0
  AND ST_DWithinSphere(p.geom, ST_Point(:lon, :lat, 4326), 3000000.0)
GROUP BY p.country
ORDER BY cities DESC LIMIT 100;";

const PRESET_RINGS: &str = "\
-- Population in concentric rings around the probe (up to 3000 km). R-tree
-- prefilter inside the CTE. The CASE then bins the exact geodesic distance
-- into bands.
SELECT
  CASE
    WHEN d <=  500000 THEN '0-500 km'
    WHEN d <= 1000000 THEN '500-1000 km'
    WHEN d <= 2000000 THEN '1000-2000 km'
    ELSE                   '2000-3000 km'
  END AS ring,
  COUNT(*) AS cities, SUM(population) AS total_pop
FROM (
  SELECT p.population,
    ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) AS d
  FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
  WHERE r.xmax >= :lon - 3000000.0 / (111320.0 * cos(radians(:lat)))
    AND r.xmin <= :lon + 3000000.0 / (111320.0 * cos(radians(:lat)))
    AND r.ymax >= :lat - 3000000.0 / 111320.0
    AND r.ymin <= :lat + 3000000.0 / 111320.0
    AND ST_DWithinSphere(p.geom, ST_Point(:lon, :lat, 4326), 3000000.0)
)
GROUP BY ring ORDER BY MIN(d);";

const PRESET_BUFFER: &str = "\
-- Draw a 5-degree planar buffer polygon around the probe point. ST_Buffer
-- expands the point into a ring (a circle in lon/lat space, which the
-- equirectangular map renders as an ellipse). The result is one row whose
-- geojson column the map fills in as a polygon overlay.
SELECT ST_AsGeoJSON(ST_Buffer(ST_Point(:lon, :lat, 4326), 5.0)) AS geojson;";

const PRESET_GEOJSON: &str = "\
-- Serialize the 100 cities nearest the probe to GeoJSON. R-tree-prefiltered
-- with a 10-degree bbox, ORDER BY refines.
SELECT p.name, p.country, ST_AsGeoJSON(p.geom) AS geojson,
  ST_X(p.geom) AS lon, ST_Y(p.geom) AS lat
FROM places p JOIN places_geom_rtree r ON p.rowid = r.id
WHERE r.xmax >= :lon - 10 AND r.xmin <= :lon + 10
  AND r.ymax >= :lat - 10 AND r.ymin <= :lat + 10
ORDER BY ST_DistanceSphere(p.geom, ST_Point(:lon, :lat, 4326)) LIMIT 100;";

#[component]
pub fn QueryPanel(
    user_lon: ReadSignal<f64>,
    user_lat: ReadSignal<f64>,
    on_outcome: EventHandler<QueryOutcome>,
) -> Element {
    let mut sql = use_signal(|| PRESET_KNN.to_string());
    let mut active_preset = use_signal(|| "KNN".to_string());

    // Auto-re-run the current query whenever the user's position changes
    // (map click, random initial placement). The SQL itself is read via
    // `peek` so manual edits to the textarea don't trigger a re-run by
    // themselves. Only position changes do. NaN means the loader hasn't
    // picked a starting city yet, so skip until we have a real position.
    use_effect(move || {
        let lon = *user_lon.read();
        let lat = *user_lat.read();
        if !lon.is_finite() || !lat.is_finite() {
            return;
        }
        let current = sql.peek().clone();
        let outcome_handler = on_outcome;
        spawn(async move {
            let outcome = worker_handle::run_query(&current, lon, lat).await;
            outcome_handler.call(outcome);
        });
    });

    rsx! {
        section { class: "query-panel",
            div { class: "panel-header",
                h2 {
                    Icon { width: 16, height: 16, icon: FaCode, class: "section-icon".to_string() }
                    "Query"
                }
                div { class: "presets",
                    PresetChip {
                        label: "KNN",
                        description: "Find the 100 cities closest to the probe point, R-tree prefiltered and ranked by geodesic distance on the WGS84 ellipsoid",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaBullseye, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "KNN",
                        on_pick: move |_| {
                            let new_sql = PRESET_KNN.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("KNN".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Radius",
                        description: "Highlight the 100 cities closest to the 1000 km boundary around the probe, ordered by geodesic distance descending so the points trace the edge of the radius",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaCompass, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Radius",
                        on_pick: move |_| {
                            let new_sql = PRESET_RADIUS.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Radius".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "BBOX",
                        description: "List cities inside a 30 by 30 degree box centered on the probe point, top 100 by population",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaVectorSquare, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "BBOX",
                        on_pick: move |_| {
                            let new_sql = PRESET_ENVELOPE.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("BBOX".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Bearing",
                        description: "Geodesic compass bearing from the probe to each of the 100 nearest cities, R-tree prefiltered. ST_Azimuth converts radians to a 0..360 reading",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaLocationArrow, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Bearing",
                        on_pick: move |_| {
                            let new_sql = PRESET_BEARING.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Bearing".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Compass",
                        description: "Project the probe 5000 km in 8 cardinal and intercardinal directions via ST_Project. Returns the destination Point per compass arm",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaArrowsToCircle, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Compass",
                        on_pick: move |_| {
                            let new_sql = PRESET_COMPASS.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Compass".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Polyline",
                        description: "Stitch the 15 most populous cities within 3000 km of the probe into a west-to-east GeoJSON LineString the map draws as one connected route",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaRoute, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Polyline",
                        on_pick: move |_| {
                            let new_sql = PRESET_POLYLINE.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Polyline".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Buffer",
                        description: "Expand the probe point into a 5-degree buffer polygon with ST_Buffer and draw the ring on the map as a filled polygon overlay",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaDrawPolygon, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Buffer",
                        on_pick: move |_| {
                            let new_sql = PRESET_BUFFER.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Buffer".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Spheroid",
                        description: "Compare ST_DistanceSphere (Haversine) and ST_DistanceSpheroid (Karney) for the 50 nearest cities. The delta column surfaces the precision gap between spherical and ellipsoidal earth models",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaScaleBalanced, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Spheroid",
                        on_pick: move |_| {
                            let new_sql = PRESET_SPHEROID.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Spheroid".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "AsText",
                        description: "Convert the EWKB BLOB geometry column to readable WKT for the 100 cities nearest the probe",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaFont, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "AsText",
                        on_pick: move |_| {
                            let new_sql = PRESET_ASTEXT.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("AsText".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Explain",
                        description: "Show the query plan SQLite picks for an R-tree-backed lookup, useful to confirm the virtual table is in use",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaMagnifyingGlass, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Explain",
                        on_pick: move |_| {
                            let new_sql = PRESET_EXPLAIN.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Explain".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Countries",
                        description: "Group cities within 3000 km of the probe by country and rank by city count and total population",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaFlag, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Countries",
                        on_pick: move |_| {
                            let new_sql = PRESET_COUNTRIES.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Countries".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "Rings",
                        description: "Aggregate city counts and total population into concentric distance bands around the probe",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaCircleDot, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "Rings",
                        on_pick: move |_| {
                            let new_sql = PRESET_RINGS.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("Rings".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                    PresetChip {
                        label: "GeoJSON",
                        description: "Serialize the 100 cities nearest the probe as GeoJSON Point features",
                        icon: rsx! { Icon { width: 11, height: 11, icon: FaFileCode, class: "btn-icon".to_string() } },
                        active: active_preset.read().as_str() == "GeoJSON",
                        on_pick: move |_| {
                            let new_sql = PRESET_GEOJSON.to_string();
                            sql.set(new_sql.clone());
                            active_preset.set("GeoJSON".to_string());
                            let lon = *user_lon.read(); let lat = *user_lat.read(); let new_sql = new_sql.clone(); let outcome_handler = on_outcome; spawn(async move { let outcome = worker_handle::run_query(&new_sql, lon, lat).await; outcome_handler.call(outcome); });
                        },
                    }
                }
                button {
                    aria_label: "Run the query against the loaded database",
                    title: "Run the query against the loaded database",
                    onclick: move |_| {
                        let current = sql.read().clone();
                        let lon = *user_lon.read();
                        let lat = *user_lat.read();
                        let outcome_handler = on_outcome;
                        spawn(async move {
                            let outcome = worker_handle::run_query(&current, lon, lat).await;
                            outcome_handler.call(outcome);
                        });
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
    active: bool,
    on_pick: EventHandler<()>,
) -> Element {
    let class = if active {
        "preset-chip active"
    } else {
        "preset-chip"
    };
    rsx! {
        button {
            class: "{class}",
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
