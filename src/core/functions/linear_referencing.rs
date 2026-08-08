//! Linear referencing: pick a point or a slice out of a line by fraction.
//!
//! ST_LineInterpolatePoint, ST_LineInterpolatePoints, ST_LineSubstring, each
//! in a planar, spherical and ellipsoidal form.

use geo::algorithm::line_measures::metric_spaces::{Euclidean, Geodesic, Haversine};
use geo::algorithm::line_measures::{InterpolateLine, InterpolatePoint, Length};
use geo::{Geometry, LineString, MultiPoint, Point};

use crate::core::error::{Result, SqliteGisError};
use crate::core::ewkb::{parse_ewkb, write_ewkb};

/// Every metric space we interpolate along. `InterpolateLine` is blanket
/// implemented for anything that can interpolate between two points and
/// measure a line, which all three of these can.
trait LineMetric: InterpolateLine<f64> + InterpolatePoint<f64> + Length<f64> {}
impl<T: InterpolateLine<f64> + InterpolatePoint<f64> + Length<f64>> LineMetric for T {}

/// Reject a fraction that is not a real number in `[0, 1]`, the way PostGIS
/// does with "2nd arg isn't within [0,1]".
fn require_fraction(value: f64, fn_name: &str, arg: &str) -> Result<f64> {
    if !(0.0..=1.0).contains(&value) {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name}: {arg} must be within [0, 1], got {value}"
        )));
    }
    Ok(value)
}

/// A step of zero would emit points forever, so it is rejected on top of the
/// usual `[0, 1]` bound.
fn require_step(value: f64, fn_name: &str) -> Result<f64> {
    let value = require_fraction(value, fn_name, "fraction")?;
    if value == 0.0 {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name}: fraction must be greater than 0"
        )));
    }
    Ok(value)
}

/// Both ends in `[0, 1]` and in order, matching PostGIS's "2nd arg must be
/// smaller then 3rd arg".
fn require_window(start: f64, end: f64, fn_name: &str) -> Result<(f64, f64)> {
    let start = require_fraction(start, fn_name, "start fraction")?;
    let end = require_fraction(end, fn_name, "end fraction")?;
    if start > end {
        return Err(SqliteGisError::InvalidInput(format!(
            "{fn_name}: start fraction {start} must not exceed end fraction {end}"
        )));
    }
    Ok((start, end))
}

/// Parse a line for a planar linear-referencing call. PostGIS accepts a
/// single LineString here and nothing else.
fn parse_line(blob: &[u8]) -> Result<(LineString<f64>, Option<i32>)> {
    let (geom, srid) = parse_ewkb(blob)?;
    match geom {
        Geometry::LineString(ls) => Ok((ls, srid)),
        other => Err(SqliteGisError::wrong_type("LineString", &other)),
    }
}

/// Same, for the curved forms: SRID 4326 and every vertex latitude in range,
/// matching the rest of the curved-earth surface.
fn parse_geographic_line(blob: &[u8], fn_name: &str) -> Result<(LineString<f64>, Option<i32>)> {
    use crate::core::functions::measurement::{
        ensure_geographic_srid, require_geographic_line_latitudes,
    };
    let (ls, srid) = parse_line(blob)?;
    ensure_geographic_srid(srid, fn_name)?;
    require_geographic_line_latitudes(&ls, fn_name)?;
    Ok((ls, srid))
}

/// A LineString with fewer than two vertices has no length to walk, so
/// PostGIS answers POINT EMPTY. `geo` would return `None` for the same case.
fn empty_point(srid: Option<i32>) -> Result<Vec<u8>> {
    write_ewkb(&Geometry::Point(Point::new(f64::NAN, f64::NAN)), srid)
}

fn interpolate(
    metric: &impl LineMetric,
    line: &LineString<f64>,
    fraction: f64,
    srid: Option<i32>,
) -> Result<Vec<u8>> {
    match metric.point_at_ratio_from_start(line, fraction) {
        Some(p) => write_ewkb(&Geometry::Point(p), srid),
        None => empty_point(srid),
    }
}

