//! Spatial predicate functions.
//!
//! ST_Intersects, ST_Contains, ST_Within, ST_Disjoint, ST_DWithin,
//! ST_DWithinSphere, ST_DWithinSpheroid,
//! ST_Covers, ST_CoveredBy, ST_Equals, ST_Touches, ST_Crosses,
//! ST_Overlaps, ST_Relate, ST_RelateMatch

use geo::algorithm::{Contains, Intersects, Relate};
use geo::coordinate_position::CoordPos;
use geo::dimensions::Dimensions;

use crate::error::{GeoLiteError, Result};
use crate::ewkb::parse_ewkb_pair;

/// ST_Intersects -- true if the two geometries share at least one point.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_intersects;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POINT(1 1)", None).unwrap();
/// assert!(st_intersects(&a, &b).unwrap());
/// ```
pub fn st_intersects(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.intersects(&gb))
}

/// ST_Contains -- true if A completely contains B (B's boundary subset of A's interior).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_contains;
/// use geolite_core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
/// let pt = geom_from_text("POINT(2 2)", None).unwrap();
/// assert!(st_contains(&poly, &pt).unwrap());
/// ```
pub fn st_contains(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.contains(&gb))
}

/// ST_Within -- true if A is completely inside B.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_within;
/// use geolite_core::functions::io::geom_from_text;
///
/// let pt = geom_from_text("POINT(2 2)", None).unwrap();
/// let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
/// assert!(st_within(&pt, &poly).unwrap());
/// ```
pub fn st_within(a: &[u8], b: &[u8]) -> Result<bool> {
    st_contains(b, a)
}

/// ST_Disjoint -- true if the two geometries share no points.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_disjoint;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POINT(0 0)", None).unwrap();
/// let b = geom_from_text("POINT(10 10)", None).unwrap();
/// assert!(st_disjoint(&a, &b).unwrap());
/// ```
pub fn st_disjoint(a: &[u8], b: &[u8]) -> Result<bool> {
    Ok(!st_intersects(a, b)?)
}

/// ST_DWithin -- true if the geometries are within `distance` of each other (Euclidean).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_dwithin;
/// use geolite_core::functions::constructors::st_point;
///
/// let a = st_point(0.0, 0.0, None).unwrap();
/// let b = st_point(3.0, 4.0, None).unwrap();
/// assert!(st_dwithin(&a, &b, 5.0).unwrap());
/// assert!(!st_dwithin(&a, &b, 4.0).unwrap());
/// ```
pub fn st_dwithin(a: &[u8], b: &[u8], distance: f64) -> Result<bool> {
    use super::measurement::st_distance;
    if !distance.is_finite() {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithin: distance must be finite".to_string(),
        ));
    }
    if distance < 0.0 {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithin: distance must be non-negative".to_string(),
        ));
    }
    Ok(st_distance(a, b)? <= distance)
}

/// ST_DWithinSphere -- true if two geographic points are within `distance` metres
/// using Haversine (spherical) distance (requires SRID 4326).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_dwithin_sphere;
/// use geolite_core::functions::constructors::st_point;
///
/// let a = st_point(-0.1278, 51.5074, Some(4326)).unwrap(); // London
/// let b = st_point(2.3522, 48.8566, Some(4326)).unwrap(); // Paris
/// assert!(st_dwithin_sphere(&a, &b, 400_000.0).unwrap());
/// assert!(!st_dwithin_sphere(&a, &b, 300_000.0).unwrap());
/// ```
pub fn st_dwithin_sphere(a: &[u8], b: &[u8], distance: f64) -> Result<bool> {
    use super::measurement::st_distance_sphere;
    if !distance.is_finite() {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithinSphere: distance must be finite".to_string(),
        ));
    }
    if distance < 0.0 {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithinSphere: distance must be non-negative".to_string(),
        ));
    }
    Ok(st_distance_sphere(a, b)? <= distance)
}

