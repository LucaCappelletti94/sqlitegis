#![no_main]
//! Geodesic math on VALID inputs. The raw-bytes battery in `fuzz_ewkb_decode`
//! almost always trips the SRID-4326-point precondition before reaching the
//! Haversine / geographiclib code, so this target builds well-formed 4326
//! points from the fuzzer's bytes and exercises the actual geodesic math.
//!
//! Oracles: the functions never panic on valid points, and the results obey
//! basic geodesic invariants (non-negative, finite distances that are
//! symmetric in their arguments).

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use sqlitegis::core::functions::constructors::st_point;
use sqlitegis::core::functions::measurement::{
    st_azimuth, st_distance_sphere, st_distance_spheroid, st_project,
};

/// A pair of geographic points plus a projection vector, all in valid ranges.
#[derive(Arbitrary, Debug)]
struct GeoCase {
    a_lon_raw: i32,
    a_lat_raw: i32,
    b_lon_raw: i32,
    b_lat_raw: i32,
    distance_raw: u32,
    azimuth_raw: i32,
}

/// Map a raw int onto `[-range, range]`.
fn scale(raw: i32, range: f64) -> f64 {
    (f64::from(raw) / f64::from(i32::MAX)) * range
}

fn symmetric(x: f64, y: f64) -> bool {
    let tol = (x.abs() + y.abs()) * 1e-6 + 1e-6;
    (x - y).abs() <= tol
}

fuzz_target!(|case: GeoCase| {
    let a_lon = scale(case.a_lon_raw, 180.0);
    let a_lat = scale(case.a_lat_raw, 90.0);
    let b_lon = scale(case.b_lon_raw, 180.0);
    let b_lat = scale(case.b_lat_raw, 90.0);

    let (Ok(a), Ok(b)) = (
        st_point(a_lon, a_lat, Some(4326)),
        st_point(b_lon, b_lat, Some(4326)),
    ) else {
        return;
    };

    // Spherical (Haversine) distance: non-negative, finite, symmetric.
    if let (Ok(dab), Ok(dba)) = (st_distance_sphere(&a, &b), st_distance_sphere(&b, &a)) {
        assert!(
            dab.is_finite() && dab >= 0.0,
            "sphere distance invalid: {dab}"
        );
        assert!(
            symmetric(dab, dba),
            "sphere distance asymmetric: {dab} vs {dba}"
        );
    }

    // Spheroidal (geographiclib) distance: same invariants.
    if let (Ok(dab), Ok(dba)) = (st_distance_spheroid(&a, &b), st_distance_spheroid(&b, &a)) {
        assert!(
            dab.is_finite() && dab >= 0.0,
            "spheroid distance invalid: {dab} for a=({a_lon},{a_lat}) b=({b_lon},{b_lat})"
        );
        assert!(
            symmetric(dab, dba),
            "spheroid distance asymmetric: {dab} vs {dba}"
        );
    }

    // Azimuth: defined for distinct points, must be finite when it succeeds.
    if let Ok(az) = st_azimuth(&a, &b) {
        assert!(az.is_finite(), "azimuth not finite: {az}");
    }

    // Project a from a finite distance/azimuth: must yield a usable point blob.
    let distance = f64::from(case.distance_raw % 20_000_000); // up to ~half Earth circumference
    let azimuth = scale(case.azimuth_raw, std::f64::consts::PI);
    if let Ok(projected) = st_project(&a, distance, azimuth) {
        assert!(!projected.is_empty(), "projected blob empty");
    }
});
