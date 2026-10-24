//! The `geo` types a geometry value can hold: any [`Geometry`](geo::Geometry) or one of its
//! variants, matching PostGIS columns typed `geometry(Point)`,
//! `geometry(Polygon)` and so on.

use geo::{
    Coord, Geometry, GeometryCollection, LineString, MultiLineString, MultiPoint, MultiPolygon,
    Point, Polygon,
};
use geozero::{GeomProcessor, GeozeroGeometry};

use crate::core::{
    error::{Result, SqliteGisError},
    ewkb::{write_ewkb, write_wkb_with_srid},
};

mod sealed {
    pub trait Sealed {}
}

/// A `geo` type that a geometry or geography value holds.
///
/// Implemented for [`Geometry`], which accepts every geometry, and for each
/// of its variants, which accept only geometries of that variant.
///
/// ```
/// use geo::{Geometry, LineString, Point};
/// use sqlitegis::core::{error::SqliteGisError, ewkb::parse_ewkb, geometry_kind::GeometryKind};
///
/// let point = Point::new(13.4, 52.5);
/// let (decoded, srid) = parse_ewkb(&point.to_ewkb(Some(4326)).unwrap()).unwrap();
/// assert_eq!((decoded, srid), (Geometry::Point(point), Some(4326)));
///
/// let line = Geometry::LineString(LineString::from(vec![(0.0, 0.0), (1.0, 1.0)]));
/// assert!(matches!(
///     Point::from_geometry(line),
///     Err(SqliteGisError::WrongType { expected: "Point", actual: "LineString" })
/// ));
/// ```
pub trait GeometryKind: sealed::Sealed + core::fmt::Debug + Sized {
    /// Serialises the geometry as EWKB, carrying the SRID when one is given.
    ///
    /// # Errors
    ///
    /// Returns an error when the geometry cannot be encoded as WKB.
    fn to_ewkb(&self, srid: Option<i32>) -> Result<Vec<u8>>;

    /// Narrows a decoded geometry to this type.
    ///
    /// # Errors
    ///
    /// Returns [`SqliteGisError::WrongType`] when the geometry is of another
    /// variant.
    fn from_geometry(geometry: Geometry<f64>) -> Result<Self>;
}

impl sealed::Sealed for Geometry<f64> {}

impl GeometryKind for Geometry<f64> {
    fn to_ewkb(&self, srid: Option<i32>) -> Result<Vec<u8>> {
        write_ewkb(self, srid)
    }

    fn from_geometry(geometry: Geometry<f64>) -> Result<Self> {
        Ok(geometry)
    }
}

impl sealed::Sealed for Point<f64> {}

impl GeometryKind for Point<f64> {
    fn to_ewkb(&self, srid: Option<i32>) -> Result<Vec<u8>> {
        // A point is `Copy`, and `write_ewkb` handles the empty point.
        write_ewkb(&Geometry::Point(*self), srid)
    }

    fn from_geometry(geometry: Geometry<f64>) -> Result<Self> {
        match geometry {
            Geometry::Point(point) => Ok(point),
            other => Err(SqliteGisError::wrong_type("Point", &other)),
        }
    }
}

/// Lends a `geo` variant to `geozero`'s WKB writer without copying it into a
/// [`Geometry`].
struct Borrowed<'a, G>(&'a G);

/// Implements [`GeometryKind`] for a [`Geometry`] variant, walking it the way
/// `geozero` walks the same variant inside a [`Geometry`].
macro_rules! geometry_kind {
    ($kind:ident, $name:literal, |$geometry:ident, $processor:ident| $walk:block) => {
        impl sealed::Sealed for $kind<f64> {}

        impl GeozeroGeometry for Borrowed<'_, $kind<f64>> {
            fn process_geom<P: GeomProcessor>(
                &self,
                $processor: &mut P,
            ) -> geozero::error::Result<()> {
                let $geometry = self.0;
                $walk
            }
        }

        impl GeometryKind for $kind<f64> {
            fn to_ewkb(&self, srid: Option<i32>) -> Result<Vec<u8>> {
                write_wkb_with_srid(&Borrowed(self), srid)
            }

            fn from_geometry(geometry: Geometry<f64>) -> Result<Self> {
                match geometry {
                    Geometry::$kind(geometry) => Ok(geometry),
                    other => Err(SqliteGisError::wrong_type($name, &other)),
                }
            }
        }
    };
}

geometry_kind!(LineString, "LineString", |line, processor| {
    process_line_string(line, true, 0, processor)
});

geometry_kind!(Polygon, "Polygon", |polygon, processor| {
    process_polygon(polygon, true, 0, processor)
});

geometry_kind!(MultiPoint, "MultiPoint", |points, processor| {
    processor.multipoint_begin(points.0.len(), 0)?;
    for (index, point) in points.0.iter().enumerate() {
        process_coord(&point.0, index, processor)?;
    }
    processor.multipoint_end(0)
});

geometry_kind!(MultiLineString, "MultiLineString", |lines, processor| {
    processor.multilinestring_begin(lines.0.len(), 0)?;
    for (index, line) in lines.0.iter().enumerate() {
        process_line_string(line, false, index, processor)?;
    }
    processor.multilinestring_end(0)
});

geometry_kind!(MultiPolygon, "MultiPolygon", |polygons, processor| {
    processor.multipolygon_begin(polygons.0.len(), 0)?;
    for (index, polygon) in polygons.0.iter().enumerate() {
        process_polygon(polygon, false, index, processor)?;
    }
    processor.multipolygon_end(0)
});

