//! Spatial operations
//!
//! ST_Union, ST_Intersection, ST_Difference, ST_SymDifference, ST_Buffer

use std::cmp::Ordering;

use geo::algorithm::bool_ops::BooleanOps;
use geo::algorithm::line_intersection::{line_intersection, LineIntersection};
use geo::algorithm::line_measures::metric_spaces::{Euclidean, Geodesic, Haversine};
use geo::algorithm::line_measures::Densify;
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
/// LineString sizes. A Bentley-Ottmann sweep would only pay off for very
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
    super::catch_geo("ST_Union", || {
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
    })
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
    super::catch_geo("ST_Intersection", || {
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
    })
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
    super::catch_geo("ST_Difference", || {
        binary_polygon_op(a, b, |ma, mb| ma.difference(mb))
    })
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
    super::catch_geo("ST_SymDifference", || {
        // MBR-only fastpath. Symmetric difference of disjoint geometries is
        // their union (XOR of non-overlapping sets is the full pair). Same
        // bytes-only splice as `st_union`.
        if let (Ok(Some(ra)), Ok(Some(rb))) = (extract_mbr(a), extract_mbr(b)) {
            if !ra.intersects(&rb) {
                return concat_multipolygon_bodies(a, b);
            }
        }
        binary_polygon_op(a, b, |ma, mb| ma.xor(mb))
    })
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
    super::catch_geo("ST_Buffer", || {
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
    })
}

/// Reject a segment length that is not a positive real number, the way
/// PostGIS does with "invalid max_distance 0 (must be >= 0)".
fn require_segment_length(value: f64, fn_name: &str) -> Result<f64> {
    if !value.is_finite() || value <= 0.0 {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name}: max_segment_length must be finite and greater than 0, got {value}"
        )));
    }
    Ok(value)
}

/// Split every segment of a geometry so none is longer than `max`, measured
/// in `metric`. `geo` densifies lineal and areal shapes, which is the same
/// set PostGIS actually changes.
fn densify_geometry<M: Densify<f64>>(
    geom: &Geometry<f64>,
    metric: &M,
    max: f64,
) -> Result<Geometry<f64>> {
    Ok(match geom {
        Geometry::LineString(ls) => Geometry::LineString(metric.densify(ls, max)),
        Geometry::MultiLineString(mls) => Geometry::MultiLineString(metric.densify(mls, max)),
        Geometry::Polygon(p) => Geometry::Polygon(metric.densify(p, max)),
        Geometry::MultiPolygon(mp) => Geometry::MultiPolygon(metric.densify(mp, max)),
        other => {
            return Err(SqliteGisError::wrong_type(
                "LineString, MultiLineString, Polygon or MultiPolygon",
                other,
            ))
        }
    })
}

/// ST_Segmentize: insert vertices so no segment is longer than
/// `max_segment_length`, in the units of the CRS.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_segmentize;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 4 0)", None).unwrap();
/// let dense = st_segmentize(&line, 2.0).unwrap();
/// assert_eq!(as_text(&dense).unwrap(), "LINESTRING(0 0,2 0,4 0)");
/// ```
pub fn st_segmentize(blob: &[u8], max_segment_length: f64) -> Result<Vec<u8>> {
    let max = require_segment_length(max_segment_length, "ST_Segmentize")?;
    let (geom, srid) = parse_ewkb(blob)?;
    write_ewkb(&densify_geometry(&geom, &Euclidean, max)?, srid)
}

/// ST_SegmentizeSphere: `ST_Segmentize` with the limit in metres along a
/// sphere of the mean earth radius (SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_segmentize_sphere;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_segmentize_sphere(&line, 3_000_000.0).is_ok());
/// ```
pub fn st_segmentize_sphere(blob: &[u8], max_segment_length: f64) -> Result<Vec<u8>> {
    let max = require_segment_length(max_segment_length, "ST_SegmentizeSphere")?;
    let (geom, srid) = parse_geographic("ST_SegmentizeSphere", blob)?;
    write_ewkb(&densify_geometry(&geom, &Haversine, max)?, srid)
}

/// ST_SegmentizeSpheroid: `ST_Segmentize` with the limit in metres along the
/// WGS84 ellipsoid (Karney, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::operations::st_segmentize_spheroid;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_segmentize_spheroid(&line, 3_000_000.0).is_ok());
/// ```
pub fn st_segmentize_spheroid(blob: &[u8], max_segment_length: f64) -> Result<Vec<u8>> {
    let max = require_segment_length(max_segment_length, "ST_SegmentizeSpheroid")?;
    let (geom, srid) = parse_geographic("ST_SegmentizeSpheroid", blob)?;
    write_ewkb(&densify_geometry(&geom, &Geodesic, max)?, srid)
}

