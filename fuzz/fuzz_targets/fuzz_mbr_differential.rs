#![no_main]
//! Structured differential for the MBR fastpath. Builds a valid `geo::Geometry`
//! from the fuzzer's bytes, serializes to EWKB, and asserts `extract_mbr`
//! returns the bounding box of every vertex. A divergence means the fastpath
//! used by the spatial predicates returns wrong results.
//!
//! The reference is an all-coordinates fold over `CoordsIter`, not
//! `BoundingRect` (which for a Polygon reads only the exterior ring, diverging
//! on degenerate rings). The fold matches what `extract_mbr` computes while
//! staying independent enough to catch byte-walker offset bugs.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use geo::{
    Coord, CoordsIter, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint,
    MultiPolygon, Point, Polygon, Rect,
};
use sqlitegis::core::ewkb::{extract_mbr, write_ewkb};

/// Keeps generated nesting under the parser's MAX_NESTING_DEPTH.
const MAX_BUILD_DEPTH: usize = 16;

#[derive(Arbitrary, Debug)]
enum GeomSpec {
    Point(i16, i16),
    Line(Vec<(i16, i16)>),
    Poly(Vec<Vec<(i16, i16)>>),
    MultiPoint(Vec<(i16, i16)>),
    MultiLine(Vec<Vec<(i16, i16)>>),
    MultiPoly(Vec<Vec<Vec<(i16, i16)>>>),
    Collection(Vec<GeomSpec>),
}

fn coord((x, y): &(i16, i16)) -> Coord<f64> {
    Coord {
        x: f64::from(*x),
        y: f64::from(*y),
    }
}

fn ring(pts: &[(i16, i16)]) -> LineString<f64> {
    LineString::new(pts.iter().map(coord).collect())
}

fn polygon(rings: &[Vec<(i16, i16)>]) -> Polygon<f64> {
    match rings.split_first() {
        Some((exterior, interiors)) => {
            Polygon::new(ring(exterior), interiors.iter().map(|r| ring(r)).collect())
        }
        None => Polygon::new(LineString::new(vec![]), vec![]),
    }
}

fn to_geometry(spec: &GeomSpec, depth: usize) -> Geometry<f64> {
    match spec {
        GeomSpec::Point(x, y) => Geometry::Point(Point::new(f64::from(*x), f64::from(*y))),
        GeomSpec::Line(pts) => Geometry::LineString(ring(pts)),
        GeomSpec::Poly(rings) => Geometry::Polygon(polygon(rings)),
        GeomSpec::MultiPoint(pts) => Geometry::MultiPoint(MultiPoint::new(
            pts.iter().map(|p| Point(coord(p))).collect(),
        )),
        GeomSpec::MultiLine(lines) => Geometry::MultiLineString(MultiLineString::new(
            lines.iter().map(|l| ring(l)).collect(),
        )),
        GeomSpec::MultiPoly(polys) => Geometry::MultiPolygon(MultiPolygon::new(
            polys.iter().map(|p| polygon(p)).collect(),
        )),
        GeomSpec::Collection(specs) => {
            let inner = if depth >= MAX_BUILD_DEPTH {
                vec![]
            } else {
                specs.iter().map(|s| to_geometry(s, depth + 1)).collect()
            };
            Geometry::GeometryCollection(GeometryCollection::new_from(inner))
        }
    }
}

/// Bounding box over every coordinate of `geom` (including polygon interior
/// rings and nested collections), skipping NaN exactly as `extract_mbr` does.
/// Independent of the byte layout, so it is a genuine differential.
fn bbox_all_coords(geom: &Geometry<f64>) -> Option<Rect<f64>> {
    let mut acc: Option<(f64, f64, f64, f64)> = None;
    for c in geom.coords_iter() {
        if c.x.is_nan() || c.y.is_nan() {
            continue;
        }
        acc = Some(match acc {
            Some((mnx, mny, mxx, mxy)) => (mnx.min(c.x), mny.min(c.y), mxx.max(c.x), mxy.max(c.y)),
            None => (c.x, c.y, c.x, c.y),
        });
    }
    acc.map(|(mnx, mny, mxx, mxy)| Rect::new(Coord { x: mnx, y: mny }, Coord { x: mxx, y: mxy }))
}

fn rect_approx_eq(a: Rect<f64>, b: Rect<f64>) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() <= 1e-9 * (x.abs() + y.abs() + 1.0);
    close(a.min().x, b.min().x)
        && close(a.min().y, b.min().y)
        && close(a.max().x, b.max().x)
        && close(a.max().y, b.max().y)
}

fuzz_target!(|spec: GeomSpec| {
    let geom = to_geometry(&spec, 0);

    // Some empty geometries are not representable as EWKB by the writer, so skip
    // those rather than asserting on a serialization we never claim to support.
    let Ok(blob) = write_ewkb(&geom, Some(4326)) else {
        return;
    };

    let fast = extract_mbr(&blob).expect("extract_mbr must succeed on a blob we just wrote");
    let reference = bbox_all_coords(&geom);

    match (fast, reference) {
        (None, None) => {}
        (Some(f), Some(r)) => assert!(
            rect_approx_eq(f, r),
            "MBR mismatch: extract_mbr={f:?} bounding_rect={r:?} for {spec:?}",
        ),
        (f, r) => {
            panic!("MBR presence mismatch: extract_mbr={f:?} bounding_rect={r:?} for {spec:?}")
        }
    }
});