fn interpolate_many(
    metric: &impl LineMetric,
    line: &LineString<f64>,
    fraction: f64,
    srid: Option<i32>,
) -> Result<Vec<u8>> {
    let mut points = Vec::new();
    let mut ratio = fraction;
    // PostGIS walks fraction, 2*fraction, ... and includes the far end when
    // the step divides the line evenly.
    while ratio < 1.0 || (ratio - 1.0).abs() < 1e-12 {
        // `None` means the line is too short to walk, so stop with whatever
        // has been collected, which for an empty line is nothing.
        let Some(p) = metric.point_at_ratio_from_start(line, ratio.min(1.0)) else {
            break;
        };
        points.push(p);
        if (ratio - 1.0).abs() < 1e-12 {
            break;
        }
        ratio += fraction;
    }
    write_ewkb(&Geometry::MultiPoint(MultiPoint::new(points)), srid)
}

fn substring(
    metric: &impl LineMetric,
    line: &LineString<f64>,
    start: f64,
    end: f64,
    srid: Option<i32>,
) -> Result<Vec<u8>> {
    let total = metric.length(line);
    let (Some(head), Some(tail)) = (
        metric.point_at_ratio_from_start(line, start),
        metric.point_at_ratio_from_start(line, end),
    ) else {
        return empty_point(srid);
    };
    if start == end {
        // PostGIS collapses a zero-length substring to a Point.
        return write_ewkb(&Geometry::Point(head), srid);
    }

    // Keep the original vertices that fall strictly inside the window, so a
    // substring of the whole line reproduces the line.
    let (from, to) = (start * total, end * total);
    let mut walked = 0.0;
    let mut coords = vec![head.into()];
    for segment in line.lines() {
        let before = walked;
        walked += metric.length(&LineString::new(vec![segment.start, segment.end]));
        if before > from && before < to {
            coords.push(segment.start);
        }
    }
    coords.push(tail.into());
    write_ewkb(&Geometry::LineString(LineString::new(coords)), srid)
}

/// ST_LineInterpolatePoint: the point a given fraction along a line, measured
/// in the units of the CRS.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_point;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", None).unwrap();
/// let mid = st_line_interpolate_point(&line, 0.5).unwrap();
/// assert_eq!(as_text(&mid).unwrap(), "POINT(45 30)");
/// ```
pub fn st_line_interpolate_point(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_fraction(fraction, "ST_LineInterpolatePoint", "fraction")?;
    let (line, srid) = parse_line(blob)?;
    interpolate(&Euclidean, &line, fraction, srid)
}

/// ST_LineInterpolatePointSphere: the point a given fraction along a line
/// measured on a sphere of the mean earth radius (SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_point_sphere;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// let mid = st_line_interpolate_point_sphere(&line, 0.5).unwrap();
/// // PostGIS answers POINT(26.565051177078 37.761243907035) for this line.
/// assert!(as_text(&mid).unwrap().starts_with("POINT(26.5650511"));
/// ```
pub fn st_line_interpolate_point_sphere(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_fraction(fraction, "ST_LineInterpolatePointSphere", "fraction")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineInterpolatePointSphere")?;
    interpolate(&Haversine, &line, fraction, srid)
}

/// ST_LineInterpolatePointSpheroid: the point a given fraction along a line
/// measured on the WGS84 ellipsoid (Karney, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_point_spheroid;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// let mid = st_line_interpolate_point_spheroid(&line, 0.5).unwrap();
/// // PostGIS answers POINT(26.606325079977 37.874403088301) for this line.
/// assert!(as_text(&mid).unwrap().starts_with("POINT(26.6063250"));
/// ```
pub fn st_line_interpolate_point_spheroid(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_fraction(fraction, "ST_LineInterpolatePointSpheroid", "fraction")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineInterpolatePointSpheroid")?;
    interpolate(&Geodesic, &line, fraction, srid)
}

/// ST_LineInterpolatePoints: points at every multiple of `fraction` along a
/// line, as a MultiPoint.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_points;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 4 0)", None).unwrap();
/// let pts = st_line_interpolate_points(&line, 0.5).unwrap();
/// assert_eq!(as_text(&pts).unwrap(), "MULTIPOINT(2 0,4 0)");
/// ```
pub fn st_line_interpolate_points(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_step(fraction, "ST_LineInterpolatePoints")?;
    let (line, srid) = parse_line(blob)?;
    interpolate_many(&Euclidean, &line, fraction, srid)
}