/// Parse for the curved segmentize forms: SRID 4326 and every vertex latitude
/// in range, matching the rest of the curved-earth surface.
fn parse_geographic(fn_name: &str, blob: &[u8]) -> Result<(Geometry<f64>, Option<i32>)> {
    use crate::core::functions::measurement::{
        ensure_geographic_srid, require_geographic_line_latitudes,
    };
    let (geom, srid) = parse_ewkb(blob)?;
    ensure_geographic_srid(srid, fn_name)?;
    match &geom {
        Geometry::LineString(ls) => require_geographic_line_latitudes(ls, fn_name)?,
        Geometry::MultiLineString(mls) => {
            for ls in &mls.0 {
                require_geographic_line_latitudes(ls, fn_name)?;
            }
        }
        Geometry::Polygon(p) => require_geographic_polygon_rings(p, fn_name)?,
        Geometry::MultiPolygon(mp) => {
            for p in &mp.0 {
                require_geographic_polygon_rings(p, fn_name)?;
            }
        }
        _ => {}
    }
    Ok((geom, srid))
}

fn require_geographic_polygon_rings(poly: &Polygon<f64>, fn_name: &str) -> Result<()> {
    use crate::core::functions::measurement::require_geographic_line_latitudes;
    require_geographic_line_latitudes(poly.exterior(), fn_name)?;
    for ring in poly.interiors() {
        require_geographic_line_latitudes(ring, fn_name)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::functions::accessors::{st_geometry_type, st_is_empty};
    use crate::core::functions::constructors::st_point;
    use crate::core::functions::io::geom_from_text;
    use crate::core::functions::measurement::{st_area, st_point_on_surface};

    /// Fuzzer-reduced degenerate-but-finite polygon that makes `i_overlay` and
    /// geo's sweeps `assert!` and abort. The guard must turn that into an error.
    const DEGENERATE_POLYGON: &[u8] = &[
        1, 3, 0, 0, 0, 6, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 229, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 129, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 229, 229, 229,
        229, 229, 229, 229, 229, 77, 229, 229, 28, 229, 229, 229, 229, 229, 229, 229, 229,
    ];

    /// Every op must return, never abort, on the degenerate polygon above.
    #[test]
    fn degenerate_geometry_never_panics() {
        let g = DEGENERATE_POLYGON;
        let _ = st_buffer(g, 1.0);
        let _ = st_union(g, g);
        let _ = st_intersection(g, g);
        let _ = st_difference(g, g);
        let _ = st_sym_difference(g, g);
        let _ = st_point_on_surface(g);
    }

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
    #[test]
    fn intersection_multipoint_multipoint_shared_point() {
        let a = geom_from_text("MULTIPOINT((0 0),(1 1))", None).unwrap();
        let b = geom_from_text("MULTIPOINT((1 1),(2 2))", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert_eq!(as_text(&r).unwrap(), "POINT(1 1)");
    }

    #[test]
    fn intersection_parallel_linestrings_no_intersection() {
        let a = geom_from_text("LINESTRING(0 0,0 10)", None).unwrap();
        let b = geom_from_text("LINESTRING(1 0,1 10)", None).unwrap();
        let r = st_intersection(&a, &b).unwrap();
        assert!(st_is_empty(&r).unwrap());
    }

    #[test]
    fn union_overlapping_slow_path() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 1,3 1,3 3,1 3,1 1))", None).unwrap();
        let u = st_union(&a, &b).unwrap();
        assert!((st_area(&u).unwrap() - 7.0).abs() < 1e-10);
    }

    #[test]
    fn sym_difference_overlapping_slow_path() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,3 0,3 2,1 2,1 0))", None).unwrap();
        let sd = st_sym_difference(&a, &b).unwrap();
        assert!((st_area(&sd).unwrap() - 4.0).abs() < 1e-10);
    }

    #[test]
    fn buffer_negative_large_distance_empty_result() {
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let result = st_buffer(&poly, -10.0).unwrap();
        assert_eq!(st_geometry_type(&result).unwrap(), "ST_Polygon");
        assert!(st_is_empty(&result).unwrap());
    }

    #[test]
    fn intersection_geometrycollection_mixed_point_and_polygon() {
        let gc = geom_from_text(
            "GEOMETRYCOLLECTION(POINT(0.5 0.5), POLYGON((0 0,1 0,1 1,0 1,0 0)))",
            None,
        )
        .unwrap();
        let r = st_intersection(&gc, &gc).unwrap();
        assert_eq!(st_geometry_type(&r).unwrap(), "ST_GeometryCollection");
        let text = as_text(&r).unwrap();
        assert!(text.contains("POINT(0.5 0.5)"), "actual: {text}");
        assert!(text.contains("POLYGON"), "actual: {text}");
    }

    // -- Segmentize ---------------------------------------------------

    /// Reference values are PostGIS 3.5 readings. Its `geography` form
    /// measures on the sphere, so `ST_SegmentizeSpheroid` has no PostGIS
    /// counterpart to read and is pinned to the geodesic interpolation of
    /// the same line instead.
    fn assert_wkt_close(got: &str, want: &str) {
        let nums = |s: &str| -> Vec<f64> {
            s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse::<f64>().ok())
                .collect()
        };
        let (g, w) = (nums(got), nums(want));
        assert_eq!(g.len(), w.len(), "shape differs: got {got}, want {want}");
        for (a, b) in g.iter().zip(w.iter()) {
            assert!((a - b).abs() < 1e-9, "got {got}, want {want}");
        }
    }

    #[test]
    fn segmentize_planar_matches_postgis() {
        let line = geom_from_text("LINESTRING(0 0, 90 60)", None).unwrap();
        let out = st_segmentize(&line, 30.0).unwrap();
        assert_wkt_close(
            &as_text(&out).unwrap(),
            "LINESTRING(0 0,22.5 15,45 30,67.5 45,90 60)",
        );
    }

    #[test]
    fn segmentize_sphere_matches_postgis_geography() {
        let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
        let out = st_segmentize_sphere(&line, 3_000_000.0).unwrap();
        assert_wkt_close(
            &as_text(&out).unwrap(),
            "LINESTRING(0 0,11.700919508154 19.35459615165,26.565051177078 37.761243907035,50.360727762244 53.139953123517,90 60)",
        );
    }

    #[test]
    fn segmentize_spheroid_lands_on_geodesic_interpolations() {
        let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
        let out = st_segmentize_spheroid(&line, 3_000_000.0).unwrap();
        // PostGIS readings of ST_LineInterpolatePoint(geography, f, true).
        assert_wkt_close(
            &as_text(&out).unwrap(),
            "LINESTRING(0 0,11.716741571563 19.435698270019,26.606325079977 37.874403088301,50.432454297567 53.213438567481,90 60)",
        );
    }

    #[test]
    fn segmentize_leaves_a_short_enough_line_alone() {
        let line = geom_from_text("LINESTRING(0 0, 1 0)", None).unwrap();
        let out = st_segmentize(&line, 10.0).unwrap();
        assert_wkt_close(&as_text(&out).unwrap(), "LINESTRING(0 0,1 0)");
    }

    #[test]
    fn segmentize_densifies_polygon_rings() {
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let out = st_segmentize(&poly, 0.6).unwrap();
        assert_wkt_close(
            &as_text(&out).unwrap(),
            "POLYGON((0 0,0.5 0,1 0,1 0.5,1 1,0.5 1,0 1,0 0.5,0 0))",
        );
    }

    #[test]
    fn segmentize_rejects_a_non_positive_length() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let line = geom_from_text("LINESTRING(0 0, 1 1)", None).unwrap();
            let err = st_segmentize(&line, bad).expect_err("must reject");
            assert!(
                format!("{err}").contains("max_segment_length"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn segmentize_rejects_shapes_without_segments() {
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        let geographic_pt = geom_from_text("POINT(0 0)", Some(4326)).unwrap();
        // The curved forms reach the same rejection past their SRID and
        // latitude guards, which have no ring to walk on a Point.
        for err in [
            st_segmentize(&pt, 1.0).expect_err("a point has no segments"),
            st_segmentize_sphere(&geographic_pt, 1000.0).expect_err("a point has no segments"),
            st_segmentize_spheroid(&geographic_pt, 1000.0).expect_err("a point has no segments"),
        ] {
            assert!(
                format!("{err}").contains("LineString"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn curved_segmentize_requires_srid_4326() {
        let line = geom_from_text("LINESTRING(0 0, 1 1)", None).unwrap();
        for err in [
            st_segmentize_sphere(&line, 1000.0).expect_err("needs SRID"),
            st_segmentize_spheroid(&line, 1000.0).expect_err("needs SRID"),
        ] {
            assert!(
                format!("{err}").contains("requires SRID 4326"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn curved_segmentize_rejects_out_of_range_latitude() {
        let bad = geom_from_text("LINESTRING(0 0, 10 95)", Some(4326)).unwrap();
        assert!(st_segmentize_sphere(&bad, 100_000.0).is_err());
        assert!(st_segmentize_spheroid(&bad, 100_000.0).is_err());
    }

    #[test]
    fn segmentize_planar_handles_multi_part_shapes() {
        let multiline = geom_from_text("MULTILINESTRING((0 0,4 0),(0 2,2 2))", None).unwrap();
        assert_wkt_close(
            &as_text(&st_segmentize(&multiline, 2.0).unwrap()).unwrap(),
            "MULTILINESTRING((0 0,2 0,4 0),(0 2,2 2))",
        );

        let multipoly = geom_from_text(
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((5 5,7 5,7 7,5 7,5 5)))",
            None,
        )
        .unwrap();
        assert_wkt_close(
            &as_text(&st_segmentize(&multipoly, 2.0).unwrap()).unwrap(),
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((5 5,7 5,7 7,5 7,5 5)))",
        );
    }

    #[test]
    fn segmentize_sphere_bulges_polygon_rings_along_great_circles() {
        // The inserted vertex on the northern edge sits at 10.0374 rather than
        // 10, because a great circle between two points at the same latitude
        // bows poleward. PostGIS reads the same value off `geography`.
        let poly = geom_from_text("POLYGON((0 0,10 0,10 10,0 10,0 0))", Some(4326)).unwrap();
        assert_wkt_close(
            &as_text(&st_segmentize_sphere(&poly, 700_000.0).unwrap()).unwrap(),
            "POLYGON((0 0,5 0,10 0,10 5,10 10,5 10.037423045911,0 10,0 5,0 0))",
        );
    }

    #[test]
    fn segmentize_sphere_handles_multilinestring() {
        let multiline =
            geom_from_text("MULTILINESTRING((0 0,10 0),(0 2,10 2))", Some(4326)).unwrap();
        assert_wkt_close(
            &as_text(&st_segmentize_sphere(&multiline, 700_000.0).unwrap()).unwrap(),
            "MULTILINESTRING((0 0,5 0,10 0),(0 2,5 2.007633435231,10 2))",
        );
    }

    #[test]
    fn segmentize_spheroid_handles_multipolygon() {
        let multipoly = geom_from_text(
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((5 5,7 5,7 7,5 7,5 5)))",
            Some(4326),
        )
        .unwrap();
        let out = st_segmentize_spheroid(&multipoly, 100_000.0).unwrap();
        let (geom, srid) = parse_ewkb(&out).unwrap();
        assert_eq!(srid, Some(4326));
        let Geometry::MultiPolygon(parts) = geom else {
            panic!("expected a MultiPolygon, got {geom:?}");
        };
        assert_eq!(parts.0.len(), 2);
        // Each 2-degree ring is roughly 222 km a side, so a 100 km limit has
        // to have split every side of both parts.
        for part in &parts.0 {
            assert!(
                part.exterior().0.len() > 5,
                "part was left undensified: {part:?}"
            );
        }
    }

    #[test]
    fn curved_segmentize_checks_every_ring_of_every_part() {
        // Exercises the polygon and multipolygon arms of the latitude guard,
        // including interior rings, which the LineString case never reaches.
        let bad_shell = geom_from_text("POLYGON((0 0,10 0,10 95,0 95,0 0))", Some(4326)).unwrap();
        let bad_hole = geom_from_text(
            "POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 95,1 95,1 1))",
            Some(4326),
        )
        .unwrap();
        let bad_part = geom_from_text(
            "MULTIPOLYGON(((0 0,2 0,2 2,0 2,0 0)),((20 20,21 20,21 95,20 95,20 20)))",
            Some(4326),
        )
        .unwrap();
        let bad_multiline =
            geom_from_text("MULTILINESTRING((0 0,1 1),(2 2,3 95))", Some(4326)).unwrap();

        for geom in [&bad_shell, &bad_hole, &bad_part, &bad_multiline] {
            for err in [
                st_segmentize_sphere(geom, 100_000.0).expect_err("out-of-range latitude"),
                st_segmentize_spheroid(geom, 100_000.0).expect_err("out-of-range latitude"),
            ] {
                assert!(
                    format!("{err}").contains("latitude"),
                    "unexpected error: {err}"
                );
            }
        }
    }
}