/// ST_DWithinSpheroid -- true if two geographic points are within `distance` metres
/// using geodesic (spheroid) distance (requires SRID 4326).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_dwithin_spheroid;
/// use geolite_core::functions::constructors::st_point;
///
/// let a = st_point(-0.1278, 51.5074, Some(4326)).unwrap(); // London
/// let b = st_point(2.3522, 48.8566, Some(4326)).unwrap(); // Paris
/// assert!(st_dwithin_spheroid(&a, &b, 400_000.0).unwrap());
/// assert!(!st_dwithin_spheroid(&a, &b, 300_000.0).unwrap());
/// ```
pub fn st_dwithin_spheroid(a: &[u8], b: &[u8], distance: f64) -> Result<bool> {
    use super::measurement::st_distance_spheroid;
    if !distance.is_finite() {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithinSpheroid: distance must be finite".to_string(),
        ));
    }
    if distance < 0.0 {
        return Err(GeoLiteError::InvalidInput(
            "ST_DWithinSpheroid: distance must be non-negative".to_string(),
        ));
    }
    Ok(st_distance_spheroid(a, b)? <= distance)
}

/// ST_Covers -- A covers B (B has no point outside A).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_covers;
/// use geolite_core::functions::io::geom_from_text;
///
/// let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
/// let pt = geom_from_text("POINT(2 2)", None).unwrap();
/// assert!(st_covers(&poly, &pt).unwrap());
/// ```
pub fn st_covers(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.relate(&gb).is_covers())
}

/// ST_CoveredBy -- B covers A.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_covered_by;
/// use geolite_core::functions::io::geom_from_text;
///
/// let pt = geom_from_text("POINT(2 2)", None).unwrap();
/// let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
/// assert!(st_covered_by(&pt, &poly).unwrap());
/// ```
pub fn st_covered_by(a: &[u8], b: &[u8]) -> Result<bool> {
    st_covers(b, a)
}

/// ST_Equals -- geometrically equal (same point set, ignoring vertex order).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_equals;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
/// let b = geom_from_text("LINESTRING(1 1,0 0)", None).unwrap();
/// assert!(st_equals(&a, &b).unwrap());
/// ```
pub fn st_equals(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.relate(&gb).is_equal_topo())
}

/// ST_Touches -- geometries share boundary points but not interior points.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_touches;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 0,2 0,2 1,1 1,1 0))", None).unwrap();
/// assert!(st_touches(&a, &b).unwrap());
/// ```
pub fn st_touches(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    // geo 0.32: is_touches() takes 0 arguments
    Ok(ga.relate(&gb).is_touches())
}

/// ST_Crosses -- geometries have some interior points in common but not all.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_crosses;
/// use geolite_core::functions::io::geom_from_text;
///
/// let line = geom_from_text("LINESTRING(-1 0,1 0)", None).unwrap();
/// let poly = geom_from_text("POLYGON((0 -1,1 -1,1 1,0 1,0 -1))", None).unwrap();
/// assert!(st_crosses(&line, &poly).unwrap());
/// ```
pub fn st_crosses(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.relate(&gb).is_crosses())
}

/// ST_Overlaps -- geometries overlap (same dimension, share interior, neither contains the other).
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_overlaps;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POLYGON((1 1,3 1,3 3,1 3,1 1))", None).unwrap();
/// assert!(st_overlaps(&a, &b).unwrap());
/// ```
pub fn st_overlaps(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(ga.relate(&gb).is_overlaps())
}

/// Convert a `Dimensions` entry to its DE-9IM character.
fn dim_char(d: Dimensions) -> char {
    match d {
        Dimensions::Empty => 'F',
        Dimensions::ZeroDimensional => '0',
        Dimensions::OneDimensional => '1',
        Dimensions::TwoDimensional => '2',
    }
}

/// Build the 9-character DE-9IM matrix string (e.g. `"FF2FF1212"`).
fn matrix_string(matrix: &geo::algorithm::relate::IntersectionMatrix) -> String {
    let positions = [CoordPos::Inside, CoordPos::OnBoundary, CoordPos::Outside];
    let mut s = String::with_capacity(9);
    for &lhs in &positions {
        for &rhs in &positions {
            s.push(dim_char(matrix.get(lhs, rhs)));
        }
    }
    s
}

/// ST_Relate -- return the DE-9IM matrix string (e.g. "FF2FF1212").
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_relate;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POINT(0 0)", None).unwrap();
/// let b = geom_from_text("POINT(0 0)", None).unwrap();
/// let matrix = st_relate(&a, &b).unwrap();
/// assert_eq!(matrix.len(), 9);
/// assert_eq!(matrix, "0FFFFFFF2");
/// ```
pub fn st_relate(a: &[u8], b: &[u8]) -> Result<String> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    Ok(matrix_string(&ga.relate(&gb)))
}

