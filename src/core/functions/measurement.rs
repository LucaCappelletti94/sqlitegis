//! Measurement functions.
//!
//! ST_Area, ST_Perimeter, ST_Length, ST_Length2D, ST_Distance,
//! ST_Centroid, ST_PointOnSurface, ST_XMin/XMax/YMin/YMax,
//! ST_DistanceSphere, ST_DistanceSpheroid, ST_Azimuth, ST_Project,
//! ST_ClosestPoint, ST_HausdorffDistance

use geo::algorithm::line_measures::metric_spaces::{Euclidean, Geodesic, Haversine};
use geo::algorithm::line_measures::{Bearing, Destination, Distance, Length};
use geo::algorithm::InteriorPoint;
use geo::algorithm::{Area, BoundingRect, Centroid, ClosestPoint, HausdorffDistance};
use geo::Closest;
use geo::{Geometry, Point, Rect};

use crate::core::error::{Result, SqliteGisError};
use crate::core::ewkb::{ensure_matching_srid, parse_ewkb, parse_ewkb_pair, write_ewkb};
use crate::core::functions::emptiness::is_empty_geometry;

fn require_non_empty_geometry(geom: &Geometry<f64>, fn_name: &str) -> Result<()> {
    if is_empty_geometry(geom) {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name} does not accept empty geometries"
        )));
    }
    Ok(())
}

fn require_non_empty_point(point: Point<f64>, fn_name: &str) -> Result<Point<f64>> {
    if point.x().is_nan() || point.y().is_nan() {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name} does not accept empty points"
        )));
    }
    Ok(point)
}

fn latitude_in_range(lat: f64) -> bool {
    (-90.0..=90.0).contains(&lat)
}

/// Reject a point whose latitude is outside `[-90, 90]`, where `geo`'s Karney
/// `Geodesic` returns `NaN` and `Haversine` a wrong finite value. Longitude is
/// periodic and left to the algorithms.
// TODO(georust/geo#1553): drop the Karney half once fixed and released. The
// Haversine paths still need it (wrong finite value, not NaN).
fn require_geographic_latitude(point: Point<f64>, fn_name: &str) -> Result<Point<f64>> {
    if !latitude_in_range(point.y()) {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name}: latitude {} is outside the valid range [-90, 90]",
            point.y()
        )));
    }
    Ok(point)
}

/// Reject a line whose any vertex latitude falls outside `[-90, 90]`, so
/// `Haversine` length is not computed over invalid geographic coordinates.
fn require_geographic_line_latitudes(ls: &geo::LineString<f64>, fn_name: &str) -> Result<()> {
    for coord in ls.coords() {
        if !latitude_in_range(coord.y) {
            return Err(SqliteGisError::InvalidInput(format!(
                "{fn_name}: latitude {} is outside the valid range [-90, 90]",
                coord.y
            )));
        }
    }
    Ok(())
}

/// ST_Area: planar area (square units of the CRS).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_area;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert!((st_area(&poly).unwrap() - 1.0).abs() < 1e-10);
/// ```
pub fn st_area(blob: &[u8]) -> Result<f64> {
    let (geom, _) = parse_ewkb(blob)?;
    Ok(geom.unsigned_area())
}

/// ST_Length / ST_Length2D: planar arc length of a LineString or MultiLineString.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_length;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0,3 4)", None).unwrap();
/// assert!((st_length(&line).unwrap() - 5.0).abs() < 1e-10);
/// ```
pub fn st_length(blob: &[u8]) -> Result<f64> {
    let (geom, _) = parse_ewkb(blob)?;
    let len = match &geom {
        Geometry::LineString(ls) => Euclidean.length(ls),
        Geometry::MultiLineString(mls) => mls.0.iter().map(|ls| Euclidean.length(ls)).sum(),
        other => {
            return Err(SqliteGisError::wrong_type(
                "LineString or MultiLineString",
                other,
            ))
        }
    };
    Ok(len)
}

