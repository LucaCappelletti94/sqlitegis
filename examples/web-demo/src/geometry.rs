//! Parse GeoJSON strings from a query result into map-drawable geometries.
//!
//! Queries that want to draw a line or polygon on the map project the
//! geometry through `ST_AsGeoJSON(...) AS geojson`. The worker renders that
//! column as a plain JSON string (raw geometry BLOBs come back as
//! `"BLOB(N bytes)"` and are not parseable). [`extract_geometries`] walks the
//! `geojson` column and turns each cell into one or more [`MapGeometry`]
//! values for `viz::WorldMap` to overlay.
//!
//! Coordinates in GeoJSON are `[lon, lat]`, which matches the canvas
//! convention used everywhere else in the demo.

use serde_json::Value;
use sqlitegis_web_demo_protocol::QueryRows;

/// A geometry reduced to the primitives the canvas can draw. Multi-part and
/// collection inputs are flattened into several entries by the parser.
#[derive(Debug, Clone, PartialEq)]
pub enum MapGeometry {
    /// A single coordinate.
    Point((f64, f64)),
    /// An open polyline through the given vertices.
    Line(Vec<(f64, f64)>),
    /// A polygon. `rings[0]` is the exterior, any remaining rings are holes.
    Polygon { rings: Vec<Vec<(f64, f64)>> },
}

/// Pull every drawable geometry out of a result's `geojson` column.
///
/// Returns an empty vector when the result has no column named `geojson`
/// (case-insensitive). Rows whose cell fails to parse as GeoJSON, or whose
/// `type` is unrecognised, are skipped rather than erroring, so a partially
/// malformed result still renders whatever it can.
pub fn extract_geometries(result: &QueryRows) -> Vec<MapGeometry> {
    let Some(idx) = result
        .columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case("geojson"))
    else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for row in &result.rows {
        let Some(cell) = row.get(idx) else { continue };
        let Ok(value) = serde_json::from_str::<Value>(cell) else {
            continue;
        };
        push_geometry(&value, &mut out);
    }
    out
}

/// Append the [`MapGeometry`] values encoded by one GeoJSON geometry object,
/// recursing through `GeometryCollection`.
fn push_geometry(value: &Value, out: &mut Vec<MapGeometry>) {
    let Some(kind) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    match kind {
        "Point" => {
            if let Some(p) = value.get("coordinates").and_then(coord) {
                out.push(MapGeometry::Point(p));
            }
        }
        "MultiPoint" => {
            for p in coord_list(value.get("coordinates")) {
                out.push(MapGeometry::Point(p));
            }
        }
        "LineString" => {
            let line = coord_list(value.get("coordinates"));
            if !line.is_empty() {
                out.push(MapGeometry::Line(line));
            }
        }
        "MultiLineString" => {
            for line in coord_rings(value.get("coordinates")) {
                if !line.is_empty() {
                    out.push(MapGeometry::Line(line));
                }
            }
        }
        "Polygon" => {
            let rings = coord_rings(value.get("coordinates"));
            if !rings.is_empty() {
                out.push(MapGeometry::Polygon { rings });
            }
        }
        "MultiPolygon" => {
            // coordinates: [ [ring, ring, ...], [ring, ...], ... ]
            if let Some(polys) = value.get("coordinates").and_then(Value::as_array) {
                for poly in polys {
                    let rings = coord_rings(Some(poly));
                    if !rings.is_empty() {
                        out.push(MapGeometry::Polygon { rings });
                    }
                }
            }
        }
        "GeometryCollection" => {
            if let Some(geoms) = value.get("geometries").and_then(Value::as_array) {
                for g in geoms {
                    push_geometry(g, out);
                }
            }
        }
        _ => {}
    }
}

/// Parse a single `[lon, lat]` array into a coordinate pair.
fn coord(value: &Value) -> Option<(f64, f64)> {
    let arr = value.as_array()?;
    let lon = arr.first()?.as_f64()?;
    let lat = arr.get(1)?.as_f64()?;
    Some((lon, lat))
}

/// Parse `[[lon, lat], ...]` into a vector of coordinate pairs.
fn coord_list(value: Option<&Value>) -> Vec<(f64, f64)> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(coord).collect())
        .unwrap_or_default()
}

/// Parse `[[[lon, lat], ...], ...]` (a list of rings or lines) into vectors.
fn coord_rings(value: Option<&Value>) -> Vec<Vec<(f64, f64)>> {
    value
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(|ring| coord_list(Some(ring))).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(geojson: &str) -> QueryRows {
        QueryRows {
            columns: vec!["geojson".to_string()],
            rows: vec![vec![geojson.to_string()]],
        }
    }

    #[test]
    fn no_geojson_column_yields_empty() {
        let r = QueryRows {
            columns: vec!["lon".into(), "lat".into()],
            rows: vec![vec!["1".into(), "2".into()]],
        };
        assert!(extract_geometries(&r).is_empty());
    }

    #[test]
    fn parses_point() {
        let g = extract_geometries(&rows(r#"{"type":"Point","coordinates":[1.0,2.0]}"#));
        assert_eq!(g, vec![MapGeometry::Point((1.0, 2.0))]);
    }

    #[test]
    fn parses_linestring() {
        let g = extract_geometries(&rows(
            r#"{"type":"LineString","coordinates":[[0,0],[1,1],[2,0]]}"#,
        ));
        assert_eq!(
            g,
            vec![MapGeometry::Line(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)])]
        );
    }

    #[test]
    fn parses_polygon_with_hole() {
        let g = extract_geometries(&rows(
            r#"{"type":"Polygon","coordinates":[[[0,0],[4,0],[4,4],[0,4],[0,0]],[[1,1],[2,1],[2,2],[1,2],[1,1]]]}"#,
        ));
        match &g[..] {
            [MapGeometry::Polygon { rings }] => {
                assert_eq!(rings.len(), 2);
                assert_eq!(rings[0].len(), 5);
                assert_eq!(rings[1].len(), 5);
            }
            other => panic!("expected one polygon with a hole, got {other:?}"),
        }
    }

    #[test]
    fn flattens_multipolygon() {
        let g = extract_geometries(&rows(
            r#"{"type":"MultiPolygon","coordinates":[[[[0,0],[1,0],[1,1],[0,1],[0,0]]],[[[5,5],[6,5],[6,6],[5,6],[5,5]]]]}"#,
        ));
        assert_eq!(g.len(), 2);
        assert!(g.iter().all(|m| matches!(m, MapGeometry::Polygon { .. })));
    }

    #[test]
    fn recurses_geometry_collection() {
        let g = extract_geometries(&rows(
            r#"{"type":"GeometryCollection","geometries":[{"type":"Point","coordinates":[1,2]},{"type":"LineString","coordinates":[[0,0],[1,1]]}]}"#,
        ));
        assert_eq!(g.len(), 2);
        assert!(matches!(g[0], MapGeometry::Point(_)));
        assert!(matches!(g[1], MapGeometry::Line(_)));
    }

    #[test]
    fn skips_malformed_rows() {
        let r = QueryRows {
            columns: vec!["geojson".into()],
            rows: vec![
                vec!["not json".into()],
                vec![r#"{"type":"Point","coordinates":[3,4]}"#.into()],
            ],
        };
        assert_eq!(extract_geometries(&r), vec![MapGeometry::Point((3.0, 4.0))]);
    }
}
