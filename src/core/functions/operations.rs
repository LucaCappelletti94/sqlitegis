//! Spatial operations
//!
//! ST_Union, ST_Intersection, ST_Difference, ST_SymDifference, ST_Buffer

use std::cmp::Ordering;

use geo::algorithm::bool_ops::BooleanOps;
use geo::algorithm::line_intersection::{line_intersection, LineIntersection};
use geo::algorithm::Buffer;
use geo::algorithm::Intersects;
use geo::{
    Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon, Point,
    Polygon,
};

use crate::core::error::{Result, SqliteGisError};
use crate::core::ewkb::{
    concat_multipolygon_bodies, extract_mbr, extract_srid, parse_ewkb, parse_ewkb_pair, write_ewkb,
};
use crate::core::functions::emptiness::{is_empty_geometry, is_empty_point};

/// Extract a Polygon or MultiPolygon from a geometry, converting single
/// Polygons into MultiPolygon for uniform BooleanOps handling.
fn require_multi_polygon(geom: Geometry<f64>) -> Result<MultiPolygon<f64>> {
    match geom {
        Geometry::Polygon(p) => Ok(MultiPolygon::new(vec![p])),
        Geometry::MultiPolygon(mp) => Ok(mp),
        other => Err(SqliteGisError::wrong_type(
            "Polygon or MultiPolygon",
            &other,
        )),
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

/// Bag of homogeneous-typed pieces extracted from a possibly-nested input.
///
/// `ST_Intersection` accepts any geometry on either side. We normalise the
/// inputs by decomposing them into points, line strings, and polygons, then
/// intersect the bags pair-wise, then pack the smallest matching variant on
/// the way out.
#[derive(Default)]
struct GeometryBag {
    points: Vec<Point<f64>>,
    lines: Vec<LineString<f64>>,
    polygons: Vec<Polygon<f64>>,
}

impl GeometryBag {
    fn new() -> Self {
        Self::default()
    }
}

fn decompose_into(geom: Geometry<f64>, bag: &mut GeometryBag) -> Result<()> {
    match geom {
        Geometry::Point(p) => {
            if !is_empty_point(&p) {
                bag.points.push(p);
            }
        }
        Geometry::MultiPoint(mp) => {
            for p in mp.0 {
                if !is_empty_point(&p) {
                    bag.points.push(p);
                }
            }
        }
        Geometry::LineString(ls) => {
            if !ls.0.is_empty() {
                bag.lines.push(ls);
            }
        }
        Geometry::MultiLineString(mls) => {
            for ls in mls.0 {
                if !ls.0.is_empty() {
                    bag.lines.push(ls);
                }
            }
        }
        Geometry::Polygon(p) => {
            if !p.exterior().0.is_empty() {
                bag.polygons.push(p);
            }
        }
        Geometry::MultiPolygon(mp) => {
            for p in mp.0 {
                if !p.exterior().0.is_empty() {
                    bag.polygons.push(p);
                }
            }
        }
        Geometry::GeometryCollection(gc) => {
            for g in gc.0 {
                decompose_into(g, bag)?;
            }
        }
        other => {
            return Err(SqliteGisError::wrong_type(
                "Point, LineString, Polygon, or a Multi/Collection of these",
                &other,
            ));
        }
    }
    Ok(())
}

fn intersect_bags(a: &GeometryBag, b: &GeometryBag) -> GeometryBag {
    let mut out = GeometryBag::new();

    if !a.polygons.is_empty() && !b.polygons.is_empty() {
        let ma = MultiPolygon::new(a.polygons.clone());
        let mb = MultiPolygon::new(b.polygons.clone());
        let result = ma.intersection(&mb);
        out.polygons.extend(result.0);
    }

    if !a.lines.is_empty() && !b.polygons.is_empty() {
        let mls = MultiLineString::new(a.lines.clone());
        let mb = MultiPolygon::new(b.polygons.clone());
        let clipped = mb.clip(&mls, false);
        out.lines.extend(clipped.0);
    }

    if !a.polygons.is_empty() && !b.lines.is_empty() {
        let ma = MultiPolygon::new(a.polygons.clone());
        let mls = MultiLineString::new(b.lines.clone());
        let clipped = ma.clip(&mls, false);
        out.lines.extend(clipped.0);
    }

    if !a.points.is_empty() && !b.polygons.is_empty() {
        let mb = MultiPolygon::new(b.polygons.clone());
        for p in &a.points {
            if mb.intersects(p) {
                out.points.push(*p);
            }
        }
    }

    if !a.polygons.is_empty() && !b.points.is_empty() {
        let ma = MultiPolygon::new(a.polygons.clone());
        for p in &b.points {
            if ma.intersects(p) {
                out.points.push(*p);
            }
        }
    }

    if !a.points.is_empty() && !b.lines.is_empty() {
        let mls = MultiLineString::new(b.lines.clone());
        for p in &a.points {
            if mls.intersects(p) {
                out.points.push(*p);
            }
        }
    }

    if !a.lines.is_empty() && !b.points.is_empty() {
        let mls = MultiLineString::new(a.lines.clone());
        for p in &b.points {
            if mls.intersects(p) {
                out.points.push(*p);
            }
        }
    }

    if !a.points.is_empty() && !b.points.is_empty() {
        for pa in &a.points {
            for pb in &b.points {
                if pa.x() == pb.x() && pa.y() == pb.y() {
                    out.points.push(*pa);
                    break;
                }
            }
        }
    }

    if !a.lines.is_empty() && !b.lines.is_empty() {
        intersect_lines_into(&a.lines, &b.lines, &mut out);
    }

    out
}

/// Naive O(n*m) pairwise segment-intersection sweep. Sufficient for typical
/// LineString sizes; a Bentley-Ottmann sweep would only pay off for very
/// long, very sparse-intersection inputs.
fn intersect_lines_into(a: &[LineString<f64>], b: &[LineString<f64>], out: &mut GeometryBag) {
    let mut collinear: Vec<LineString<f64>> = Vec::new();
    for la in a {
        for seg_a in la.lines() {
            for lb in b {
                for seg_b in lb.lines() {
                    match line_intersection(seg_a, seg_b) {
                        Some(LineIntersection::SinglePoint { intersection, .. }) => {
                            out.points.push(Point::new(intersection.x, intersection.y));
                        }
                        Some(LineIntersection::Collinear { intersection }) => {
                            collinear.push(LineString::from(vec![
                                (intersection.start.x, intersection.start.y),
                                (intersection.end.x, intersection.end.y),
                            ]));
                        }
                        None => {}
                    }
                }
            }
        }
    }
    out.lines.extend(collinear);
}

fn coord_cmp(a: &Point<f64>, b: &Point<f64>) -> Ordering {
    a.x()
        .partial_cmp(&b.x())
        .unwrap_or(Ordering::Equal)
        .then(a.y().partial_cmp(&b.y()).unwrap_or(Ordering::Equal))
}

fn pack(bag: GeometryBag) -> Geometry<f64> {
    let GeometryBag {
        mut points,
        lines,
        polygons,
    } = bag;

    points.sort_by(coord_cmp);
    points.dedup_by(|a, b| a.x() == b.x() && a.y() == b.y());

    let has_points = !points.is_empty();
    let has_lines = !lines.is_empty();
    let has_polygons = !polygons.is_empty();
    let kinds = (has_points as u8) + (has_lines as u8) + (has_polygons as u8);

    if kinds == 0 {
        return Geometry::GeometryCollection(GeometryCollection::new_from(vec![]));
    }

    if kinds > 1 {
        let mut parts: Vec<Geometry<f64>> = Vec::new();
        if points.len() == 1 {
            parts.push(Geometry::Point(points.into_iter().next().unwrap()));
        } else if !points.is_empty() {
            parts.push(Geometry::MultiPoint(MultiPoint::new(points)));
        }
        if lines.len() == 1 {
            parts.push(Geometry::LineString(lines.into_iter().next().unwrap()));
        } else if !lines.is_empty() {
            parts.push(Geometry::MultiLineString(MultiLineString::new(lines)));
        }
        if polygons.len() == 1 {
            parts.push(Geometry::Polygon(polygons.into_iter().next().unwrap()));
        } else if !polygons.is_empty() {
            parts.push(Geometry::MultiPolygon(MultiPolygon::new(polygons)));
        }
        return Geometry::GeometryCollection(GeometryCollection::new_from(parts));
    }

    if has_points {
        return if points.len() == 1 {
            Geometry::Point(points.into_iter().next().unwrap())
        } else {
            Geometry::MultiPoint(MultiPoint::new(points))
        };
    }
    if has_lines {
        return if lines.len() == 1 {
            Geometry::LineString(lines.into_iter().next().unwrap())
        } else {
            Geometry::MultiLineString(MultiLineString::new(lines))
        };
    }
    if polygons.len() == 1 {
        Geometry::Polygon(polygons.into_iter().next().unwrap())
    } else {
        Geometry::MultiPolygon(MultiPolygon::new(polygons))
    }
}

/// ST_Union: compute the geometric union of two polygon geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_union;
/// use sqlitegis::core::functions::io::geom_from_text;
/// use sqlitegis::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let u = st_union(&a, &b).unwrap();
/// assert!((st_area(&u).unwrap() - 6.0).abs() < 1e-10);
/// ```
pub fn st_union(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    // MBR-only fastpath. If both bboxes exist and are disjoint, the
    // union is simply the concatenation of both polygon lists. We splice
    // the input EWKB bytes directly without decoding, which is several
    // times faster than the decode + Vec + serialize path.
    if let (Ok(Some(ra)), Ok(Some(rb))) = (extract_mbr(a), extract_mbr(b)) {
        if !ra.intersects(&rb) {
            return concat_multipolygon_bodies(a, b);
        }
    }
    binary_polygon_op(a, b, |ma, mb| ma.union(mb))
}

/// ST_Intersection: compute the geometric intersection of two geometries.
///
/// Accepts any combination of Point, LineString, Polygon, their Multi*
/// variants, and GeometryCollection on either side. The result is packed
/// into the smallest matching variant (single primitive, single Multi*,
/// or GeometryCollection for mixed-dimension results). Disjoint inputs
/// return an empty GeometryCollection.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_intersection;
/// use sqlitegis::core::functions::io::geom_from_text;
/// use sqlitegis::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let i = st_intersection(&a, &b).unwrap();
/// assert!((st_area(&i).unwrap() - 2.0).abs() < 1e-10);
/// ```
///
/// Point inside polygon returns the point:
///
/// ```
/// use sqlitegis::core::functions::operations::st_intersection;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let pt = geom_from_text("POINT(1 1)", None).unwrap();
/// let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let r = st_intersection(&pt, &poly).unwrap();
/// assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
/// ```
///
/// Two crossing line strings yield the crossing point:
///
/// ```
/// use sqlitegis::core::functions::operations::st_intersection;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let a = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
/// let b = geom_from_text("LINESTRING(0 2,2 0)", None).unwrap();
/// let r = st_intersection(&a, &b).unwrap();
/// assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
/// ```
pub fn st_intersection(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    if let (Ok(Some(ra)), Ok(Some(rb))) = (extract_mbr(a), extract_mbr(b)) {
        if !ra.intersects(&rb) {
            let empty = Geometry::GeometryCollection(GeometryCollection::new_from(vec![]));
            return write_ewkb(&empty, extract_srid(a));
        }
    }
    let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
    let mut bag_a = GeometryBag::new();
    let mut bag_b = GeometryBag::new();
    decompose_into(ga, &mut bag_a)?;
    decompose_into(gb, &mut bag_b)?;
    let result = intersect_bags(&bag_a, &bag_b);
    write_ewkb(&pack(result), srid)
}

/// ST_Difference: compute the geometric difference (A minus B) of two polygon geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_difference;
/// use sqlitegis::core::functions::io::geom_from_text;
/// use sqlitegis::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let d = st_difference(&a, &b).unwrap();
/// assert!((st_area(&d).unwrap() - 2.0).abs() < 1e-10);
/// ```
pub fn st_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    binary_polygon_op(a, b, |ma, mb| ma.difference(mb))
}

/// ST_SymDifference: compute the symmetric difference (XOR) of two polygon geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_sym_difference;
/// use sqlitegis::core::functions::io::geom_from_text;
/// use sqlitegis::core::functions::measurement::st_area;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
/// let sd = st_sym_difference(&a, &b).unwrap();
/// assert!((st_area(&sd).unwrap() - 4.0).abs() < 1e-10);
/// ```
pub fn st_sym_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    // MBR-only fastpath. Symmetric difference of disjoint geometries is
    // their union (XOR of non-overlapping sets is the full pair). Same
    // bytes-only splice as `st_union`.
    if let (Ok(Some(ra)), Ok(Some(rb))) = (extract_mbr(a), extract_mbr(b)) {
        if !ra.intersects(&rb) {
            return concat_multipolygon_bodies(a, b);
        }
    }
    binary_polygon_op(a, b, |ma, mb| ma.xor(mb))
}

