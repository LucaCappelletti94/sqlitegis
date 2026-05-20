//! Geometry constructor functions.
//!
//! ST_Point, ST_MakePoint, ST_MakeLine, ST_MakePolygon,
//! ST_MakeEnvelope, ST_Collect

use geo::{Coord, Geometry, LineString, Point, Polygon, Rect};

use crate::core::error::{Result, SqliteGisError};
use crate::core::ewkb::{parse_ewkb, parse_ewkb_pair, write_ewkb};

/// ST_Point / ST_MakePoint (2D): construct a Point geometry.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::st_point;
/// use sqlitegis::core::functions::accessors::{st_x, st_y, st_srid};
///
/// let blob = st_point(1.5, 2.5, Some(4326)).unwrap();
/// assert!((st_x(&blob).unwrap().unwrap() - 1.5).abs() < 1e-10);
/// assert!((st_y(&blob).unwrap().unwrap() - 2.5).abs() < 1e-10);
/// assert_eq!(st_srid(&blob).unwrap(), 4326);
/// ```
pub fn st_point(x: f64, y: f64, srid: Option<i32>) -> Result<Vec<u8>> {
    if !x.is_finite() || !y.is_finite() {
        return Err(SqliteGisError::InvalidInput(
            "coordinates must be finite".to_string(),
        ));
    }
    write_ewkb(&Geometry::Point(Point::new(x, y)), srid)
}

/// ST_MakeLine: build a LineString from two point geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::{st_point, st_make_line};
/// use sqlitegis::core::functions::accessors::st_num_points;
///
/// let a = st_point(0.0, 0.0, None).unwrap();
/// let b = st_point(1.0, 1.0, None).unwrap();
/// let line = st_make_line(&a, &b).unwrap();
/// assert_eq!(st_num_points(&line).unwrap(), 2);
/// ```
pub fn st_make_line(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
    let extract_point = |g: Geometry<f64>| match g {
        Geometry::Point(p) => Ok(p),
        other => Err(SqliteGisError::wrong_type("Point", &other)),
    };
    let pa = extract_point(ga)?;
    let pb = extract_point(gb)?;

    for p in [&pa, &pb] {
        if p.x().is_nan() || p.y().is_nan() {
            return Err(SqliteGisError::InvalidInput(
                "point must not be empty".to_string(),
            ));
        }
        if !p.x().is_finite() || !p.y().is_finite() {
            return Err(SqliteGisError::InvalidInput(
                "point coordinates must be finite".to_string(),
            ));
        }
    }

    let ls = LineString::from(vec![Coord::from(pa), Coord::from(pb)]);
    write_ewkb(&Geometry::LineString(ls), srid)
}

/// ST_MakePolygon: construct a Polygon from a closed shell LineString.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::st_make_polygon;
/// use sqlitegis::core::functions::accessors::st_geometry_type;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let shell = geom_from_text("LINESTRING(0 0,1 0,1 1,0 1,0 0)", None).unwrap();
/// let poly = st_make_polygon(&shell).unwrap();
/// assert_eq!(st_geometry_type(&poly).unwrap(), "ST_Polygon");
/// ```
pub fn st_make_polygon(shell: &[u8]) -> Result<Vec<u8>> {
    let (gs, srid) = parse_ewkb(shell)?;
    let exterior = match gs {
        Geometry::LineString(ls) => ls,
        other => return Err(SqliteGisError::wrong_type("LineString", &other)),
    };

    if exterior.0.len() < 4 {
        return Err(SqliteGisError::InvalidInput(
            "polygon shell must contain at least 4 points".to_string(),
        ));
    }
    if exterior.0.first() != exterior.0.last() {
        return Err(SqliteGisError::InvalidInput(
            "polygon shell must be closed (first point must equal last point)".to_string(),
        ));
    }

    let poly = Polygon::new(exterior, vec![]);
    write_ewkb(&Geometry::Polygon(poly), srid)
}

/// ST_MakeEnvelope: build a rectangular Polygon from four corner coordinates.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::st_make_envelope;
/// use sqlitegis::core::functions::measurement::st_area;
///
/// let blob = st_make_envelope(0.0, 0.0, 2.0, 3.0, None).unwrap();
/// assert!((st_area(&blob).unwrap() - 6.0).abs() < 1e-10);
/// ```
pub fn st_make_envelope(
    xmin: f64,
    ymin: f64,
    xmax: f64,
    ymax: f64,
    srid: Option<i32>,
) -> Result<Vec<u8>> {
    if !xmin.is_finite() || !ymin.is_finite() || !xmax.is_finite() || !ymax.is_finite() {
        return Err(SqliteGisError::InvalidInput(
            "coordinates must be finite".to_string(),
        ));
    }
    if xmin > xmax || ymin > ymax {
        return Err(SqliteGisError::InvalidInput(format!(
            "xmin ({xmin}) must be <= xmax ({xmax}) and ymin ({ymin}) must be <= ymax ({ymax})"
        )));
    }
    let rect = Rect::new(Coord { x: xmin, y: ymin }, Coord { x: xmax, y: ymax });
    write_ewkb(&Geometry::Rect(rect), srid)
}