/// ST_Relate (pattern) -- check a DE-9IM pattern string against two geometries.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_relate_match_geoms;
/// use geolite_core::functions::io::geom_from_text;
///
/// let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
/// let b = geom_from_text("POINT(1 1)", None).unwrap();
/// // "T*****FF*" is the Contains pattern
/// assert!(st_relate_match_geoms(&a, &b, "T*****FF*").unwrap());
/// ```
pub fn st_relate_match_geoms(a: &[u8], b: &[u8], pattern: &str) -> Result<bool> {
    let (ga, gb, _) = parse_ewkb_pair(a, b)?;
    // Use geo's built-in pattern matching; this validates `pattern`.
    ga.relate(&gb)
        .matches(pattern)
        .map_err(|e| GeoLiteError::InvalidInput(format!("invalid DE-9IM pattern: {e}")))
}

/// ST_RelateMatch -- match a DE-9IM matrix string against a pattern string.
///
/// # Example
///
/// ```
/// use geolite_core::functions::predicates::st_relate_match;
///
/// assert!(st_relate_match("0FFFFFFF2", "0FFF*FFF2").unwrap());
/// assert!(!st_relate_match("0FFFFFFF2", "1********").unwrap());
/// ```
pub fn st_relate_match(matrix: &str, pattern: &str) -> Result<bool> {
    validate_de9im_matrix(matrix)?;
    validate_de9im_pattern(pattern)?;
    Ok(de9im_pattern_match(matrix, pattern))
}

fn validate_de9im_text<F>(value: &str, kind: &str, allowed: F) -> Result<()>
where
    F: Fn(char) -> bool,
{
    if value.len() != 9 {
        return Err(GeoLiteError::InvalidInput(format!(
            "invalid DE-9IM {kind} length: expected 9, got {}",
            value.len()
        )));
    }
    for (idx, ch) in value.chars().enumerate() {
        if !allowed(ch) {
            return Err(GeoLiteError::InvalidInput(format!(
                "invalid DE-9IM {kind} character '{ch}' at position {}",
                idx + 1
            )));
        }
    }
    Ok(())
}

fn validate_de9im_matrix(matrix: &str) -> Result<()> {
    validate_de9im_text(matrix, "matrix", |ch| matches!(ch, 'F' | '0' | '1' | '2'))
}

fn validate_de9im_pattern(pattern: &str) -> Result<()> {
    validate_de9im_text(pattern, "pattern", |ch| {
        matches!(ch, 'T' | 'F' | '*' | '0' | '1' | '2')
    })
}