/// ST_Buffer: expand or shrink a geometry by a given distance.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_buffer;
/// use sqlitegis::core::functions::constructors::st_point;
/// use sqlitegis::core::functions::measurement::st_area;
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
                SqliteGisError::InvalidInput(
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

    use crate::core::functions::io::as_text;

    #[test]
    fn intersection_point_point_match() {
        let a = geom_from_text("POINT(1 2)", None).unwrap();
        let b = geom_from_text("POINT(1 2)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 2)");
    }

    #[test]
    fn intersection_point_point_disjoint_is_empty() {
        let a = geom_from_text("POINT(1 2)", None).unwrap();
        let b = geom_from_text("POINT(3 4)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert!(st_is_empty(&r).unwrap());
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_GeometryCollection");
    }

    #[test]
    fn intersection_point_in_polygon() {
        let pt = geom_from_text("POINT(1 1)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&pt, &poly).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_point_outside_polygon_is_empty() {
        let pt = geom_from_text("POINT(5 5)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&pt, &poly).unwrap();
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn intersection_polygon_point_swapped() {
        let pt = geom_from_text("POINT(1 1)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&poly, &pt).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_point_on_linestring() {
        let pt = geom_from_text("POINT(1 1)", None).unwrap();
        let ls = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let r = st_intersection(&pt, &ls).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_point_off_linestring_is_empty() {
        let pt = geom_from_text("POINT(1 0)", None).unwrap();
        let ls = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let r = st_intersection(&pt, &ls).unwrap();
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn intersection_linestring_polygon_clips() {
        let ls = geom_from_text("LINESTRING(-1 1,3 1)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&ls, &poly).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_LineString");
        assert_eq!(as_text(&r).unwrap(), "LINESTRING(0 1,2 1)");
    }

    #[test]
    fn intersection_polygon_linestring_swapped_clips() {
        let ls = geom_from_text("LINESTRING(-1 1,3 1)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&poly, &ls).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_LineString");
        assert_eq!(as_text(&r).unwrap(), "LINESTRING(0 1,2 1)");
    }

    #[test]
    fn intersection_linestring_disjoint_polygon_is_empty() {
        let ls = geom_from_text("LINESTRING(10 10,20 20)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&ls, &poly).unwrap();
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn intersection_two_crossing_linestrings_point() {
        let a = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let b = geom_from_text("LINESTRING(0 2,2 0)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_collinear_linestrings_yield_overlap_linestring() {
        let a = geom_from_text("LINESTRING(0 0,4 0)", None).unwrap();
        let b = geom_from_text("LINESTRING(2 0,6 0)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_LineString");
        assert_eq!(as_text(&r).unwrap(), "LINESTRING(2 0,4 0)");
    }

    #[test]
    fn intersection_parallel_linestrings_disjoint_is_empty() {
        let a = geom_from_text("LINESTRING(0 0,2 0)", None).unwrap();
        let b = geom_from_text("LINESTRING(0 1,2 1)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn intersection_multipoint_polygon_keeps_inside_points() {
        let mp = geom_from_text("MULTIPOINT((1 1),(5 5),(0 0))", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&mp, &poly).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_MultiPoint");
        let text = as_text(&r).unwrap();
        assert!(text.contains("0 0"), "actual: {text}");
        assert!(text.contains("1 1"), "actual: {text}");
        assert!(!text.contains("5 5"), "actual: {text}");
    }

    #[test]
    fn intersection_geometrycollection_input_dispatches_per_part() {
        let gc =
            geom_from_text("GEOMETRYCOLLECTION(POINT(1 1),LINESTRING(-1 1,3 1))", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&gc, &poly).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_GeometryCollection");
        let text = as_text(&r).unwrap();
        assert!(text.contains("POINT(1 1)"));
        assert!(text.contains("LINESTRING(0 1,2 1)"));
    }

    #[test]
    fn intersection_rejects_unsupported_type() {
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let mut bag = GeometryBag::new();
        let rect = Geometry::Rect(geo::Rect::new(
            geo::Coord { x: 0.0, y: 0.0 },
            geo::Coord { x: 1.0, y: 1.0 },
        ));
        assert!(decompose_into(rect, &mut bag).is_err());
        let _ = (pt, poly);
    }

    #[test]
    fn intersection_polygon_multipoint_swapped_filters_outside() {
        let mp = geom_from_text("MULTIPOINT((1 1),(5 5),(0 0))", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&poly, &mp).unwrap();
        let text = as_text(&r).unwrap();
        assert!(text.contains("0 0") && text.contains("1 1"));
        assert!(!text.contains("5 5"));
    }

    #[test]
    fn intersection_multipoint_linestring_keeps_on_line_points() {
        let mp = geom_from_text("MULTIPOINT((1 1),(5 5))", None).unwrap();
        let ls = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let r = st_intersection(&mp, &ls).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_linestring_multipoint_swapped_keeps_on_line_points() {
        let mp = geom_from_text("MULTIPOINT((1 1),(5 5))", None).unwrap();
        let ls = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let r = st_intersection(&ls, &mp).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_disjoint_multipolygon_yields_multipolygon() {
        let a = geom_from_text(
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((10 0,12 0,12 2,10 2,10 0)))",
            None,
        )
        .unwrap();
        let b = geom_from_text(
            "MULTIPOLYGON(((1 0,3 0,3 2,1 2,1 0)),((11 0,13 0,13 2,11 2,11 0)))",
            None,
        )
        .unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_MultiPolygon");
    }

    #[test]
    fn intersection_multilinestring_polygon_yields_multilinestring() {
        let mls = geom_from_text("MULTILINESTRING((-1 1,3 1),(-1 0.5,3 0.5))", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&mls, &poly).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_MultiLineString");
    }

    #[test]
    fn intersection_geometrycollection_mixed_multi_parts() {
        let gc = geom_from_text(
            "GEOMETRYCOLLECTION(MULTIPOINT((1 1),(0 0)),MULTILINESTRING((-1 1,3 1),(-1 0.5,3 0.5)))",
            None,
        )
        .unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&gc, &poly).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_GeometryCollection");
        let text = as_text(&r).unwrap();
        assert!(text.contains("MULTIPOINT"), "actual: {text}");
        assert!(text.contains("MULTILINESTRING"), "actual: {text}");
    }

    #[test]
    fn intersection_nested_geometrycollection_recurses() {
        let gc =
            geom_from_text("GEOMETRYCOLLECTION(GEOMETRYCOLLECTION(POINT(1 1)))", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&gc, &poly).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_empty_inputs_decompose_cleanly() {
        let empty_mp = geom_from_text("MULTIPOINT EMPTY", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let r = st_intersection(&empty_mp, &poly).unwrap();
        assert!(st_is_empty(&r).unwrap());

        let empty_mls = geom_from_text("MULTILINESTRING EMPTY", None).unwrap();
        let r2 = st_intersection(&empty_mls, &poly).unwrap();
        assert!(st_is_empty(&r2).unwrap());

        let empty_mpoly = geom_from_text("MULTIPOLYGON EMPTY", None).unwrap();
        let r3 = st_intersection(&empty_mpoly, &poly).unwrap();
        assert!(st_is_empty(&r3).unwrap());
    }

    #[test]
    fn sym_difference_disjoint_concatenates_polygons() {
        let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((10 10,11 10,11 11,10 11,10 10))", None).unwrap();
        let sd = st_sym_difference(&a, &b).unwrap();
        assert!((st_area(&sd).unwrap() - 2.0).abs() < 1e-10);
        assert_eq!(st_geometry_type(&sd).unwrap(), "ST_MultiPolygon");
    }

    #[test]
    fn union_disjoint_concatenates_polygons() {
        let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((10 10,11 10,11 11,10 11,10 10))", None).unwrap();
        let u = st_union(&a, &b).unwrap();
        // Two unit squares: total area 2.0.
        assert!((st_area(&u).unwrap() - 2.0).abs() < 1e-10);
        assert_eq!(st_geometry_type(&u).unwrap(), "ST_MultiPolygon");
    }

    #[test]
    fn intersection_mbr_disjoint_returns_empty_geometrycollection() {
        let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((10 10,11 10,11 11,10 11,10 10))", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_GeometryCollection");
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn intersection_disconnected_linestring_polygon_yields_multilinestring() {
        let ls = geom_from_text("LINESTRING(-1 1,3 1,3 5,5 5,5 1,7 1)", None).unwrap();
        let poly_a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let poly_b = geom_from_text("POLYGON((4 0,6 0,6 2,4 2,4 0))", None).unwrap();
        let mp = geom_from_text(
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((4 0,6 0,6 2,4 2,4 0)))",
            None,
        )
        .unwrap();
        let r = st_intersection(&ls, &mp).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_MultiLineString");
        let _ = (poly_a, poly_b);
    }
}