/// ST_Perimeter: planar perimeter of a Polygon or MultiPolygon.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_perimeter;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert!((st_perimeter(&poly).unwrap() - 4.0).abs() < 1e-10);
/// ```
pub fn st_perimeter(blob: &[u8]) -> Result<f64> {
    fn poly_perimeter(p: &geo::Polygon<f64>) -> f64 {
        Euclidean.length(p.exterior())
            + p.interiors()
                .iter()
                .map(|r| Euclidean.length(r))
                .sum::<f64>()
    }
    let (geom, _) = parse_ewkb(blob)?;
    let perim = match &geom {
        Geometry::Polygon(p) => poly_perimeter(p),
        Geometry::MultiPolygon(mp) => mp.0.iter().map(poly_perimeter).sum(),
        other => return Err(SqliteGisError::wrong_type("Polygon or MultiPolygon", other)),
    };
    Ok(perim)
}

/// Dispatch euclidean distance between any two geo geometry types.
fn euclidean_geometry_distance(a: &Geometry<f64>, b: &Geometry<f64>) -> f64 {
    Euclidean.distance(a, b)
}

/// ST_Distance: minimum Euclidean distance between two geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_distance;
/// use sqlitegis::core::functions::constructors::st_point;
///
/// let a = st_point(0.0, 0.0, None).unwrap();
/// let b = st_point(3.0, 4.0, None).unwrap();
/// assert!((st_distance(&a, &b).unwrap() - 5.0).abs() < 1e-10);
/// ```
pub fn st_distance(a: &[u8], b: &[u8]) -> Result<f64> {
    crate::core::functions::catch_geo("ST_Distance", || {
        let (ga, gb, _) = parse_ewkb_pair(a, b)?;
        require_non_empty_geometry(&ga, "ST_Distance")?;
        require_non_empty_geometry(&gb, "ST_Distance")?;
        Ok(euclidean_geometry_distance(&ga, &gb))
    })
}

/// ST_Centroid: geometric centroid of any geometry.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_centroid;
/// use sqlitegis::core::functions::accessors::{st_x, st_y};
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let c = st_centroid(&poly).unwrap();
/// assert!((st_x(&c).unwrap().unwrap() - 1.0).abs() < 1e-10);
/// assert!((st_y(&c).unwrap().unwrap() - 1.0).abs() < 1e-10);
/// ```
pub fn st_centroid(blob: &[u8]) -> Result<Vec<u8>> {
    crate::core::functions::catch_geo("ST_Centroid", || {
        let (geom, srid) = parse_ewkb(blob)?;
        let c = geom
            .centroid()
            .ok_or_else(|| SqliteGisError::wrong_type("non-empty geometry", &geom))?;
        write_ewkb(&Geometry::Point(c), srid)
    })
}

/// ST_PointOnSurface: a point guaranteed to lie on the geometry.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_point_on_surface;
/// use sqlitegis::core::functions::predicates::st_within;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
/// let pt = st_point_on_surface(&poly).unwrap();
/// // The point on surface should be within the polygon
/// assert!(st_within(&pt, &poly).unwrap());
/// ```
pub fn st_point_on_surface(blob: &[u8]) -> Result<Vec<u8>> {
    crate::core::functions::catch_geo("ST_PointOnSurface", || {
        let (geom, srid) = parse_ewkb(blob)?;
        let p = geom
            .interior_point()
            .ok_or_else(|| SqliteGisError::wrong_type("non-empty geometry", &geom))?;
        write_ewkb(&Geometry::Point(p), srid)
    })
}

/// ST_HausdorffDistance: Hausdorff distance between two geometries.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_hausdorff_distance;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let a = geom_from_text("LINESTRING(0 0,1 0)", None).unwrap();
/// let b = geom_from_text("LINESTRING(0 1,1 1)", None).unwrap();
/// assert!((st_hausdorff_distance(&a, &b).unwrap() - 1.0).abs() < 1e-10);
/// ```
pub fn st_hausdorff_distance(a: &[u8], b: &[u8]) -> Result<f64> {
    crate::core::functions::catch_geo("ST_HausdorffDistance", || {
        let (ga, gb, _) = parse_ewkb_pair(a, b)?;
        require_non_empty_geometry(&ga, "ST_HausdorffDistance")?;
        require_non_empty_geometry(&gb, "ST_HausdorffDistance")?;
        Ok(ga.hausdorff_distance(&gb))
    })
}

// Bounding-box accessors

/// ST_XMin: minimum X of the bounding rectangle.
///
/// Returns `None` for empty geometries (PostGIS-compatible).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_xmin;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(1 2,3 4)", None).unwrap();
/// assert!((st_xmin(&blob).unwrap().unwrap() - 1.0).abs() < 1e-10);
/// ```
pub fn st_xmin(blob: &[u8]) -> Result<Option<f64>> {
    let r = bbox(blob)?;
    Ok(r.map(|r| r.min().x))
}