geometry_kind!(
    GeometryCollection,
    "GeometryCollection",
    |collection, processor| {
        processor.geometrycollection_begin(collection.0.len(), 0)?;
        for geometry in &collection.0 {
            geozero::geo_types::process_geom(geometry, processor)?;
        }
        processor.geometrycollection_end(0)
    }
);

fn process_coord<P: GeomProcessor>(
    coord: &Coord<f64>,
    index: usize,
    processor: &mut P,
) -> geozero::error::Result<()> {
    if processor.multi_dim() {
        processor.coordinate(coord.x, coord.y, None, None, None, None, index)
    } else {
        processor.xy(coord.x, coord.y, index)
    }
}

fn process_line_string<P: GeomProcessor>(
    line: &LineString<f64>,
    tagged: bool,
    index: usize,
    processor: &mut P,
) -> geozero::error::Result<()> {
    processor.linestring_begin(tagged, line.0.len(), index)?;
    for (coord_index, coord) in line.0.iter().enumerate() {
        process_coord(coord, coord_index, processor)?;
    }
    processor.linestring_end(tagged, index)
}

fn process_polygon<P: GeomProcessor>(
    polygon: &Polygon<f64>,
    tagged: bool,
    index: usize,
    processor: &mut P,
) -> geozero::error::Result<()> {
    let interiors = polygon.interiors();
    processor.polygon_begin(tagged, interiors.len() + 1, index)?;
    process_line_string(polygon.exterior(), false, 0, processor)?;
    for (ring_index, ring) in interiors.iter().enumerate() {
        process_line_string(ring, false, ring_index + 1, processor)?;
    }
    processor.polygon_end(tagged, index)
}

#[cfg(test)]
mod tests {
    use geo::{point, polygon, LineString, MultiPoint, Polygon};

    use super::*;

    fn line() -> LineString<f64> {
        LineString::from(vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)])
    }

    fn square() -> Polygon<f64> {
        polygon![
            exterior: [(x: 0.0, y: 0.0), (x: 4.0, y: 0.0), (x: 4.0, y: 4.0), (x: 0.0, y: 4.0), (x: 0.0, y: 0.0)],
            interiors: [[(x: 1.0, y: 1.0), (x: 2.0, y: 1.0), (x: 2.0, y: 2.0), (x: 1.0, y: 1.0)]],
        ]
    }

    /// Asserts that a variant encodes byte for byte like the same geometry
    /// inside a [`Geometry`], with and without an SRID, and narrows back.
    fn assert_encodes_like_geometry<G: GeometryKind>(value: G, geometry: Geometry<f64>) {
        for srid in [None, Some(4326)] {
            assert_eq!(
                value.to_ewkb(srid).unwrap(),
                write_ewkb(&geometry, srid).unwrap()
            );
        }
        let narrowed = G::from_geometry(geometry).unwrap();
        assert_eq!(
            narrowed.to_ewkb(None).unwrap(),
            value.to_ewkb(None).unwrap()
        );
    }

    #[test]
    fn variants_encode_like_the_same_geometry() {
        let point = point!(x: 13.4, y: 52.5);
        assert_encodes_like_geometry(point, Geometry::Point(point));
        let empty = Point::new(f64::NAN, f64::NAN);
        assert_encodes_like_geometry(empty, Geometry::Point(empty));
        assert_encodes_like_geometry(line(), Geometry::LineString(line()));
        assert_encodes_like_geometry(square(), Geometry::Polygon(square()));
        let points = MultiPoint::from(vec![(0.0, 0.0), (1.0, 2.0)]);
        assert_encodes_like_geometry(points.clone(), Geometry::MultiPoint(points));
        let lines = MultiLineString::new(vec![line(), line()]);
        assert_encodes_like_geometry(lines.clone(), Geometry::MultiLineString(lines));
        let polygons = MultiPolygon::new(vec![square(), square()]);
        assert_encodes_like_geometry(polygons.clone(), Geometry::MultiPolygon(polygons));
        let collection = GeometryCollection::new_from(vec![
            Geometry::Point(point),
            Geometry::LineString(line()),
            Geometry::Polygon(square()),
        ]);
        assert_encodes_like_geometry(collection.clone(), Geometry::GeometryCollection(collection));
        assert_encodes_like_geometry(Geometry::Polygon(square()), Geometry::Polygon(square()));
    }

    #[test]
    fn variants_reject_other_geometries() {
        let line = Geometry::LineString(line());
        assert!(matches!(
            Polygon::from_geometry(line.clone()),
            Err(SqliteGisError::WrongType {
                expected: "Polygon",
                actual: "LineString"
            })
        ));
        assert!(matches!(
            MultiPoint::from_geometry(line.clone()),
            Err(SqliteGisError::WrongType {
                expected: "MultiPoint",
                actual: "LineString"
            })
        ));
        assert!(matches!(
            LineString::from_geometry(Geometry::Polygon(square())),
            Err(SqliteGisError::WrongType {
                expected: "LineString",
                actual: "Polygon"
            })
        ));
        assert!(matches!(
            MultiLineString::from_geometry(line.clone()),
            Err(SqliteGisError::WrongType {
                expected: "MultiLineString",
                actual: "LineString"
            })
        ));
        assert!(matches!(
            MultiPolygon::from_geometry(line.clone()),
            Err(SqliteGisError::WrongType {
                expected: "MultiPolygon",
                actual: "LineString"
            })
        ));
        assert!(matches!(
            GeometryCollection::from_geometry(line),
            Err(SqliteGisError::WrongType {
                expected: "GeometryCollection",
                actual: "LineString"
            })
        ));
    }
}
