//! Spatial operations
//!
//! ST_Union, ST_Intersection, ST_Difference, ST_SymDifference, ST_Buffer

use geo::algorithm::bool_ops::BooleanOps;
use geo::algorithm::Buffer;
use geo::{Geometry, MultiPolygon};

use crate::core::error::{GeoLiteError, Result};
use crate::core::ewkb::{geometry_type_name, parse_ewkb, parse_ewkb_pair, write_ewkb};
use crate::core::functions::emptiness::is_empty_geometry;

/// Extract a Polygon or MultiPolygon from a geometry, converting single
/// Polygons into MultiPolygon for uniform BooleanOps handling.
fn require_multi_polygon(geom: Geometry<f64>) -> Result<MultiPolygon<f64>> {
    match geom {
        Geometry::Polygon(p) => Ok(MultiPolygon::new(vec![p])),
        Geometry::MultiPolygon(mp) => Ok(mp),
        other => Err(GeoLiteError::WrongType {
            expected: "Polygon or MultiPolygon",
            actual: geometry_type_name(&other),
        }),
    }
}

fn binary_polygon_op<F>(a: &[u8], b: &[u8], op: F) -> Result<Vec<u8>>
where
    F: FnOnce(&MultiPolygon<f64>, &MultiPolygon<f64>) -> MultiPolygon<f64>,
{
    let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
    let ma = require_multi_polygon(ga)?;
    let mb = require_multi_polygon(gb)?;
    let result = op(&ma, &mb);
    write_ewkb(&Geometry::MultiPolygon(result), srid)
}

/// ST_Union -- compute the geometric union of two polygon geometries.
///
/// # Example
///
/// ```
/// use geolite::core::functions::operations::st_union;
/// use geolite::core::functions::io::geom_from_text;
/// use geolite::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let u = st_union(&a, &b).unwrap();
/// assert!((st_area(&u).unwrap() - 6.0).abs() < 1e-10);
/// ```
pub fn st_union(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    binary_polygon_op(a, b, |ma, mb| ma.union(mb))
}

/// ST_Intersection -- compute the geometric intersection of two polygon geometries.
///
/// # Example
///
/// ```
/// use geolite::core::functions::operations::st_intersection;
/// use geolite::core::functions::io::geom_from_text;
/// use geolite::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let i = st_intersection(&a, &b).unwrap();
/// assert!((st_area(&i).unwrap() - 2.0).abs() < 1e-10);
/// ```
pub fn st_intersection(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    binary_polygon_op(a, b, |ma, mb| ma.intersection(mb))
}

/// ST_Difference -- compute the geometric difference (A minus B) of two polygon geometries.
///
/// # Example
///
/// ```
/// use geolite::core::functions::operations::st_difference;
/// use geolite::core::functions::io::geom_from_text;
/// use geolite::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let d = st_difference(&a, &b).unwrap();
/// assert!((st_area(&d).unwrap() - 2.0).abs() < 1e-10);
/// ```
pub fn st_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    binary_polygon_op(a, b, |ma, mb| ma.difference(mb))
}

/// ST_SymDifference -- compute the symmetric difference (XOR) of two polygon geometries.
///
/// # Example
///
/// ```
/// use geolite::core::functions::operations::st_sym_difference;
/// use geolite::core::functions::io::geom_from_text;
/// use geolite::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let sd = st_sym_difference(&a, &b).unwrap();
/// assert!((st_area(&sd).unwrap() - 4.0).abs() < 1e-10);
/// ```
pub fn st_sym_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    binary_polygon_op(a, b, |ma, mb| ma.xor(mb))
}