/// ST_LineInterpolatePointsSphere: [`st_line_interpolate_points`] measured on
/// a sphere of the mean earth radius (SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_points_sphere;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_line_interpolate_points_sphere(&line, 0.25).is_ok());
/// ```
pub fn st_line_interpolate_points_sphere(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_step(fraction, "ST_LineInterpolatePointsSphere")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineInterpolatePointsSphere")?;
    interpolate_many(&Haversine, &line, fraction, srid)
}

/// ST_LineInterpolatePointsSpheroid: [`st_line_interpolate_points`] measured
/// on the WGS84 ellipsoid (Karney, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_interpolate_points_spheroid;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_line_interpolate_points_spheroid(&line, 0.25).is_ok());
/// ```
pub fn st_line_interpolate_points_spheroid(blob: &[u8], fraction: f64) -> Result<Vec<u8>> {
    let fraction = require_step(fraction, "ST_LineInterpolatePointsSpheroid")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineInterpolatePointsSpheroid")?;
    interpolate_many(&Geodesic, &line, fraction, srid)
}

/// ST_LineSubstring: the part of a line between two fractions of its length.
///
/// Equal fractions collapse to a Point, the way PostGIS does.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_substring;
/// use sqlitegis::core::functions::io::{as_text, geom_from_text};
///
/// let line = geom_from_text("LINESTRING(0 0, 4 0)", None).unwrap();
/// let mid = st_line_substring(&line, 0.25, 0.75).unwrap();
/// assert_eq!(as_text(&mid).unwrap(), "LINESTRING(1 0,3 0)");
/// ```
pub fn st_line_substring(blob: &[u8], start: f64, end: f64) -> Result<Vec<u8>> {
    let (start, end) = require_window(start, end, "ST_LineSubstring")?;
    let (line, srid) = parse_line(blob)?;
    substring(&Euclidean, &line, start, end, srid)
}

/// ST_LineSubstringSphere: [`st_line_substring`] measured on a sphere of the
/// mean earth radius (SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_substring_sphere;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_line_substring_sphere(&line, 0.25, 0.75).is_ok());
/// ```
pub fn st_line_substring_sphere(blob: &[u8], start: f64, end: f64) -> Result<Vec<u8>> {
    let (start, end) = require_window(start, end, "ST_LineSubstringSphere")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineSubstringSphere")?;
    substring(&Haversine, &line, start, end, srid)
}