/// Pure DE-9IM pattern matcher.
/// Pattern chars: T=non-empty, F=empty, *=any, 0/1/2=exact dimension.
fn de9im_pattern_match(matrix: &str, pattern: &str) -> bool {
    if matrix.len() != 9 || pattern.len() != 9 {
        return false;
    }
    matrix.chars().zip(pattern.chars()).all(|(m, p)| match p {
        '*' => true,
        'T' => matches!(m, '0' | '1' | '2'),
        'F' => m == 'F',
        '0' => m == '0',
        '1' => m == '1',
        '2' => m == '2',
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::functions::constructors::st_point;
    use crate::functions::io::geom_from_text;

    // -- de9im_pattern_match (private) ------------------------------

    #[test]
    fn pattern_match_wrong_length() {
        assert!(!de9im_pattern_match("0FFF", "T*****"));
        assert!(!de9im_pattern_match("0FFFFFFF2", "T*"));
    }

    #[test]
    fn pattern_match_wildcard() {
        assert!(de9im_pattern_match("012FF0102", "*********"));
    }

    #[test]
    fn pattern_match_t_matches_012() {
        assert!(de9im_pattern_match("0FFFFFFF2", "T*******T"));
    }

    #[test]
    fn pattern_match_t_does_not_match_f() {
        assert!(!de9im_pattern_match("FFFFFFFFF", "T********"));
    }

    #[test]
    fn pattern_match_f_matches_f() {
        assert!(de9im_pattern_match("FFFFFFFFF", "FFFFFFFFF"));
    }

    #[test]
    fn pattern_match_exact_digits() {
        assert!(de9im_pattern_match("0FFFFFFF2", "0FFFFFFF2"));
        assert!(!de9im_pattern_match("0FFFFFFF2", "1FFFFFFF2"));
    }

    #[test]
    fn pattern_match_unknown_char_fails() {
        assert!(!de9im_pattern_match("0FFFFFFF2", "XFFFFFFFF"));
    }

    // -- Predicate consistency --------------------------------------

    #[test]
    fn within_is_reverse_contains() {
        let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
        let pt = st_point(2.0, 2.0, None).unwrap();
        assert_eq!(
            st_within(&pt, &poly).unwrap(),
            st_contains(&poly, &pt).unwrap()
        );
    }

    #[test]
    fn disjoint_is_not_intersects() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(10.0, 10.0, None).unwrap();
        assert_eq!(
            st_disjoint(&a, &b).unwrap(),
            !st_intersects(&a, &b).unwrap()
        );
    }

    #[test]
    fn covered_by_is_reverse_covers() {
        let poly = geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0))", None).unwrap();
        let pt = st_point(2.0, 2.0, None).unwrap();
        assert_eq!(
            st_covered_by(&pt, &poly).unwrap(),
            st_covers(&poly, &pt).unwrap()
        );
    }

    // -- Specific predicates ----------------------------------------

    #[test]
    fn touches_adjacent_squares() {
        let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 0,2 0,2 1,1 1,1 0))", None).unwrap();
        assert!(st_touches(&a, &b).unwrap());
        assert!(!st_overlaps(&a, &b).unwrap());
    }

    #[test]
    fn crosses_line_through_polygon() {
        let line = geom_from_text("LINESTRING(-1 0.5,2 0.5)", None).unwrap();
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert!(st_crosses(&line, &poly).unwrap());
    }

    #[test]
    fn overlaps_partial() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((1 1,3 1,3 3,1 3,1 1))", None).unwrap();
        assert!(st_overlaps(&a, &b).unwrap());
        assert!(!st_contains(&a, &b).unwrap());
        assert!(!st_within(&a, &b).unwrap());
    }

    #[test]
    fn st_relate_known_matrix() {
        let a = geom_from_text("POINT(0 0)", None).unwrap();
        let b = geom_from_text("POINT(1 1)", None).unwrap();
        let matrix = st_relate(&a, &b).unwrap();
        assert_eq!(matrix.len(), 9);
        // Two disjoint points: FF0FFF0F2
        assert_eq!(matrix, "FF0FFF0F2");
    }

    #[test]
    fn st_relate_match_string() {
        assert!(st_relate_match("FF0FFF0F2", "FF0FFF0F2").unwrap());
        assert!(st_relate_match("FF0FFF0F2", "FF*FFF*F*").unwrap());
        assert!(!st_relate_match("FF0FFF0F2", "T********").unwrap());
    }

    #[test]
    fn st_relate_match_rejects_invalid_pattern() {
        assert!(st_relate_match("FF0FFF0F2", "INVALID").is_err());
        assert!(st_relate_match("FF0FFF0F2", "T*").is_err());
    }

    #[test]
    fn st_relate_match_rejects_invalid_matrix() {
        assert!(st_relate_match("INVALID", "FF*FFF*F*").is_err());
        assert!(st_relate_match("TFFFFFFF2", "FF*FFF*F*").is_err());
        assert!(st_relate_match("FF0FFF0F*", "FF*FFF*F*").is_err());
    }

    #[test]
    fn st_relate_match_geoms_handles_valid_and_invalid_patterns() {
        let a = geom_from_text("POLYGON((0 0,2 0,2 2,0 2,0 0))", None).unwrap();
        let b = geom_from_text("POINT(1 1)", None).unwrap();
        assert!(st_relate_match_geoms(&a, &b, "T*****FF*").unwrap());
        assert!(st_relate_match_geoms(&a, &b, "INVALID").is_err());
    }

    #[test]
    fn st_relate_line_intersection_matrix_includes_one_dimensional_entry() {
        let a = geom_from_text("LINESTRING(0 0,2 2)", None).unwrap();
        let b = geom_from_text("LINESTRING(0 2,2 0)", None).unwrap();
        let matrix = st_relate(&a, &b).unwrap();
        assert!(matrix.contains('1'));
    }

    #[test]
    fn st_equals_same_geometry() {
        let a = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        let b = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert!(st_equals(&a, &b).unwrap());
    }

    #[test]
    fn st_dwithin_boundary() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(3.0, 4.0, None).unwrap();
        // distance = 5.0 exactly
        assert!(st_dwithin(&a, &b, 5.0).unwrap());
        assert!(!st_dwithin(&a, &b, 4.99).unwrap());
    }

    #[test]
    fn st_dwithin_non_finite_distance_rejected() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(3.0, 4.0, None).unwrap();
        for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = st_dwithin(&a, &b, distance)
                .expect_err("ST_DWithin should reject non-finite distance");
            assert!(
                format!("{err}").contains("distance must be finite"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn st_dwithin_negative_distance_rejected() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(3.0, 4.0, None).unwrap();
        let err = st_dwithin(&a, &b, -1.0).expect_err("ST_DWithin should reject negative distance");
        assert!(
            format!("{err}").contains("distance must be non-negative"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn st_dwithin_sphere_boundary() {
        let a = st_point(-0.1278, 51.5074, Some(4326)).unwrap();
        let b = st_point(2.3522, 48.8566, Some(4326)).unwrap();
        let d = super::super::measurement::st_distance_sphere(&a, &b).unwrap();

        assert!(st_dwithin_sphere(&a, &b, d).unwrap());
        assert!(!st_dwithin_sphere(&a, &b, d - 1.0).unwrap());
    }

    #[test]
    fn st_dwithin_spheroid_boundary() {
        let a = st_point(-0.1278, 51.5074, Some(4326)).unwrap();
        let b = st_point(2.3522, 48.8566, Some(4326)).unwrap();
        let d = super::super::measurement::st_distance_spheroid(&a, &b).unwrap();

        assert!(st_dwithin_spheroid(&a, &b, d).unwrap());
        assert!(!st_dwithin_spheroid(&a, &b, d - 1.0).unwrap());
    }

    #[test]
    fn st_dwithin_geodesic_non_finite_distance_rejected() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, Some(4326)).unwrap();
        for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = st_dwithin_sphere(&a, &b, distance)
                .expect_err("ST_DWithinSphere should reject non-finite distance");
            assert!(
                format!("{err}").contains("distance must be finite"),
                "unexpected error: {err}"
            );

            let err = st_dwithin_spheroid(&a, &b, distance)
                .expect_err("ST_DWithinSpheroid should reject non-finite distance");
            assert!(
                format!("{err}").contains("distance must be finite"),
                "unexpected error: {err}"
            );
        }
    }

    #[test]
    fn st_dwithin_geodesic_negative_distance_rejected() {
        let a = st_point(0.0, 0.0, Some(4326)).unwrap();
        let b = st_point(1.0, 1.0, Some(4326)).unwrap();
        let err = st_dwithin_sphere(&a, &b, -1.0)
            .expect_err("ST_DWithinSphere should reject negative distance");
        assert!(
            format!("{err}").contains("distance must be non-negative"),
            "unexpected error: {err}"
        );
        let err = st_dwithin_spheroid(&a, &b, -1.0)
            .expect_err("ST_DWithinSpheroid should reject negative distance");
        assert!(
            format!("{err}").contains("distance must be non-negative"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn st_dwithin_geodesic_requires_4326_and_points() {
        let a = st_point(0.0, 0.0, None).unwrap();
        let b = st_point(1.0, 1.0, Some(4326)).unwrap();
        assert!(st_dwithin_sphere(&a, &b, 1_000.0).is_err());
        assert!(st_dwithin_spheroid(&a, &b, 1_000.0).is_err());

        let line = geom_from_text("LINESTRING(0 0,1 1)", Some(4326)).unwrap();
        let pt = st_point(0.0, 0.0, Some(4326)).unwrap();
        assert!(st_dwithin_sphere(&line, &pt, 1_000.0).is_err());
        assert!(st_dwithin_spheroid(&line, &pt, 1_000.0).is_err());
    }
}