/// ST_Buffer -- expand or shrink a geometry by a given distance.
///
/// # Example
///
/// ```
/// use geolite::core::functions::operations::st_buffer;
/// use geolite::core::functions::constructors::st_point;
/// use geolite::core::functions::measurement::st_area;
///
/// let pt = st_point(0.0, 0.0, None).unwrap();
/// let buffered = st_buffer(&pt, 1.0).unwrap();
/// let area = st_area(&buffered).unwrap();
/// // Area of a circle with radius 1 approximately  pi
/// assert!((area - std::f64::consts::PI).abs() < 0.1);
/// ```
pub fn st_buffer(blob: &[u8], distance: f64) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    if is_empty_geometry(&geom) {
        let empty = Geometry::Polygon(geo::Polygon::new(geo::LineString::new(vec![]), vec![]));
        return write_ewkb(&empty, srid);
    }
    let result = geom.buffer(distance);
    let mut polygons = result.0;
    let out_geom = match polygons.len() {
        0 => Geometry::Polygon(geo::Polygon::new(geo::LineString::new(vec![]), vec![])),
        1 => {
            let polygon = polygons.pop().ok_or_else(|| {
                GeoLiteError::InvalidInput(
                    "buffer result unexpectedly missing single polygon".to_string(),
                )
            })?;
            Geometry::Polygon(polygon)
        }
        _ => Geometry::MultiPolygon(MultiPolygon::new(polygons)),
    };
    write_ewkb(&out_geom, srid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::functions::accessors::{st_geometry_type, st_is_empty};
    use crate::core::functions::constructors::st_point;
    use crate::core::functions::io::geom_from_text;
    use crate::core::functions::measurement::st_area;

    #[test]
    fn union_overlapping() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
        let u = st_union(&a, &b).unwrap();
        assert!((st_area(&u).unwrap() - 6.0).abs() < 1e-10);
    }

    #[test]
    fn intersection_overlapping() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
        let i = st_intersection(&a, &b).unwrap();
        assert!((st_area(&i).unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn difference_overlapping() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
        let d = st_difference(&a, &b).unwrap();
        assert!((st_area(&d).unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn sym_difference_overlapping() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
        let sd = st_sym_difference(&a, &b).unwrap();
        assert!((st_area(&sd).unwrap() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn buffer_point() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        let buffered = st_buffer(&pt, 1.0).unwrap();
        let area = st_area(&buffered).unwrap();
        assert!((area - std::f64::consts::PI).abs() < 0.1);
        assert_eq!(st_geometry_type(&buffered).unwrap(), "ST_Polygon");
    }

    #[test]
    fn buffer_multipoint_returns_multipolygon_for_disconnected_components() {
        let mp = geom_from_text("MULTIPOINT((0 0),(10 0))", None).unwrap();
        let buffered = st_buffer(&mp, 1.0).unwrap();
        assert_eq!(st_geometry_type(&buffered).unwrap(), "ST_MultiPolygon");
    }

    #[test]
    fn union_wrong_type() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert!(st_union(&line, &poly).is_err());
    }

    #[test]
    fn union_accepts_multipolygon_inputs() {
        let mp = geom_from_text("MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)))", None).unwrap();
        let poly = geom_from_text("POLYGON((1 0,2 0,2 1,1 1,1 0))", None).unwrap();
        let u = st_union(&mp, &poly).unwrap();
        assert!(st_area(&u).unwrap() > 1.0);
    }

    #[test]
    fn buffer_negative_shrinks() {
        let poly = geom_from_text("POLYGON((0 0,10 0,10 10,0 10,0 0))", None).unwrap();
        let shrunk = st_buffer(&poly, -1.0).unwrap();
        let area = st_area(&shrunk).unwrap();
        assert!(area < 100.0 && area > 0.0);
    }

    #[test]
    fn buffer_empty_polygon_returns_empty_polygon() {
        let empty = geom_from_text("POLYGON EMPTY", None).unwrap();
        let buffered = st_buffer(&empty, 1.0).unwrap();
        assert_eq!(st_geometry_type(&buffered).unwrap(), "ST_Polygon");
        assert!(st_is_empty(&buffered).unwrap());
    }

    #[test]
    fn buffer_empty_point_returns_empty_polygon() {
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        let buffered = st_buffer(&empty, 1.0).unwrap();
        assert_eq!(st_geometry_type(&buffered).unwrap(), "ST_Polygon");
        assert!(st_is_empty(&buffered).unwrap());
    }
}
