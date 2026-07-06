#![no_main]
//! Differential fuzzing against GEOS (the engine behind PostGIS and Shapely),
//! pointing a fuzzer at the actual computational geometry rather than the
//! parsing shell. Builds VALID geometry (convex hulls of random points) and
//! compares `sqlitegis`/geo against GEOS. A finding is a geo panic on valid
//! input (caught directly by libFuzzer) or an area/distance disagreeing beyond
//! tolerance.
//!
//! Comparisons only hold on WELL-CONDITIONED geometry. A needle-thin input
//! sliver, or a boolean result that is itself a sliver (a grazing intersection,
//! a near-covering difference), sits at the float robustness limit where geo
//! and GEOS legitimately diverge, so `hull_wkb` filters input slivers and the
//! intersection/difference checks are skipped unless both engines agree the
//! result is substantial.
//!
//! Needs system `libgeos-dev`.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use geo::{Area, BoundingRect, ConvexHull, Geometry, MultiPoint, Point};
use geos::{Geom, Geometry as GeosGeom};

use sqlitegis::core::ewkb::write_ewkb;
use sqlitegis::core::functions::measurement::{st_area, st_distance};
use sqlitegis::core::functions::operations::{st_difference, st_intersection, st_union};

#[derive(Arbitrary, Debug)]
struct Pair {
    a: Vec<(i16, i16)>,
    b: Vec<(i16, i16)>,
}

/// ISO-WKB convex hull of `pts`, or `None` if degenerate or ill-conditioned.
/// A valid hull means any disagreement points at the algorithm, not the input.
fn hull_wkb(pts: &[(i16, i16)]) -> Option<Vec<u8>> {
    if pts.len() < 3 {
        return None;
    }
    let mp = MultiPoint::new(
        pts.iter()
            .map(|(x, y)| Point::new(f64::from(*x), f64::from(*y)))
            .collect(),
    );
    let hull = mp.convex_hull();
    let area = hull.unsigned_area();
    // Reject needle-thin slivers (large area, near-zero thickness), where geo
    // and GEOS diverge at their robustness limit. Require both bbox sides
    // non-degenerate and the hull to fill a fair fraction of its box.
    let bbox = hull.bounding_rect()?;
    let (w, h) = (bbox.width(), bbox.height());
    if area <= 1.0 || w < 8.0 || h < 8.0 || area < 0.05 * w * h {
        return None;
    }
    write_ewkb(&Geometry::Polygon(hull), None).ok()
}

/// 1% relative tolerance (unit floor) to ignore cross-engine rounding while
/// still catching a gross (missing-polygon, wrong-topology) divergence.
fn close(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-2 * a.abs().max(b.abs()).max(1.0)
}

fuzz_target!(|pair: Pair| {
    let (Some(a), Some(b)) = (hull_wkb(&pair.a), hull_wkb(&pair.b)) else {
        return;
    };
    let (Ok(ga), Ok(gb)) = (GeosGeom::new_from_wkb(&a), GeosGeom::new_from_wkb(&b)) else {
        return;
    };
    // Only compare when GEOS agrees both inputs are valid.
    if !(ga.is_valid().unwrap_or(false) && gb.is_valid().unwrap_or(false)) {
        return;
    }

    // Dump both inputs on failure so a divergence is reproducible.
    let inputs = || {
        format!(
            "\n  A={}\n  B={}",
            ga.to_wkt().unwrap_or_default(),
            gb.to_wkt().unwrap_or_default(),
        )
    };

    // A boolean result is comparable only when substantial. Intersection (grazing
    // inputs) and difference (near-covering) can shrink to a robustness-limit
    // sliver. Skip only when both engines agree it is tiny, so a one-sided
    // divergence is still caught.
    let min_input = ga.area().unwrap_or(0.0).min(gb.area().unwrap_or(0.0));
    let substantial = |x: f64, y: f64| min_input > 0.0 && x.max(y) >= 0.01 * min_input;

    // Single-geometry area.
    if let (Ok(sa), Ok(ka)) = (st_area(&a), ga.area()) {
        assert!(close(sa, ka), "ST_Area geo={sa} geos={ka}{}", inputs());
    }

    // Boolean-op result areas: exercises the sweep-line core of both engines.
    // Union is always at least the larger input, so it never shrinks to a sliver.
    if let (Ok(u), Ok(gu)) = (st_union(&a, &b), ga.union(&gb)) {
        let (su, ku) = (
            st_area(&u).unwrap_or(f64::NAN),
            gu.area().unwrap_or(f64::NAN),
        );
        assert!(close(su, ku), "union area geo={su} geos={ku}{}", inputs());
    }
    if let (Ok(i), Ok(gi)) = (st_intersection(&a, &b), ga.intersection(&gb)) {
        let (si, ki) = (
            st_area(&i).unwrap_or(f64::NAN),
            gi.area().unwrap_or(f64::NAN),
        );
        if substantial(si, ki) {
            assert!(
                close(si, ki),
                "intersection area geo={si} geos={ki}{}",
                inputs()
            );
        }
    }
    if let (Ok(d), Ok(gd)) = (st_difference(&a, &b), ga.difference(&gb)) {
        let (sd, kd) = (
            st_area(&d).unwrap_or(f64::NAN),
            gd.area().unwrap_or(f64::NAN),
        );
        if substantial(sd, kd) {
            assert!(
                close(sd, kd),
                "difference area geo={sd} geos={kd}{}",
                inputs()
            );
        }
    }

    // Distance between the two hulls.
    if let (Ok(sd), Ok(kd)) = (st_distance(&a, &b), ga.distance(&gb)) {
        assert!(close(sd, kd), "ST_Distance geo={sd} geos={kd}{}", inputs());
    }
});