/// ST_Collect (scalar): combine two geometries into a GeometryCollection.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::{st_point, st_collect};
/// use sqlitegis::core::functions::accessors::{st_num_geometries, st_geometry_type};
///
/// let a = st_point(0.0, 0.0, None).unwrap();
/// let b = st_point(1.0, 1.0, None).unwrap();
/// let gc = st_collect(&a, &b).unwrap();
/// assert_eq!(st_geometry_type(&gc).unwrap(), "ST_GeometryCollection");
/// assert_eq!(st_num_geometries(&gc).unwrap(), 2);
/// ```
pub fn st_collect(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
    let gc = geo::GeometryCollection::new_from(vec![ga, gb]);
    write_ewkb(&Geometry::GeometryCollection(gc), srid)
}

/// Half the Web Mercator circumference in metres (EPSG:3857).
const WEB_MERCATOR_HALF_SIZE: f64 = 20037508.3427892;

/// ST_TileEnvelope: Web Mercator tile bounding box (EPSG:3857).
/// Returns a Polygon in EPSG:3857 coordinates.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::constructors::st_tile_envelope;
/// use sqlitegis::core::functions::accessors::st_srid;
///
/// let tile = st_tile_envelope(0, 0, 0).unwrap();
/// assert_eq!(st_srid(&tile).unwrap(), 3857);
/// ```
pub fn st_tile_envelope(zoom: u32, tile_x: u32, tile_y: u32) -> Result<Vec<u8>> {
    if zoom > 31 {
        return Err(SqliteGisError::InvalidInput(format!(
            "zoom level {zoom} exceeds maximum of 31"
        )));
    }
    let n = 2u32.pow(zoom);
    if tile_x >= n || tile_y >= n {
        return Err(SqliteGisError::InvalidInput(format!(
            "tile coordinates ({tile_x}, {tile_y}) out of range for zoom {zoom} (max {})",
            n - 1
        )));
    }
    let n = n as f64;
    let tile_size = WEB_MERCATOR_HALF_SIZE * 2.0 / n;
    let xmin = tile_x as f64 * tile_size - WEB_MERCATOR_HALF_SIZE;
    let xmax = xmin + tile_size;
    let ymax = WEB_MERCATOR_HALF_SIZE - tile_y as f64 * tile_size;
    let ymin = ymax - tile_size;
    st_make_envelope(xmin, ymin, xmax, ymax, Some(3857))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ewkb::extract_srid;
    use crate::core::functions::accessors::{st_geometry_type, st_num_geometries, st_x, st_y};
    use crate::core::functions::io::geom_from_text;
    use crate::core::functions::measurement::st_area;

    #[test]
    fn st_make_line_non_point_input_a() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_make_line(&line, &pt).is_err());
    }

    #[test]
    fn st_make_line_non_point_input_b() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert!(st_make_line(&pt, &poly).is_err());
    }

    #[test]
    fn st_make_polygon_non_linestring() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_make_polygon(&pt).is_err());
    }

    #[test]
    fn st_make_polygon_rejects_unclosed_shell() {
        let shell = geom_from_text("LINESTRING(0 0,1 0,1 1,0 1)", None).unwrap();
        assert!(st_make_polygon(&shell).is_err());
    }

    #[test]
    fn st_make_polygon_rejects_too_short_shell() {
        let shell = geom_from_text("LINESTRING(0 0,1 1,0 0)", None).unwrap();
        assert!(st_make_polygon(&shell).is_err());
    }

    #[test]
    fn st_make_line_mixed_srid_errors() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, Some(3857)).unwrap();
        assert!(st_make_line(&a, &b).is_err());
    }

    #[test]
    fn st_collect_mixed_srid_errors() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, None).unwrap();
        assert!(st_collect(&a, &b).is_err());
    }

    #[test]
    fn st_tile_envelope_zoom0_covers_world() {
        let tile = st_tile_envelope(0, 0, 0).unwrap();
        let area = st_area(&tile).unwrap();
        assert!(area > 0.0);
    }

    #[test]
    fn st_tile_envelope_zoom1_quarter_of_zoom0() {
        let z0 = st_tile_envelope(0, 0, 0).unwrap();
        let z1 = st_tile_envelope(1, 0, 0).unwrap();
        let area0 = st_area(&z0).unwrap();
        let area1 = st_area(&z1).unwrap();
        assert!((area1 / area0 - 0.25).abs() < 1e-6);
    }

    #[test]
    fn st_point_without_srid() {
        let blob = st_point(1.0, 2.0, None).unwrap();
        assert_eq!(extract_srid(&blob), None);
        assert!((st_x(&blob).unwrap().unwrap() - 1.0).abs() < 1e-10);
        assert!((st_y(&blob).unwrap().unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn st_point_rejects_non_finite_coordinates() {
        assert!(st_point(f64::INFINITY, 0.0, None).is_err());
        assert!(st_point(0.0, f64::NAN, None).is_err());
    }

    #[test]
    fn st_make_envelope_type_check() {
        let blob = st_make_envelope(0.0, 0.0, 1.0, 1.0, Some(4326)).unwrap();
        // st_make_envelope produces a Rect which is serialized as a geometry
        assert_eq!(extract_srid(&blob), Some(4326));
    }

    #[test]
    fn st_make_envelope_rejects_inverted_x() {
        assert!(st_make_envelope(10.0, 0.0, 5.0, 5.0, None).is_err());
    }

    #[test]
    fn st_make_envelope_rejects_inverted_y() {
        assert!(st_make_envelope(0.0, 10.0, 5.0, 3.0, None).is_err());
    }

    #[test]
    fn st_make_envelope_rejects_nan() {
        assert!(st_make_envelope(f64::NAN, 0.0, 5.0, 5.0, None).is_err());
    }

    #[test]
    fn st_make_envelope_degenerate_accepts_equal_coords() {
        // A degenerate envelope (line or point) is valid. xmin==xmax is allowed
        assert!(st_make_envelope(1.0, 1.0, 1.0, 2.0, None).is_ok());
        assert!(st_make_envelope(0.0, 0.0, 0.0, 0.0, None).is_ok());
    }

    #[test]
    fn st_make_line_success() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, Some(4326)).unwrap();
        let line = st_make_line(&a, &b).unwrap();
        assert_eq!(extract_srid(&line), Some(4326));
        assert_eq!(st_geometry_type(&line).unwrap(), "ST_LineString");
    }

    #[test]
    fn st_make_line_rejects_empty_points() {
        let empty = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        let point = st_point(1.0, 1.0, Some(4326)).unwrap();
        assert!(st_make_line(&empty, &point).is_err());
        assert!(st_make_line(&point, &empty).is_err());
    }

    #[test]
    fn st_make_polygon_success() {
        let shell = geom_from_text("LINESTRING(0 0,2 0,2 2,0 2,0 0)", Some(3857)).unwrap();
        let poly = st_make_polygon(&shell).unwrap();
        assert_eq!(extract_srid(&poly), Some(3857));
        assert_eq!(st_geometry_type(&poly).unwrap(), "ST_Polygon");
    }

    #[test]
    fn st_collect_success() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, Some(4326)).unwrap();
        let gc = st_collect(&a, &b).unwrap();
        assert_eq!(extract_srid(&gc), Some(4326));
        assert_eq!(st_geometry_type(&gc).unwrap(), "ST_GeometryCollection");
        assert_eq!(st_num_geometries(&gc).unwrap(), 2);
    }

    #[test]
    fn st_tile_envelope_rejects_invalid_inputs() {
        assert!(st_tile_envelope(32, 0, 0).is_err());
        assert!(st_tile_envelope(1, 2, 0).is_err());
        assert!(st_tile_envelope(1, 0, 2).is_err());
    }

    #[test]
    fn st_make_line_rejects_non_finite_point() {
        use crate::core::ewkb::WKB_POINT;
        // Build a POINT EWKB with INFINITY x and a finite y. Not the POINT EMPTY
        // sentinel (which is NaN on both axes), so the NaN guard at line 56 lets
        // it through and we hit the finite-coord check.
        let mut inf = vec![0x01u8];
        inf.extend_from_slice(&WKB_POINT.to_le_bytes());
        inf.extend_from_slice(&f64::INFINITY.to_le_bytes());
        inf.extend_from_slice(&0.0f64.to_le_bytes());

        let other = st_point(1.0, 1.0, None).unwrap();
        let err = st_make_line(&inf, &other).expect_err("Inf x must be rejected");
        assert!(matches!(err, SqliteGisError::InvalidInput(ref s) if s.contains("finite")));
    }
}