/// ST_XMax: maximum X of the bounding rectangle.
///
/// Returns `None` for empty geometries (PostGIS-compatible).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_xmax;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(1 2,3 4)", None).unwrap();
/// assert!((st_xmax(&blob).unwrap().unwrap() - 3.0).abs() < 1e-10);
/// ```
pub fn st_xmax(blob: &[u8]) -> Result<Option<f64>> {
    let r = bbox(blob)?;
    Ok(r.map(|r| r.max().x))
}

/// ST_YMin: minimum Y of the bounding rectangle.
///
/// Returns `None` for empty geometries (PostGIS-compatible).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_ymin;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(1 2,3 4)", None).unwrap();
/// assert!((st_ymin(&blob).unwrap().unwrap() - 2.0).abs() < 1e-10);
/// ```
pub fn st_ymin(blob: &[u8]) -> Result<Option<f64>> {
    let r = bbox(blob)?;
    Ok(r.map(|r| r.min().y))
}

/// ST_YMax: maximum Y of the bounding rectangle.
///
/// Returns `None` for empty geometries (PostGIS-compatible).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_ymax;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(1 2,3 4)", None).unwrap();
/// assert!((st_ymax(&blob).unwrap().unwrap() - 4.0).abs() < 1e-10);
/// ```
pub fn st_ymax(blob: &[u8]) -> Result<Option<f64>> {
    let r = bbox(blob)?;
    Ok(r.map(|r| r.max().y))
}

fn bbox(blob: &[u8]) -> Result<Option<Rect<f64>>> {
    let (geom, _) = parse_ewkb(blob)?;
    if is_empty_geometry(&geom) {
        return Ok(None);
    }
    geom.bounding_rect()
        .ok_or_else(|| SqliteGisError::wrong_type("non-empty geometry", &geom))
        .map(Some)
}

// Spherical / geodetic variants

fn require_point(g: Geometry<f64>) -> Result<Point<f64>> {
    match g {
        Geometry::Point(p) => Ok(p),
        other => Err(SqliteGisError::wrong_type("Point", &other)),
    }
}

fn ensure_geographic_srid(srid: Option<i32>, fn_name: &str) -> Result<()> {
    match srid {
        Some(4326) => Ok(()),
        Some(srid) => Err(SqliteGisError::InvalidInput(format!(
            "{fn_name} requires SRID 4326 (got {srid})"
        ))),
        None => Err(SqliteGisError::InvalidInput(format!(
            "{fn_name} requires SRID 4326 (got unknown/unspecified SRID)"
        ))),
    }
}

fn ensure_matching_geographic_srid(
    left: Option<i32>,
    right: Option<i32>,
    fn_name: &str,
) -> Result<Option<i32>> {
    let srid = ensure_matching_srid(left, right)?;
    ensure_geographic_srid(srid, fn_name)?;
    Ok(srid)
}

fn parse_two_geographic_points(
    a: &[u8],
    b: &[u8],
    fn_name: &str,
) -> Result<(Point<f64>, Point<f64>, Option<i32>)> {
    let (ga, srid_a) = parse_ewkb(a)?;
    let (gb, srid_b) = parse_ewkb(b)?;
    let srid = ensure_matching_geographic_srid(srid_a, srid_b, fn_name)?;
    let pa = require_geographic_latitude(
        require_non_empty_point(require_point(ga)?, fn_name)?,
        fn_name,
    )?;
    let pb = require_geographic_latitude(
        require_non_empty_point(require_point(gb)?, fn_name)?,
        fn_name,
    )?;
    Ok((pa, pb, srid))
}

/// ST_DistanceSphere: Haversine distance in metres (requires Point inputs, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_distance_sphere;
/// use sqlitegis::core::functions::constructors::st_point;
///
/// let london = st_point(-0.1278, 51.5074, Some(4326)).unwrap();
/// let paris = st_point(2.3522, 48.8566, Some(4326)).unwrap();
/// let dist = st_distance_sphere(&london, &paris).unwrap();
/// assert!(dist > 300_000.0 && dist < 400_000.0); // ~340 km
/// ```
pub fn st_distance_sphere(a: &[u8], b: &[u8]) -> Result<f64> {
    let (pa, pb, _) = parse_two_geographic_points(a, b, "ST_DistanceSphere")?;
    Ok(Haversine.distance(pa, pb))
}