/// ST_LineSubstringSpheroid: [`st_line_substring`] measured on the WGS84
/// ellipsoid (Karney, SRID 4326).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::linear_referencing::st_line_substring_spheroid;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(0 0, 90 60)", Some(4326)).unwrap();
/// assert!(st_line_substring_spheroid(&line, 0.25, 0.75).is_ok());
/// ```
pub fn st_line_substring_spheroid(blob: &[u8], start: f64, end: f64) -> Result<Vec<u8>> {
    let (start, end) = require_window(start, end, "ST_LineSubstringSpheroid")?;
    let (line, srid) = parse_geographic_line(blob, "ST_LineSubstringSpheroid")?;
    substring(&Geodesic, &line, start, end, srid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::functions::io::{as_text, geom_from_text};

    /// Reference values are PostGIS 3.5 readings for the same line, taken
    /// through `geometry` for the planar form and through `geography` with
    /// `use_spheroid` on and off for the two curved ones.
    const LINE: &str = "LINESTRING(0 0, 90 60)";

    fn line(srid: Option<i32>) -> Vec<u8> {
        geom_from_text(LINE, srid).unwrap()
    }

    fn wkt(blob: &[u8]) -> String {
        as_text(blob).unwrap()
    }

    /// Compare against a PostGIS reading to 9 decimal places of longitude and
    /// latitude, which is well under a millimetre and far tighter than the
    /// difference between the earth models under test.
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
            assert!(
                (a - b).abs() < 1e-9,
                "got {got}, want {want} (differ at {a} vs {b})"
            );
        }
    }

    #[test]
    fn line_interpolate_point_planar_matches_postgis() {
        let p = st_line_interpolate_point(&line(None), 0.5).unwrap();
        assert_wkt_close(&wkt(&p), "POINT(45 30)");
    }

    #[test]
    fn line_interpolate_point_sphere_matches_postgis() {
        let p = st_line_interpolate_point_sphere(&line(Some(4326)), 0.5).unwrap();
        assert_wkt_close(&wkt(&p), "POINT(26.565051177078 37.761243907035)");
    }

    #[test]
    fn line_interpolate_point_spheroid_matches_postgis() {
        let p = st_line_interpolate_point_spheroid(&line(Some(4326)), 0.5).unwrap();
        assert_wkt_close(&wkt(&p), "POINT(26.606325079977 37.874403088301)");
    }

    #[test]
    fn the_three_earth_models_disagree() {
        // Guards against a variant silently delegating to the wrong model.
        let planar = wkt(&st_line_interpolate_point(&line(Some(4326)), 0.5).unwrap());
        let sphere = wkt(&st_line_interpolate_point_sphere(&line(Some(4326)), 0.5).unwrap());
        let spheroid = wkt(&st_line_interpolate_point_spheroid(&line(Some(4326)), 0.5).unwrap());
        assert_ne!(planar, sphere);
        assert_ne!(sphere, spheroid);
    }

    #[test]
    fn line_interpolate_point_keeps_the_srid() {
        let p = st_line_interpolate_point_spheroid(&line(Some(4326)), 0.5).unwrap();
        assert_eq!(
            crate::core::functions::accessors::st_srid(&p).unwrap(),
            4326
        );
    }
    #[test]
    fn line_interpolate_points_planar_matches_postgis() {
        let p = st_line_interpolate_points(&line(None), 0.25).unwrap();
        assert_wkt_close(&wkt(&p), "MULTIPOINT((22.5 15),(45 30),(67.5 45),(90 60))");
    }

    #[test]
    fn line_interpolate_points_sphere_matches_postgis() {
        let p = st_line_interpolate_points_sphere(&line(Some(4326)), 0.25).unwrap();
        assert_wkt_close(
            &wkt(&p),
            "MULTIPOINT((11.700919508154 19.35459615165),(26.565051177078 37.761243907035),(50.360727762244 53.139953123517),(90 60))",
        );
    }

    #[test]
    fn line_interpolate_points_spheroid_matches_postgis() {
        let p = st_line_interpolate_points_spheroid(&line(Some(4326)), 0.25).unwrap();
        assert_wkt_close(
            &wkt(&p),
            "MULTIPOINT((11.716741571563 19.435698270019),(26.606325079977 37.874403088301),(50.432454297567 53.213438567481),(90 60))",
        );
    }

    #[test]
    fn line_substring_planar_matches_postgis() {
        let s = st_line_substring(&line(None), 0.25, 0.75).unwrap();
        assert_wkt_close(&wkt(&s), "LINESTRING(22.5 15,67.5 45)");
    }

    #[test]
    fn line_substring_spheroid_matches_postgis() {
        let s = st_line_substring_spheroid(&line(Some(4326)), 0.25, 0.75).unwrap();
        assert_wkt_close(
            &wkt(&s),
            "LINESTRING(11.716741571563 19.435698270019,50.432454297567 53.213438567481)",
        );
    }

    #[test]
    fn line_substring_sphere_ends_on_the_sphere_interpolations() {
        // PostGIS offers no spherical substring to read, so the contract is
        // that the ends agree with the spherical interpolation at the same
        // fractions, which PostGIS does publish.
        let s = st_line_substring_sphere(&line(Some(4326)), 0.25, 0.75).unwrap();
        assert_wkt_close(
            &wkt(&s),
            "LINESTRING(11.700919508154 19.35459615165,50.360727762244 53.139953123517)",
        );
    }

    #[test]
    fn line_substring_keeps_interior_vertices() {
        let zigzag = geom_from_text("LINESTRING(0 0,10 0,10 10,20 10)", None).unwrap();
        let s = st_line_substring(&zigzag, 0.0, 1.0).unwrap();
        assert_wkt_close(&wkt(&s), "LINESTRING(0 0,10 0,10 10,20 10)");
    }

    #[test]
    fn degenerate_line_substring_is_a_point() {
        // PostGIS returns a POINT when the two fractions are equal.
        let s = st_line_substring(&line(None), 0.5, 0.5).unwrap();
        assert_wkt_close(&wkt(&s), "POINT(45 30)");
    }

    #[test]
    fn line_substring_rejects_reversed_fractions() {
        let err =
            st_line_substring(&line(None), 0.75, 0.25).expect_err("reversed fractions must error");
        assert!(
            format!("{err}").contains("start fraction"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fractions_outside_the_unit_interval_are_rejected() {
        for f in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(st_line_interpolate_point(&line(None), f).is_err());
            assert!(st_line_interpolate_points(&line(None), f).is_err());
            assert!(st_line_substring(&line(None), 0.0, f).is_err());
        }
    }

    #[test]
    fn line_interpolate_points_rejects_a_zero_fraction() {
        // A zero step would emit points forever.
        assert!(st_line_interpolate_points(&line(None), 0.0).is_err());
    }

    #[test]
    fn linear_referencing_rejects_non_lines() {
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", Some(4326)).unwrap();
        for err in [
            st_line_interpolate_point(&poly, 0.5).expect_err("polygon is not a line"),
            st_line_interpolate_points(&poly, 0.5).expect_err("polygon is not a line"),
            st_line_substring(&poly, 0.0, 1.0).expect_err("polygon is not a line"),
        ] {
            assert!(
                format!("{err}").contains("LineString"),
                "unexpected error: {err}"
            );
        }
        let multi = geom_from_text("MULTILINESTRING((0 0,1 1))", Some(4326)).unwrap();
        assert!(st_line_interpolate_point(&multi, 0.5).is_err());
    }

    #[test]
    fn curved_linear_referencing_requires_srid_4326() {
        let l = line(None);
        for err in [
            st_line_interpolate_point_sphere(&l, 0.5).expect_err("needs SRID"),
            st_line_interpolate_point_spheroid(&l, 0.5).expect_err("needs SRID"),
            st_line_interpolate_points_sphere(&l, 0.5).expect_err("needs SRID"),
            st_line_interpolate_points_spheroid(&l, 0.5).expect_err("needs SRID"),
            st_line_substring_sphere(&l, 0.0, 1.0).expect_err("needs SRID"),
            st_line_substring_spheroid(&l, 0.0, 1.0).expect_err("needs SRID"),
        ] {
            assert!(
                format!("{err}").contains("requires SRID 4326"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn curved_linear_referencing_rejects_out_of_range_latitude() {
        let bad = geom_from_text("LINESTRING(0 0, 10 95)", Some(4326)).unwrap();
        assert!(st_line_interpolate_point_sphere(&bad, 0.5).is_err());
        assert!(st_line_interpolate_point_spheroid(&bad, 0.5).is_err());
        assert!(st_line_substring_spheroid(&bad, 0.0, 1.0).is_err());
    }

    #[test]
    fn empty_line_interpolates_to_an_empty_point() {
        let empty = geom_from_text("LINESTRING EMPTY", None).unwrap();
        let p = st_line_interpolate_point(&empty, 0.5).unwrap();
        assert_eq!(wkt(&p), "POINT EMPTY");
    }

    #[test]
    fn empty_line_interpolate_points_is_an_empty_multipoint() {
        // PostGIS answers POINT EMPTY here, changing the return type on an
        // edge case. We keep the MultiPoint the signature promises.
        let empty = geom_from_text("LINESTRING EMPTY", None).unwrap();
        let pts = st_line_interpolate_points(&empty, 0.5).unwrap();
        assert_eq!(wkt(&pts), "MULTIPOINT EMPTY");
    }

    #[test]
    fn empty_line_substring_is_an_empty_point() {
        // PostGIS answers SQL NULL here. We answer an empty geometry, which
        // keeps the function total.
        let empty = geom_from_text("LINESTRING EMPTY", None).unwrap();
        let s = st_line_substring(&empty, 0.25, 0.75).unwrap();
        assert_eq!(wkt(&s), "POINT EMPTY");
    }
}
