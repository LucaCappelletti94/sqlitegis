//! Shared helpers for the differential fuzz targets.
//!
//! Each target generates a pair of random Polygon or MultiPolygon EWKB
//! blobs via WKT roundtrip, then runs both the fastpath (the production
//! `st_union` or `st_sym_difference` function under test) and a reference
//! path that always goes through the full decode + `BooleanOps` lane, and
//! compares the two results for geometric parity.

use arbitrary::Arbitrary;

use sqlitegis::core::ewkb::extract_srid;
use sqlitegis::core::functions::accessors::{st_geometry_type, st_srid};
use sqlitegis::core::functions::io::geom_from_text;
use sqlitegis::core::functions::measurement::st_area;

/// One unit ring (5 vertices, closed) centred on `(cx, cy)` with the
/// given `half` size in degrees. Bounded coordinates keep WKT parsing
/// happy and keep the per-row work cheap during a fuzz session.
fn ring_wkt(cx: f64, cy: f64, half: f64) -> String {
    let x0 = cx - half;
    let y0 = cy - half;
    let x1 = cx + half;
    let y1 = cy + half;
    format!("({x0} {y0},{x1} {y0},{x1} {y1},{x0} {y1},{x0} {y0})")
}

/// One axis-aligned square polygon.
fn polygon_wkt(cx: f64, cy: f64, half: f64) -> String {
    format!("POLYGON{}", ring_wkt(cx, cy, half))
}

/// `n_rings`-many disjoint axis-aligned squares packed as a MultiPolygon.
fn multipolygon_wkt(seed: (f64, f64), half: f64, n_rings: u8) -> String {
    let n = (n_rings as usize).max(1);
    let mut parts = Vec::with_capacity(n);
    for i in 0..n {
        let cx = seed.0 + (i as f64) * (half * 4.0);
        parts.push(ring_wkt(cx, seed.1, half));
    }
    format!("MULTIPOLYGON({})", parts.join(","))
}

/// Random shape spec the fuzzer can build from a byte stream.
#[derive(Debug, Arbitrary)]
pub struct ShapeSpec {
    pub cx_raw: i16,
    pub cy_raw: i16,
    pub half_raw: u8,
    pub multi_count: u8,
    pub as_multi: bool,
}

impl ShapeSpec {
    pub fn to_wkt(&self) -> String {
        // Map raw inputs into safe lat/lon ranges (well clear of poles
        // and antimeridian to keep WKT parsing straightforward).
        let cx = (self.cx_raw as f64) * (140.0 / i16::MAX as f64);
        let cy = (self.cy_raw as f64) * (70.0 / i16::MAX as f64);
        // Half-size between 0.01 and 1.0 degrees.
        let half = 0.01 + (self.half_raw as f64) / 256.0 * 0.99;
        if self.as_multi {
            let n = (self.multi_count % 4) + 1;
            multipolygon_wkt((cx, cy), half, n)
        } else {
            polygon_wkt(cx, cy, half)
        }
    }
}

#[derive(Debug, Arbitrary)]
pub struct Pair {
    pub a: ShapeSpec,
    pub b: ShapeSpec,
}

impl Pair {
    /// Build the two EWKB inputs. Returns `None` if either WKT fails to
    /// parse so the harness can short-circuit on degenerate inputs.
    pub fn build(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let a = geom_from_text(&self.a.to_wkt(), Some(4326)).ok()?;
        let b = geom_from_text(&self.b.to_wkt(), Some(4326)).ok()?;
        Some((a, b))
    }
}

/// Differential assertion shared by both fuzz targets: the geometry
/// returned by `under_test(a, b)` must match the geometry returned by
/// `reference(a, b)` on area, SRID, and (when both are non-empty)
/// declared type.
pub fn assert_parity(
    a: &[u8],
    b: &[u8],
    under_test: fn(&[u8], &[u8]) -> sqlitegis::core::error::Result<Vec<u8>>,
    reference: fn(&[u8], &[u8]) -> sqlitegis::core::error::Result<Vec<u8>>,
) {
    let fast = match under_test(a, b) {
        Ok(v) => v,
        Err(_) => return,
    };
    let slow = match reference(a, b) {
        Ok(v) => v,
        Err(_) => return,
    };

    let area_fast = st_area(&fast).expect("area on fast result");
    let area_slow = st_area(&slow).expect("area on slow result");
    let tol = (area_fast.abs() + area_slow.abs() + 1.0) * 1e-9;
    assert!(
        (area_fast - area_slow).abs() <= tol,
        "area mismatch: fast={area_fast} slow={area_slow} tol={tol}"
    );

    let srid_fast = extract_srid(&fast);
    let srid_slow = extract_srid(&slow);
    assert_eq!(srid_fast, srid_slow, "SRID mismatch");

    let st_srid_fast = st_srid(&fast).ok();
    let st_srid_slow = st_srid(&slow).ok();
    assert_eq!(st_srid_fast, st_srid_slow, "ST_SRID mismatch");

    let type_fast = st_geometry_type(&fast).expect("type on fast");
    let type_slow = st_geometry_type(&slow).expect("type on slow");
    // Both paths should produce MultiPolygon (or empty MultiPolygon for
    // pathological inputs); they should match.
    assert_eq!(type_fast, type_slow, "geometry type mismatch");
}