/// ST_DistanceSpheroid: Geodesic distance in metres (Karney algorithm, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_distance_spheroid;
/// use sqlitegis::core::functions::constructors::st_point;
///
/// let london = st_point(-0.1278, 51.5074, Some(4326)).unwrap();
/// let paris = st_point(2.3522, 48.8566, Some(4326)).unwrap();
/// let dist = st_distance_spheroid(&london, &paris).unwrap();
/// assert!(dist > 300_000.0 && dist < 400_000.0); // ~340 km
/// ```
pub fn st_distance_spheroid(a: &[u8], b: &[u8]) -> Result<f64> {
    let (pa, pb, _) = parse_two_geographic_points(a, b, "ST_DistanceSpheroid")?;
    Ok(Geodesic.distance(pa, pb))
}

/// ST_LengthSphere: Haversine arc length of a line in metres (SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_length_sphere;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(-0.1278 51.5074, 2.3522 48.8566)", Some(4326)).unwrap();
/// let len = st_length_sphere(&line).unwrap();
/// assert!(len > 300_000.0); // > 300 km
/// ```
pub fn st_length_sphere(blob: &[u8]) -> Result<f64> {
    let (geom, srid) = parse_ewkb(blob)?;
    ensure_geographic_srid(srid, "ST_LengthSphere")?;
    match &geom {
        Geometry::LineString(ls) => {
            require_geographic_line_latitudes(ls, "ST_LengthSphere")?;
            Ok(Haversine.length(ls))
        }
        Geometry::MultiLineString(mls) => {
            for ls in &mls.0 {
                require_geographic_line_latitudes(ls, "ST_LengthSphere")?;
            }
            Ok(mls.0.iter().map(|ls| Haversine.length(ls)).sum())
        }
        other => Err(SqliteGisError::wrong_type(
            "LineString or MultiLineString",
            other,
        )),
    }
}

/// ST_Azimuth: bearing from origin to target in radians (0 = north, clockwise, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_azimuth;
/// use sqlitegis::core::functions::constructors::st_point;
///
/// let origin = st_point(0.0, 0.0, Some(4326)).unwrap();
/// let target = st_point(0.0, 1.0, Some(4326)).unwrap();
/// let az = st_azimuth(&origin, &target).unwrap();
/// // Due north -> azimuth approximately  0
/// assert!(az.abs() < 0.01);
/// ```
pub fn st_azimuth(origin: &[u8], target: &[u8]) -> Result<f64> {
    let (po, pt, _) = parse_two_geographic_points(origin, target, "ST_Azimuth")?;
    Ok(Geodesic.bearing(po, pt).to_radians())
}

/// ST_Project: destination point given a start, bearing (radians), and distance (metres, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_project;
/// use sqlitegis::core::functions::constructors::st_point;
/// use sqlitegis::core::functions::accessors::st_y;
///
/// let origin = st_point(0.0, 0.0, Some(4326)).unwrap();
/// // Project 111_000m due north (azimuth=0)
/// let dest = st_project(&origin, 111_000.0, 0.0).unwrap();
/// // Should be roughly 1 degree north
/// assert!((st_y(&dest).unwrap().unwrap() - 1.0).abs() < 0.1);
/// ```
pub fn st_project(origin: &[u8], distance: f64, azimuth: f64) -> Result<Vec<u8>> {
    if !distance.is_finite() {
        return Err(SqliteGisError::InvalidInput(
            "ST_Project: distance must be finite".to_string(),
        ));
    }
    if !azimuth.is_finite() {
        return Err(SqliteGisError::InvalidInput(
            "ST_Project: azimuth must be finite".to_string(),
        ));
    }
    let (go, srid) = parse_ewkb(origin)?;
    ensure_geographic_srid(srid, "ST_Project")?;
    let po = require_geographic_latitude(
        require_non_empty_point(require_point(go)?, "ST_Project")?,
        "ST_Project",
    )?;
    let dest: Point<f64> = Geodesic.destination(po, azimuth.to_degrees(), distance);
    write_ewkb(&Geometry::Point(dest), srid)
}

