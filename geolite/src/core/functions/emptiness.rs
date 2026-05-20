use geo::Geometry;

pub(crate) fn is_empty_point(p: &geo::Point<f64>) -> bool {
    p.x().is_nan() && p.y().is_nan()
}

fn is_empty_polygon(p: &geo::Polygon<f64>) -> bool {
    p.exterior().0.is_empty()
}

pub(crate) fn is_empty_geometry(geom: &Geometry<f64>) -> bool {
    match geom {
        Geometry::Point(p) => is_empty_point(p),
        Geometry::Line(_) => false,
        Geometry::LineString(ls) => ls.0.is_empty(),
        Geometry::Polygon(p) => is_empty_polygon(p),
        Geometry::MultiPoint(mp) => mp.0.is_empty() || mp.0.iter().all(is_empty_point),
        Geometry::MultiLineString(mls) => {
            mls.0.is_empty() || mls.0.iter().all(|ls| ls.0.is_empty())
        }
        Geometry::MultiPolygon(mp) => mp.0.is_empty() || mp.0.iter().all(is_empty_polygon),
        Geometry::GeometryCollection(gc) => gc.0.is_empty() || gc.0.iter().all(is_empty_geometry),
        Geometry::Rect(_) => false,
        Geometry::Triangle(_) => false,
    }
}