/// ST_ClosestPoint: the point on geometry A closest to geometry B (point).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::measurement::st_closest_point;
/// use sqlitegis::core::functions::constructors::st_point;
/// use sqlitegis::core::functions::accessors::{st_x, st_y};
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0,10 0)", None).unwrap();
/// let pt = st_point(5.0, 5.0, None).unwrap();
/// let cp = st_closest_point(&line, &pt).unwrap();
/// assert!((st_x(&cp).unwrap().unwrap() - 5.0).abs() < 1e-10);
/// assert!((st_y(&cp).unwrap().unwrap() - 0.0).abs() < 1e-10);
/// ```
pub fn st_closest_point(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    crate::core::functions::catch_geo("ST_ClosestPoint", || {
        let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
        require_non_empty_geometry(&ga, "ST_ClosestPoint")?;
        let pb = require_non_empty_point(require_point(gb)?, "ST_ClosestPoint")?;
        let cp = ga.closest_point(&pb);
        let pt = match cp {
            Closest::Intersection(p) | Closest::SinglePoint(p) => p,
            Closest::Indeterminate => {
                return Err(SqliteGisError::InvalidInput(
                    "ST_ClosestPoint: unable to determine closest point".to_string(),
                ))
            }
        };
        write_ewkb(&Geometry::Point(pt), srid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::functions::accessors::{st_x, st_y};
    use crate::core::functions::constructors::st_point;
    use crate::core::functions::io::geom_from_text;

    /// EWKB for a polygon whose ring has a NaN vertex, so `is_closed()` (a
    /// `first == last` compare) is false and geo's centroid `assert!` aborts.
    /// Fuzzer-found.
    fn nan_ring_polygon() -> Vec<u8> {
        use crate::core::ewkb::WKB_POLYGON;
        let mut blob = vec![0x01u8];
        blob.extend_from_slice(&WKB_POLYGON.to_le_bytes());
        blob.extend_from_slice(&1u32.to_le_bytes()); // one ring
        blob.extend_from_slice(&1u32.to_le_bytes()); // a single NaN point
        blob.extend_from_slice(&f64::NAN.to_le_bytes());
        blob.extend_from_slice(&f64::NAN.to_le_bytes());
        blob
    }

    #[test]
    fn st_centroid_degenerate_ring_does_not_panic() {
        // Must come back as a recoverable error, not abort the process.
        let blob = nan_ring_polygon();
        assert!(st_centroid(&blob).is_err());
    }

    #[test]
    fn st_point_on_surface_degenerate_ring_does_not_panic() {
        let blob = nan_ring_polygon();
        assert!(st_point_on_surface(&blob).is_err());
    }

    // -- Wrong-type errors ------------------------------------------

    #[test]
    fn st_length_wrong_type() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_length(&pt).is_err());
    }

    #[test]
    fn st_perimeter_wrong_type() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_perimeter(&line).is_err());
    }

    #[test]
    fn st_distance_sphere_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_distance_sphere(&line, &pt).is_err());
    }

    #[test]
    fn st_distance_spheroid_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_distance_spheroid(&line, &pt).is_err());
    }

    #[test]
    fn geodesic_out_of_range_latitude_errors_not_nan() {
        // Latitude past the pole drives geo's Karney geodesic to NaN; the
        // geographic functions must reject it with an error instead. The
        // fuzzer found this (a=(0,-90.00000004)).
        let valid = st_point(2.3522, 48.8566, Some(4326)).unwrap();
        for bad in [
            st_point(0.0, 95.0, Some(4326)).unwrap(),
            st_point(0.0, -90.0001, Some(4326)).unwrap(),
        ] {
            assert!(st_distance_spheroid(&bad, &valid).is_err());
            assert!(st_distance_sphere(&bad, &valid).is_err());
            assert!(st_azimuth(&bad, &valid).is_err());
            assert!(st_project(&bad, 1000.0, 0.5).is_err());
        }
    }

    #[test]
    fn st_length_sphere_out_of_range_latitude_errors() {
        // Haversine silently returns a wrong finite length past the poles, so
        // ST_LengthSphere must reject a line with an out-of-range vertex.
        let bad = geom_from_text("LINESTRING(0 0, 10 95)", Some(4326)).unwrap();
        assert!(st_length_sphere(&bad).is_err());
        let bad_multi =
            geom_from_text("MULTILINESTRING((0 0, 1 1),(2 2, 3 -91))", Some(4326)).unwrap();
        assert!(st_length_sphere(&bad_multi).is_err());
        // A valid line still works.
        let ok = geom_from_text("LINESTRING(0 0, 10 10)", Some(4326)).unwrap();
        assert!(st_length_sphere(&ok).unwrap() > 0.0);
    }

    #[test]
    fn st_azimuth_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_azimuth(&line, &pt).is_err());
    }

    #[test]
    fn st_project_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_project(&line, 100.0, 0.0).is_err());
    }

    #[test]
    fn st_length_sphere_non_linestring() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_length_sphere(&pt).is_err());
    }

    // -- Distance to self -------------------------------------------

    #[test]
    fn distance_to_self_is_zero() {
        let pt = st_point(1.0, 2.0, None).unwrap();
        assert!(st_distance(&pt, &pt).unwrap().abs() < 1e-10);
    }

    #[test]
    fn st_distance_multipoint_point_uses_min_distance() {
        let mp = geom_from_text("MULTIPOINT((0 0),(10 0))", None).unwrap();
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        let d = st_distance(&mp, &pt).unwrap();
        assert!((d - 0.0).abs() < 1e-10, "distance = {d}");
    }

    #[test]
    fn st_distance_mixed_srid_errors() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(3.0, 4.0, Some(3857)).unwrap();
        assert!(st_distance(&a, &b).is_err());
    }

    #[test]
    fn st_azimuth_mixed_srid_errors() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(0.0, 1.0, Some(3857)).unwrap();
        assert!(st_azimuth(&a, &b).is_err());
    }

    #[test]
    fn st_distance_sphere_non_geographic_srid_errors() {
        let a = st_point(0.0, 0.0, Some(3857)).unwrap();
        let b = st_point(1.0, 1.0, Some(3857)).unwrap();
        assert!(st_distance_sphere(&a, &b).is_err());
    }

    #[test]
    fn st_distance_sphere_missing_srid_errors() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(1.0, 1.0, None).unwrap();
        assert!(st_distance_sphere(&a, &b).is_err());
    }

    #[test]
    fn st_distance_spheroid_non_geographic_srid_errors() {
        let a = st_point(0.0, 0.0, Some(3857)).unwrap();
        let b = st_point(1.0, 1.0, Some(3857)).unwrap();
        assert!(st_distance_spheroid(&a, &b).is_err());
    }

    #[test]
    fn st_distance_spheroid_missing_srid_errors() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(1.0, 1.0, None).unwrap();
        assert!(st_distance_spheroid(&a, &b).is_err());
    }

    #[test]
    fn st_length_sphere_non_geographic_srid_errors() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", Some(3857)).unwrap();
        assert!(st_length_sphere(&line).is_err());
    }

    #[test]
    fn st_length_sphere_missing_srid_errors() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_length_sphere(&line).is_err());
    }

    #[test]
    fn st_project_non_geographic_srid_errors() {
        let origin = st_point(0.0, 0.0, Some(3857)).unwrap();
        assert!(st_project(&origin, 111_000.0, 0.0).is_err());
    }

    #[test]
    fn st_project_missing_srid_errors() {
        let origin = st_point(0.0, 0.0, None).unwrap();
        assert!(st_project(&origin, 111_000.0, 0.0).is_err());
    }

    #[test]
    fn st_project_non_finite_args_error() {
        let origin = st_point(0.0, 0.0, Some(4326)).unwrap();

        let err = st_project(&origin, f64::INFINITY, 0.0)
            .expect_err("non-finite distance should be rejected");
        assert!(format!("{err}").contains("distance must be finite"));

        let err = st_project(&origin, 1_000.0, f64::NEG_INFINITY).expect_err("non-finite azimuth");
        assert!(format!("{err}").contains("azimuth must be finite"));
    }

    #[test]
    fn st_azimuth_missing_srid_errors() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(0.0, 1.0, None).unwrap();
        assert!(st_azimuth(&a, &b).is_err());
    }

    #[test]
    fn distance_sphere_to_self_is_zero() {
        let pt = st_point(1.0, 2.0, Some(4326)).unwrap();
        assert!(st_distance_sphere(&pt, &pt).unwrap().abs() < 1e-10);
    }

    #[test]
    fn distance_spheroid_to_self_is_zero() {
        let pt = st_point(1.0, 2.0, Some(4326)).unwrap();
        assert!(st_distance_spheroid(&pt, &pt).unwrap().abs() < 1e-10);
    }

    // -- Hausdorff --------------------------------------------------

    #[test]
    fn hausdorff_identical_is_zero() {
        let line = geom_from_text("LINESTRING(0 0,1 1,2 0)", None).unwrap();
        assert!(st_hausdorff_distance(&line, &line).unwrap().abs() < 1e-10);
    }

    #[test]
    fn st_hausdorff_distance_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        let err = st_hausdorff_distance(&empty, &pt).expect_err("empty point must error");
        assert!(format!("{err}").contains("does not accept empty geometries"));
    }

    // -- Closest point ----------------------------------------------

    #[test]
    fn closest_point_perpendicular_projection() {
        let line = geom_from_text("LINESTRING(0 0,10 0)", None).unwrap();
        let pt = st_point(5.0, 3.0, None).unwrap();
        let cp = st_closest_point(&line, &pt).unwrap();
        assert!((st_x(&cp).unwrap().unwrap() - 5.0).abs() < 1e-10);
        assert!((st_y(&cp).unwrap().unwrap() - 0.0).abs() < 1e-10);
    }

    // -- Bounding box -----------------------------------------------

    #[test]
    fn bbox_invariants() {
        let poly = geom_from_text("POLYGON((1 2,5 2,5 8,1 8,1 2))", None).unwrap();
        let xmin = st_xmin(&poly).unwrap().unwrap();
        let xmax = st_xmax(&poly).unwrap().unwrap();
        let ymin = st_ymin(&poly).unwrap().unwrap();
        let ymax = st_ymax(&poly).unwrap().unwrap();
        assert!(xmin <= xmax);
        assert!(ymin <= ymax);
        assert!((xmin - 1.0).abs() < 1e-10);
        assert!((xmax - 5.0).abs() < 1e-10);
        assert!((ymin - 2.0).abs() < 1e-10);
        assert!((ymax - 8.0).abs() < 1e-10);
    }

    #[test]
    fn bbox_accessors_return_none_for_empty_geometries() {
        let empty_point = geom_from_text("POINT EMPTY", None).unwrap();
        assert_eq!(st_xmin(&empty_point).unwrap(), None);
        assert_eq!(st_xmax(&empty_point).unwrap(), None);
        assert_eq!(st_ymin(&empty_point).unwrap(), None);
        assert_eq!(st_ymax(&empty_point).unwrap(), None);

        let empty_gc = geom_from_text("GEOMETRYCOLLECTION EMPTY", None).unwrap();
        assert_eq!(st_xmin(&empty_gc).unwrap(), None);
        assert_eq!(st_xmax(&empty_gc).unwrap(), None);
        assert_eq!(st_ymin(&empty_gc).unwrap(), None);
        assert_eq!(st_ymax(&empty_gc).unwrap(), None);
    }

    #[test]
    fn st_area_point_is_zero() {
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_area(&pt).unwrap().abs() < 1e-10);
    }

    #[test]
    fn st_length_multilinestring() {
        let mls = geom_from_text("MULTILINESTRING((0 0,1 0),(0 0,0 1))", None).unwrap();
        assert!((st_length(&mls).unwrap() - 2.0).abs() < 1e-10);
    }

    #[test]
    fn st_length_linestring() {
        let line = geom_from_text("LINESTRING(0 0,3 4)", None).unwrap();
        assert!((st_length(&line).unwrap() - 5.0).abs() < 1e-10);
    }

    #[test]
    fn st_perimeter_polygon_and_multipolygon() {
        let poly =
            geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))", None).unwrap();
        assert!((st_perimeter(&poly).unwrap() - 20.0).abs() < 1e-10);

        let mpoly = geom_from_text(
            "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))",
            None,
        )
        .unwrap();
        assert!((st_perimeter(&mpoly).unwrap() - 8.0).abs() < 1e-10);
    }

    #[test]
    fn centroid_and_point_on_surface_are_computable() {
        let poly = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", Some(4326)).unwrap();

        let centroid = st_centroid(&poly).unwrap();
        assert!((st_x(&centroid).unwrap().unwrap() - 1.0).abs() < 1e-10);
        assert!((st_y(&centroid).unwrap().unwrap() - 1.0).abs() < 1e-10);

        let pos = st_point_on_surface(&poly).unwrap();
        assert!(st_x(&pos).unwrap().unwrap() >= 0.0);
        assert!(st_y(&pos).unwrap().unwrap() >= 0.0);
    }

    #[test]
    fn st_length_sphere_linestring_and_multilinestring() {
        let line =
            geom_from_text("LINESTRING(-0.1278 51.5074,2.3522 48.8566)", Some(4326)).unwrap();
        assert!(st_length_sphere(&line).unwrap() > 300_000.0);

        let mls = geom_from_text(
            "MULTILINESTRING((-0.1278 51.5074,2.3522 48.8566),(2.3522 48.8566,13.4050 52.5200))",
            Some(4326),
        )
        .unwrap();
        assert!(st_length_sphere(&mls).unwrap() > st_length_sphere(&line).unwrap());
    }

    #[test]
    fn st_azimuth_and_project_success() {
        let origin = st_point(0.0, 0.0, Some(4326)).unwrap();
        let north = st_point(0.0, 1.0, Some(4326)).unwrap();
        let azimuth = st_azimuth(&origin, &north).unwrap();
        assert!(azimuth.abs() < 0.01);

        let dest = st_project(&origin, 111_000.0, 0.0).unwrap();
        assert!((st_y(&dest).unwrap().unwrap() - 1.0).abs() < 0.2);
        assert_eq!(
            crate::core::functions::accessors::st_srid(&dest).unwrap(),
            4326
        );
    }

    #[test]
    fn st_closest_point_indeterminate_for_empty_geometry() {
        let empty = geom_from_text("GEOMETRYCOLLECTION EMPTY", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        assert!(st_closest_point(&empty, &pt).is_err());
    }

    #[test]
    fn st_distance_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        let pt = st_point(0.0, 0.0, None).unwrap();
        let err = st_distance(&empty, &pt).expect_err("empty point must error");
        assert!(format!("{err}").contains("does not accept empty geometries"));
    }

    #[test]
    fn st_distance_sphere_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        let pt = st_point(0.0, 0.0, Some(4326)).unwrap();
        assert!(st_distance_sphere(&empty, &pt).is_err());
    }

    #[test]
    fn st_distance_spheroid_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        let pt = st_point(0.0, 0.0, Some(4326)).unwrap();
        assert!(st_distance_spheroid(&empty, &pt).is_err());
    }

    #[test]
    fn st_azimuth_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        let pt = st_point(0.0, 0.0, Some(4326)).unwrap();
        assert!(st_azimuth(&empty, &pt).is_err());
    }

    #[test]
    fn st_project_empty_point_errors() {
        let empty = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        assert!(st_project(&empty, 1_000.0, 0.0).is_err());
    }

    #[test]
    fn st_closest_point_empty_target_point_errors() {
        let line = geom_from_text("LINESTRING(0 0,10 0)", None).unwrap();
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        assert!(st_closest_point(&line, &empty).is_err());
    }

    #[test]
    fn st_length_sphere_rejects_polygon() {
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", Some(4326)).unwrap();
        let err = st_length_sphere(&poly).expect_err("polygon input must error");
        assert!(format!("{err}").contains("LineString or MultiLineString"));
    }

    #[test]
    fn st_distance_sphere_second_point_bad_latitude() {
        // Exercises parse_two_geographic_points line 362-365: require_geographic_latitude
        // on the second point (pb path).
        let valid = st_point(0.0, 0.0, Some(4326)).unwrap();
        let bad = st_point(0.0, 100.0, Some(4326)).unwrap();
        let err = st_distance_sphere(&valid, &bad).expect_err("bad latitude must error");
        assert!(
            format!("{err}").contains("latitude"),
            "expected latitude error, got: {err}"
        );
    }

    #[test]
    fn st_closest_point_nan_geometry_errors_at_parse() {
        // NaN coordinates are rejected by reject_non_finite_coords in parse_ewkb,
        // so they never reach the closest_point algorithm. The Indeterminate
        // path (lines 518-520) is defensive code for geometries that geo's
        // algorithm cannot resolve, but no known finite non-empty geometry
        // triggers it in practice.
        let nan_poly = nan_ring_polygon();
        let pt = st_point(0.0, 0.0, None).unwrap();
        let err = st_closest_point(&nan_poly, &pt).expect_err("NaN geometry must error");
        assert!(
            format!("{err}").contains("non-finite"),
            "expected non-finite error, got: {err}"
        );
    }
}
