//! Geometry accessor functions.
//!
//! ST_SRID, ST_SetSRID, ST_GeometryType, GeometryType, ST_IsEmpty,
//! ST_X, ST_Y, ST_Z, ST_NDims, ST_CoordDim, ST_Dimension,
//! ST_NumPoints, ST_NPoints, ST_NumGeometries,
//! ST_NumInteriorRings / ST_NumInteriorRing / ST_NumRings,
//! ST_PointN, ST_StartPoint, ST_EndPoint,
//! ST_ExteriorRing, ST_InteriorRingN, ST_GeometryN,
//! ST_Envelope, ST_IsValid, ST_Zmflag, ST_MemSize

use geo::algorithm::Validation;
use geo::{BoundingRect, Geometry};

use crate::error::{GeoLiteError, Result};
use crate::ewkb::{
    geom_type_name, geometry_type_name, is_empty_point_blob, parse_ewkb, set_srid,
    validate_ewkb_payload, write_ewkb, EwkbHeader, WKB_POINT,
};
use crate::functions::emptiness::{is_empty_geometry, is_empty_point};

fn validated_header(blob: &[u8]) -> Result<EwkbHeader> {
    validate_ewkb_payload(blob)
}

/// ST_SRID -- return the SRID stored in the EWKB header.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_srid;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// assert_eq!(st_srid(&blob).unwrap(), 4326);
/// ```
pub fn st_srid(blob: &[u8]) -> Result<i32> {
    let header = validated_header(blob)?;
    Ok(header.srid.unwrap_or(0))
}

/// ST_SetSRID -- rewrite the SRID in the EWKB header.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_srid, st_set_srid};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let updated = st_set_srid(&blob, 3857).unwrap();
/// assert_eq!(st_srid(&updated).unwrap(), 3857);
/// ```
pub fn st_set_srid(blob: &[u8], srid: i32) -> Result<Vec<u8>> {
    set_srid(blob, srid)
}

/// ST_GeometryType / GeometryType -- return the PostGIS geometry type string.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_geometry_type;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_geometry_type(&blob).unwrap(), "ST_Point");
/// ```
pub fn st_geometry_type(blob: &[u8]) -> Result<&'static str> {
    let header = validated_header(blob)?;
    Ok(geom_type_name(header.geom_type))
}

/// ST_NDims -- number of coordinate dimensions (2, 3, or 4).
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_ndims;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_ndims(&blob).unwrap(), 2);
/// ```
pub fn st_ndims(blob: &[u8]) -> Result<i32> {
    let header = validated_header(blob)?;
    let z = if header.has_z { 1 } else { 0 };
    let m = if header.has_m { 1 } else { 0 };
    Ok(2 + z + m)
}

/// ST_CoordDim -- same as ST_NDims for non-curve geometries.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_coord_dim;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_coord_dim(&blob).unwrap(), 2);
/// ```
pub fn st_coord_dim(blob: &[u8]) -> Result<i32> {
    st_ndims(blob)
}

/// ST_Zmflag -- 0=2D, 1=M only, 2=Z only, 3=ZM.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_zmflag;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_zmflag(&blob).unwrap(), 0); // 2D
/// ```
pub fn st_zmflag(blob: &[u8]) -> Result<i32> {
    let header = validated_header(blob)?;
    let flag = match (header.has_z, header.has_m) {
        (false, false) => 0,
        (true, false) => 2,
        (false, true) => 1,
        (true, true) => 3,
    };
    Ok(flag)
}

/// ST_IsEmpty -- true if the geometry has no points.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_is_empty;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert!(!st_is_empty(&blob).unwrap());
///
/// let empty = geom_from_text("LINESTRING EMPTY", None).unwrap();
/// assert!(st_is_empty(&empty).unwrap());
/// ```
pub fn st_is_empty(blob: &[u8]) -> Result<bool> {
    let (geom, _) = parse_ewkb(blob)?;
    Ok(is_empty_geometry(&geom))
}

/// ST_MemSize -- byte length of the EWKB blob.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_mem_size;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_mem_size(&blob).unwrap(), blob.len() as i64);
/// ```
pub fn st_mem_size(blob: &[u8]) -> Result<i64> {
    let _ = validated_header(blob)?;
    Ok(blob.len() as i64)
}

/// ST_X -- X coordinate of a Point.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_x;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(3.5 7.2)", None).unwrap();
/// assert_eq!(st_x(&blob).unwrap(), Some(3.5));
/// ```
pub fn st_x(blob: &[u8]) -> Result<Option<f64>> {
    let (geom, _) = parse_ewkb(blob)?;
    match geom {
        Geometry::Point(p) if is_empty_point(&p) => Ok(None),
        Geometry::Point(p) => Ok(Some(p.x())),
        other => Err(GeoLiteError::WrongType {
            expected: "Point",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_Y -- Y coordinate of a Point.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_y;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(3.5 7.2)", None).unwrap();
/// assert_eq!(st_y(&blob).unwrap(), Some(7.2));
/// ```
pub fn st_y(blob: &[u8]) -> Result<Option<f64>> {
    let (geom, _) = parse_ewkb(blob)?;
    match geom {
        Geometry::Point(p) if is_empty_point(&p) => Ok(None),
        Geometry::Point(p) => Ok(Some(p.y())),
        other => Err(GeoLiteError::WrongType {
            expected: "Point",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_Z -- Z coordinate of a Point when present.
///
/// Contract:
/// - Point Z / Point ZM: returns Z coordinate
/// - Point (XY), Point M, Point EMPTY: returns NULL
/// - non-Point input: wrong-type error
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_z;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(3.5 7.2)", None).unwrap();
/// assert_eq!(st_z(&blob).unwrap(), None);
/// ```
pub fn st_z(blob: &[u8]) -> Result<Option<f64>> {
    let header = validated_header(blob)?;
    if header.geom_type != WKB_POINT {
        return Err(GeoLiteError::WrongType {
            expected: "Point",
            actual: geom_type_name(header.geom_type)
                .strip_prefix("ST_")
                .unwrap_or("Unknown"),
        });
    }
    if is_empty_point_blob(blob)? || !header.has_z {
        return Ok(None);
    }

    let z_offset = header.data_offset + 16;
    if blob.len() < z_offset + 8 {
        return Err(GeoLiteError::InvalidEwkb(format!(
            "point payload truncated: got {} bytes",
            blob.len()
        )));
    }
    let mut z_bytes = [0u8; 8];
    z_bytes.copy_from_slice(&blob[z_offset..z_offset + 8]);
    let z = if header.little_endian {
        f64::from_le_bytes(z_bytes)
    } else {
        f64::from_be_bytes(z_bytes)
    };
    Ok(Some(z))
}

/// ST_NumPoints -- number of points in a LineString.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_num_points;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
/// assert_eq!(st_num_points(&blob).unwrap(), 3);
/// ```
pub fn st_num_points(blob: &[u8]) -> Result<i32> {
    let (geom, _) = parse_ewkb(blob)?;
    match geom {
        Geometry::LineString(ls) => Ok(ls.0.len() as i32),
        other => Err(GeoLiteError::WrongType {
            expected: "LineString",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_NPoints -- total point count across any geometry type.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_npoints;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert_eq!(st_npoints(&blob).unwrap(), 5);
/// ```
pub fn st_npoints(blob: &[u8]) -> Result<i32> {
    fn count(g: &Geometry<f64>) -> usize {
        match g {
            Geometry::Point(p) => usize::from(!is_empty_point(p)),
            Geometry::Line(_) => 2,
            Geometry::LineString(ls) => ls.0.len(),
            Geometry::Polygon(p) => {
                p.exterior().0.len() + p.interiors().iter().map(|r| r.0.len()).sum::<usize>()
            }
            Geometry::MultiPoint(mp) => mp.0.iter().map(|p| usize::from(!is_empty_point(p))).sum(),
            Geometry::MultiLineString(mls) => mls.0.iter().map(|ls| ls.0.len()).sum(),
            Geometry::MultiPolygon(mp) => mp
                .0
                .iter()
                .map(|p| {
                    p.exterior().0.len() + p.interiors().iter().map(|r| r.0.len()).sum::<usize>()
                })
                .sum(),
            Geometry::GeometryCollection(gc) => gc.0.iter().map(count).sum(),
            Geometry::Rect(_) => 4,
            Geometry::Triangle(_) => 3,
        }
    }
    let (geom, _) = parse_ewkb(blob)?;
    Ok(count(&geom) as i32)
}

/// ST_NumGeometries -- component count for multi/collection types.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_num_geometries;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("MULTIPOINT((0 0),(1 1))", None).unwrap();
/// assert_eq!(st_num_geometries(&blob).unwrap(), 2);
///
/// let single = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(st_num_geometries(&single).unwrap(), 1);
/// ```
pub fn st_num_geometries(blob: &[u8]) -> Result<i32> {
    let (geom, _) = parse_ewkb(blob)?;
    let n = match &geom {
        Geometry::MultiPoint(mp) => mp.0.len(),
        Geometry::MultiLineString(mls) => mls.0.len(),
        Geometry::MultiPolygon(mp) => mp.0.len(),
        Geometry::GeometryCollection(gc) => gc.0.len(),
        _ => 1,
    };
    Ok(n as i32)
}

/// ST_NumInteriorRings / ST_NumInteriorRing -- hole count in a Polygon.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_num_interior_rings;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))", None).unwrap();
/// assert_eq!(st_num_interior_rings(&blob).unwrap(), 1);
/// ```
pub fn st_num_interior_rings(blob: &[u8]) -> Result<i32> {
    let (geom, _) = parse_ewkb(blob)?;
    match geom {
        Geometry::Polygon(p) => Ok(p.interiors().len() as i32),
        other => Err(GeoLiteError::WrongType {
            expected: "Polygon",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_NumRings -- total ring count (exterior + interiors) for a Polygon.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_num_rings;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))", None).unwrap();
/// assert_eq!(st_num_rings(&blob).unwrap(), 2);
/// ```
pub fn st_num_rings(blob: &[u8]) -> Result<i32> {
    let (geom, _) = parse_ewkb(blob)?;
    match geom {
        Geometry::Polygon(p) => {
            if p.exterior().0.is_empty() {
                Ok(0)
            } else {
                Ok(1 + p.interiors().len() as i32)
            }
        }
        other => Err(GeoLiteError::WrongType {
            expected: "Polygon",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_PointN -- nth point (1-based) of a LineString.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_point_n, st_x, st_y};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
/// let pt = st_point_n(&blob, 2, None).unwrap();
/// assert!((st_x(&pt).unwrap().unwrap() - 1.0).abs() < 1e-10);
/// assert!((st_y(&pt).unwrap().unwrap() - 1.0).abs() < 1e-10);
/// ```
pub fn st_point_n(blob: &[u8], n: i32, srid: Option<i32>) -> Result<Vec<u8>> {
    let (geom, src_srid) = parse_ewkb(blob)?;
    let srid = srid.or(src_srid);
    match geom {
        Geometry::LineString(ls) => {
            let idx = if n > 0 {
                n as usize - 1
            } else {
                return Err(GeoLiteError::OutOfBounds {
                    index: n,
                    len: ls.0.len(),
                });
            };
            let coord = ls.0.get(idx).ok_or(GeoLiteError::OutOfBounds {
                index: n,
                len: ls.0.len(),
            })?;
            write_ewkb(&Geometry::Point(geo::Point::from(*coord)), srid)
        }
        other => Err(GeoLiteError::WrongType {
            expected: "LineString",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_StartPoint -- first point of a LineString.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_start_point, st_x};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(10 20,30 40)", None).unwrap();
/// let pt = st_start_point(&blob).unwrap();
/// assert!((st_x(&pt).unwrap().unwrap() - 10.0).abs() < 1e-10);
/// ```
pub fn st_start_point(blob: &[u8]) -> Result<Vec<u8>> {
    st_point_n(blob, 1, None)
}

/// ST_EndPoint -- last point of a LineString.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_end_point, st_x};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(10 20,30 40)", None).unwrap();
/// let pt = st_end_point(&blob).unwrap();
/// assert!((st_x(&pt).unwrap().unwrap() - 30.0).abs() < 1e-10);
/// ```
pub fn st_end_point(blob: &[u8]) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    match geom {
        Geometry::LineString(ls) => {
            let last = ls.0.last().ok_or(GeoLiteError::WrongType {
                expected: "non-empty LineString",
                actual: "LineString (empty)",
            })?;
            write_ewkb(&Geometry::Point(geo::Point::from(*last)), srid)
        }
        other => Err(GeoLiteError::WrongType {
            expected: "LineString",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_ExteriorRing -- exterior ring of a Polygon as a LineString.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_exterior_ring, st_num_points};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// let ring = st_exterior_ring(&blob).unwrap();
/// assert_eq!(st_num_points(&ring).unwrap(), 5);
/// ```
pub fn st_exterior_ring(blob: &[u8]) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    match geom {
        Geometry::Polygon(p) => write_ewkb(&Geometry::LineString(p.exterior().clone()), srid),
        other => Err(GeoLiteError::WrongType {
            expected: "Polygon",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_InteriorRingN -- nth interior ring (1-based) of a Polygon.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_interior_ring_n;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text(
///     "POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))",
///     None,
/// ).unwrap();
/// let ring = st_interior_ring_n(&blob, 1).unwrap();
/// assert!(!ring.is_empty());
/// ```
pub fn st_interior_ring_n(blob: &[u8], n: i32) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    match geom {
        Geometry::Polygon(p) => {
            let idx = if n > 0 {
                n as usize - 1
            } else {
                return Err(GeoLiteError::OutOfBounds {
                    index: n,
                    len: p.interiors().len(),
                });
            };
            let ring = p.interiors().get(idx).ok_or(GeoLiteError::OutOfBounds {
                index: n,
                len: p.interiors().len(),
            })?;
            write_ewkb(&Geometry::LineString(ring.clone()), srid)
        }
        other => Err(GeoLiteError::WrongType {
            expected: "Polygon",
            actual: geometry_type_name(&other),
        }),
    }
}

/// ST_GeometryN -- nth sub-geometry (1-based) of a collection.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::{st_geometry_n, st_geometry_type};
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("MULTIPOINT((0 0),(1 1))", None).unwrap();
/// let sub = st_geometry_n(&blob, 1).unwrap();
/// assert_eq!(st_geometry_type(&sub).unwrap(), "ST_Point");
/// ```
pub fn st_geometry_n(blob: &[u8], n: i32) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    let idx = if n > 0 {
        n as usize - 1
    } else {
        return Err(GeoLiteError::OutOfBounds { index: n, len: 0 });
    };
    let (sub, len) = match geom {
        Geometry::MultiPoint(mp) => {
            let len = mp.0.len();
            (mp.0.into_iter().nth(idx).map(Geometry::Point), len)
        }
        Geometry::MultiLineString(mls) => {
            let len = mls.0.len();
            (mls.0.into_iter().nth(idx).map(Geometry::LineString), len)
        }
        Geometry::MultiPolygon(mp) => {
            let len = mp.0.len();
            (mp.0.into_iter().nth(idx).map(Geometry::Polygon), len)
        }
        Geometry::GeometryCollection(gc) => {
            let len = gc.0.len();
            (gc.0.into_iter().nth(idx), len)
        }
        single => {
            if idx == 0 {
                return write_ewkb(&single, srid);
            } else {
                return Err(GeoLiteError::OutOfBounds { index: n, len: 1 });
            }
        }
    };
    match sub {
        Some(g) => write_ewkb(&g, srid),
        None => Err(GeoLiteError::OutOfBounds { index: n, len }),
    }
}

/// ST_Dimension -- topological dimension: 0=point, 1=line, 2=area.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_dimension;
/// use geolite_core::functions::io::geom_from_text;
///
/// let pt = geom_from_text("POINT(0 0)", None).unwrap();
/// assert_eq!(st_dimension(&pt).unwrap(), 0);
///
/// let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
/// assert_eq!(st_dimension(&line).unwrap(), 1);
///
/// let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert_eq!(st_dimension(&poly).unwrap(), 2);
/// ```
pub fn st_dimension(blob: &[u8]) -> Result<i32> {
    fn geometry_dimension(geom: &Geometry<f64>) -> i32 {
        match geom {
            Geometry::Point(_) | Geometry::MultiPoint(_) => 0,
            Geometry::Line(_) | Geometry::LineString(_) | Geometry::MultiLineString(_) => 1,
            Geometry::Polygon(_)
            | Geometry::MultiPolygon(_)
            | Geometry::Rect(_)
            | Geometry::Triangle(_) => 2,
            Geometry::GeometryCollection(gc) => {
                gc.0.iter().map(geometry_dimension).max().unwrap_or(0)
            }
        }
    }

    let (geom, _) = parse_ewkb(blob)?;
    Ok(geometry_dimension(&geom))
}

/// ST_Envelope -- axis-aligned envelope geometry.
///
/// Current behavior:
/// - non-empty: returns the rectangular envelope as a Polygon
/// - empty: returns the same empty geometry unchanged
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_envelope;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("LINESTRING(0 0,3 4)", None).unwrap();
/// let env = st_envelope(&blob).unwrap();
/// assert!(!env.is_empty());
/// ```
pub fn st_envelope(blob: &[u8]) -> Result<Vec<u8>> {
    let (geom, srid) = parse_ewkb(blob)?;
    if is_empty_geometry(&geom) {
        return write_ewkb(&geom, srid);
    }
    let rect = geom
        .bounding_rect()
        .ok_or_else(|| GeoLiteError::WrongType {
            expected: "non-empty geometry",
            actual: geometry_type_name(&geom),
        })?;
    write_ewkb(&Geometry::Rect(rect), srid)
}

/// ST_IsValid -- true when the geometry passes validity checks.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_is_valid;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert!(st_is_valid(&blob).unwrap());
/// ```
pub fn st_is_valid(blob: &[u8]) -> Result<bool> {
    let (geom, _) = parse_ewkb(blob)?;
    Ok(geom.is_valid())
}

/// ST_IsValidReason -- human-readable validity report.
///
/// # Example
///
/// ```
/// use geolite_core::functions::accessors::st_is_valid_reason;
/// use geolite_core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
/// assert_eq!(st_is_valid_reason(&blob).unwrap(), "Valid Geometry");
/// ```
pub fn st_is_valid_reason(blob: &[u8]) -> Result<String> {
    let (geom, _) = parse_ewkb(blob)?;
    if geom.is_valid() {
        Ok("Valid Geometry".to_string())
    } else {
        // geo's Validation trait gives typed errors; collect them
        let mut reasons = Vec::new();
        if let Err(e) = geom.check_validation() {
            reasons.push(format!("{e}"));
        }
        Ok(if reasons.is_empty() {
            "Invalid geometry".to_string()
        } else {
            reasons.join("; ")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ewkb::{EWKB_M_FLAG, EWKB_Z_FLAG, WKB_POINT};
    use crate::functions::io::geom_from_text;

    fn point_blob(flags: u32) -> Vec<u8> {
        let mut blob = vec![0x01];
        blob.extend_from_slice(&(WKB_POINT | flags).to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        if flags & EWKB_Z_FLAG != 0 {
            blob.extend_from_slice(&3.0f64.to_le_bytes());
        }
        if flags & EWKB_M_FLAG != 0 {
            blob.extend_from_slice(&4.0f64.to_le_bytes());
        }
        blob
    }

    // -- Wrong-type errors ------------------------------------------

    #[test]
    fn st_x_on_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_x(&line).is_err());
    }

    #[test]
    fn st_y_on_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_y(&line).is_err());
    }

    #[test]
    fn st_x_y_on_point_empty_return_none() {
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        assert_eq!(st_x(&empty).unwrap(), None);
        assert_eq!(st_y(&empty).unwrap(), None);
        assert_eq!(st_z(&empty).unwrap(), None);
    }

    #[test]
    fn st_z_on_non_point() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        let err = st_z(&line).expect_err("non-point input must error");
        assert!(format!("{err}").contains("not a Point"));
    }

    #[test]
    fn st_num_points_on_non_linestring() {
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        assert!(st_num_points(&pt).is_err());
    }

    #[test]
    fn st_num_interior_rings_on_non_polygon() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_num_interior_rings(&line).is_err());
    }

    #[test]
    fn st_num_rings_on_non_polygon() {
        let line = geom_from_text("LINESTRING(0 0,1 1)", None).unwrap();
        assert!(st_num_rings(&line).is_err());
    }

    #[test]
    fn st_num_rings_polygon_empty() {
        let blob = geom_from_text("POLYGON EMPTY", None).unwrap();
        assert_eq!(st_num_rings(&blob).unwrap(), 0);
    }

    #[test]
    fn st_exterior_ring_on_non_polygon() {
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        assert!(st_exterior_ring(&pt).is_err());
    }

    #[test]
    fn st_end_point_on_non_linestring() {
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        assert!(st_end_point(&pt).is_err());
    }

    // -- Out-of-bounds ----------------------------------------------

    #[test]
    fn st_point_n_zero_index() {
        let line = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        assert!(st_point_n(&line, 0, None).is_err());
    }

    #[test]
    fn st_point_n_negative_index() {
        let line = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        assert!(st_point_n(&line, -1, None).is_err());
    }

    #[test]
    fn st_point_n_too_large() {
        let line = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        assert!(st_point_n(&line, 100, None).is_err());
    }

    #[test]
    fn st_interior_ring_n_no_holes() {
        let poly = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert!(st_interior_ring_n(&poly, 1).is_err());
    }

    #[test]
    fn st_geometry_n_single_geom_index_1() {
        let pt = geom_from_text("POINT(1 2)", None).unwrap();
        // index 1 on a single geometry returns self
        let sub = st_geometry_n(&pt, 1).unwrap();
        assert!((st_x(&sub).unwrap().unwrap() - 1.0).abs() < 1e-10);
    }

    #[test]
    fn st_geometry_n_single_geom_index_2_oob() {
        let pt = geom_from_text("POINT(1 2)", None).unwrap();
        assert!(st_geometry_n(&pt, 2).is_err());
    }

    // -- Validity ---------------------------------------------------

    #[test]
    fn st_is_valid_bowtie() {
        // Bowtie polygon (self-intersecting) -- may or may not be detected depending on geo version
        let bowtie = geom_from_text("POLYGON((0 0,2 2,2 0,0 2,0 0))", None).unwrap();
        // Just verify it doesn't panic
        let _ = st_is_valid(&bowtie);
    }

    // -- Dimension --------------------------------------------------

    #[test]
    fn st_dimension_multipoint() {
        let mp = geom_from_text("MULTIPOINT((0 0),(1 1))", None).unwrap();
        assert_eq!(st_dimension(&mp).unwrap(), 0);
    }

    #[test]
    fn st_dimension_multilinestring() {
        let mls = geom_from_text("MULTILINESTRING((0 0,1 1),(2 2,3 3))", None).unwrap();
        assert_eq!(st_dimension(&mls).unwrap(), 1);
    }

    #[test]
    fn st_dimension_multipolygon() {
        let mp = geom_from_text(
            "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))",
            None,
        )
        .unwrap();
        assert_eq!(st_dimension(&mp).unwrap(), 2);
    }

    #[test]
    fn st_dimension_geometrycollection_uses_max_member_dimension() {
        let gc =
            geom_from_text("GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))", None).unwrap();
        assert_eq!(st_dimension(&gc).unwrap(), 1);
    }

    // -- SRID propagation -------------------------------------------

    #[test]
    fn st_srid_returns_0_when_none() {
        let blob = geom_from_text("POINT(1 2)", None).unwrap();
        assert_eq!(st_srid(&blob).unwrap(), 0);
    }

    #[test]
    fn st_npoints_polygon_with_hole() {
        let blob = geom_from_text(
            "POLYGON((0 0,10 0,10 10,0 10,0 0),(1 1,2 1,2 2,1 2,1 1))",
            None,
        )
        .unwrap();
        assert_eq!(st_npoints(&blob).unwrap(), 10); // 5 exterior + 5 interior
    }

    #[test]
    fn st_is_valid_reason_valid() {
        let blob = geom_from_text("POLYGON((0 0,1 0,1 1,0 1,0 0))", None).unwrap();
        assert_eq!(st_is_valid_reason(&blob).unwrap(), "Valid Geometry");
    }

    #[test]
    fn st_set_srid_rewrites_header() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let updated = st_set_srid(&blob, 3857).unwrap();
        assert_eq!(st_srid(&updated).unwrap(), 3857);
    }

    #[test]
    fn st_geometry_type_and_num_points_success() {
        let point = geom_from_text("POINT(3 4)", None).unwrap();
        assert_eq!(st_geometry_type(&point).unwrap(), "ST_Point");

        let line = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        assert_eq!(st_num_points(&line).unwrap(), 3);
    }

    #[test]
    fn st_ndims_coord_dim_and_zmflag_cover_all_header_flags() {
        let xy = geom_from_text("POINT(1 2)", None).unwrap();
        assert_eq!(st_ndims(&xy).unwrap(), 2);
        assert_eq!(st_coord_dim(&xy).unwrap(), 2);
        assert_eq!(st_zmflag(&xy).unwrap(), 0);

        let z = point_blob(EWKB_Z_FLAG);
        assert_eq!(st_ndims(&z).unwrap(), 3);
        assert_eq!(st_coord_dim(&z).unwrap(), 3);
        assert_eq!(st_zmflag(&z).unwrap(), 2);

        let m = point_blob(EWKB_M_FLAG);
        assert_eq!(st_ndims(&m).unwrap(), 3);
        assert_eq!(st_zmflag(&m).unwrap(), 1);

        let zm = point_blob(EWKB_Z_FLAG | EWKB_M_FLAG);
        assert_eq!(st_ndims(&zm).unwrap(), 4);
        assert_eq!(st_zmflag(&zm).unwrap(), 3);
    }

    #[test]
    fn st_z_point_dimension_contract() {
        let xy = geom_from_text("POINT(1 2)", None).unwrap();
        assert_eq!(st_z(&xy).unwrap(), None);

        let z = point_blob(EWKB_Z_FLAG);
        assert_eq!(st_z(&z).unwrap(), Some(3.0));

        let m = point_blob(EWKB_M_FLAG);
        assert_eq!(st_z(&m).unwrap(), None);

        let zm = point_blob(EWKB_Z_FLAG | EWKB_M_FLAG);
        assert_eq!(st_z(&zm).unwrap(), Some(3.0));
    }

    #[test]
    fn st_z_supports_big_endian_ewkb_point_z() {
        let mut blob = vec![0x00];
        blob.extend_from_slice(&(WKB_POINT | EWKB_Z_FLAG).to_be_bytes());
        blob.extend_from_slice(&1.0f64.to_be_bytes());
        blob.extend_from_slice(&2.0f64.to_be_bytes());
        blob.extend_from_slice(&3.0f64.to_be_bytes());

        assert_eq!(st_z(&blob).unwrap(), Some(3.0));
    }

    #[test]
    fn st_z_rejects_truncated_point_z_payload() {
        let mut blob = vec![0x01];
        blob.extend_from_slice(&(WKB_POINT | EWKB_Z_FLAG).to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());

        let err = st_z(&blob).expect_err("truncated Z payload must error");
        assert!(format!("{err}").contains("point payload truncated"));
    }

    #[test]
    fn st_is_empty_rejects_zm_payload() {
        let mut blob = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG | EWKB_M_FLAG;
        blob.extend_from_slice(&typ.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes());
        blob.extend_from_slice(&4.0f64.to_le_bytes());

        let err = st_is_empty(&blob).expect_err("Z/M payloads must be rejected");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    #[test]
    fn st_is_valid_rejects_zm_payload() {
        let mut blob = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG;
        blob.extend_from_slice(&typ.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes());

        let err = st_is_valid(&blob).expect_err("Z payloads must be rejected");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    #[test]
    fn st_is_empty_checks_multiple_geometry_kinds() {
        let point = geom_from_text("POINT(0 0)", None).unwrap();
        assert!(!st_is_empty(&point).unwrap());

        let empty_point = geom_from_text("POINT EMPTY", None).unwrap();
        assert!(st_is_empty(&empty_point).unwrap());

        let line = geom_from_text("LINESTRING EMPTY", None).unwrap();
        assert!(st_is_empty(&line).unwrap());

        let polygon = geom_from_text("POLYGON EMPTY", None).unwrap();
        assert!(st_is_empty(&polygon).unwrap());

        let multipoint = geom_from_text("MULTIPOINT EMPTY", None).unwrap();
        assert!(st_is_empty(&multipoint).unwrap());

        let multilinestring = geom_from_text("MULTILINESTRING EMPTY", None).unwrap();
        assert!(st_is_empty(&multilinestring).unwrap());

        let multipolygon = geom_from_text("MULTIPOLYGON EMPTY", None).unwrap();
        assert!(st_is_empty(&multipolygon).unwrap());

        let gc = geom_from_text("GEOMETRYCOLLECTION EMPTY", None).unwrap();
        assert!(st_is_empty(&gc).unwrap());
    }

    #[test]
    fn st_is_empty_treats_collections_with_only_empty_members_as_empty() {
        let multilinestring = geom_from_text("MULTILINESTRING(EMPTY,EMPTY)", None).unwrap();
        assert!(st_is_empty(&multilinestring).unwrap());

        let multipolygon = geom_from_text("MULTIPOLYGON(EMPTY)", None).unwrap();
        assert!(st_is_empty(&multipolygon).unwrap());

        let gc = geom_from_text("GEOMETRYCOLLECTION(LINESTRING EMPTY)", None).unwrap();
        assert!(st_is_empty(&gc).unwrap());
    }

    #[test]
    fn st_mem_size_matches_blob_length() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        assert_eq!(st_mem_size(&blob).unwrap(), blob.len() as i64);
    }

    #[test]
    fn st_mem_size_rejects_malformed_ewkb() {
        let malformed = [0x01, 0x02];
        assert!(st_mem_size(&malformed).is_err());
    }

    #[test]
    fn st_npoints_covers_multi_geometries() {
        let point = geom_from_text("POINT(0 0)", None).unwrap();
        assert_eq!(st_npoints(&point).unwrap(), 1);

        let empty_point = geom_from_text("POINT EMPTY", None).unwrap();
        assert_eq!(st_npoints(&empty_point).unwrap(), 0);

        let line = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        assert_eq!(st_npoints(&line).unwrap(), 3);

        let multipoint = geom_from_text("MULTIPOINT((0 0),(1 1))", None).unwrap();
        assert_eq!(st_npoints(&multipoint).unwrap(), 2);

        let multilinestring = geom_from_text("MULTILINESTRING((0 0,1 0),(2 0,2 2))", None).unwrap();
        assert_eq!(st_npoints(&multilinestring).unwrap(), 4);

        let multipolygon = geom_from_text(
            "MULTIPOLYGON(((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1)))",
            None,
        )
        .unwrap();
        assert_eq!(st_npoints(&multipolygon).unwrap(), 10);

        let gc =
            geom_from_text("GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))", None).unwrap();
        assert_eq!(st_npoints(&gc).unwrap(), 3);

        let mls_all_empty = geom_from_text("MULTILINESTRING(EMPTY,EMPTY)", None).unwrap();
        assert_eq!(st_npoints(&mls_all_empty).unwrap(), 0);

        let mpoly_all_empty = geom_from_text("MULTIPOLYGON(EMPTY)", None).unwrap();
        assert_eq!(st_npoints(&mpoly_all_empty).unwrap(), 0);
    }

    #[test]
    fn st_num_geometries_covers_collection_kinds() {
        let mp = geom_from_text("MULTIPOINT((0 0),(1 1),(2 2))", None).unwrap();
        assert_eq!(st_num_geometries(&mp).unwrap(), 3);

        let mls = geom_from_text("MULTILINESTRING((0 0,1 1),(2 2,3 3))", None).unwrap();
        assert_eq!(st_num_geometries(&mls).unwrap(), 2);

        let mpoly = geom_from_text(
            "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))",
            None,
        )
        .unwrap();
        assert_eq!(st_num_geometries(&mpoly).unwrap(), 2);

        let gc = geom_from_text(
            "GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1),POINT(2 2))",
            None,
        )
        .unwrap();
        assert_eq!(st_num_geometries(&gc).unwrap(), 3);

        let single = geom_from_text("POINT(0 0)", None).unwrap();
        assert_eq!(st_num_geometries(&single).unwrap(), 1);
    }

    #[test]
    fn ring_accessors_success_and_bounds_errors() {
        let poly =
            geom_from_text("POLYGON((0 0,4 0,4 4,0 4,0 0),(1 1,2 1,2 2,1 2,1 1))", None).unwrap();

        assert_eq!(st_num_interior_rings(&poly).unwrap(), 1);
        assert_eq!(st_num_rings(&poly).unwrap(), 2);

        let exterior = st_exterior_ring(&poly).unwrap();
        assert_eq!(st_num_points(&exterior).unwrap(), 5);

        let interior = st_interior_ring_n(&poly, 1).unwrap();
        assert_eq!(st_num_points(&interior).unwrap(), 5);

        assert!(st_interior_ring_n(&poly, 0).is_err());
        let pt = geom_from_text("POINT(0 0)", None).unwrap();
        assert!(st_interior_ring_n(&pt, 1).is_err());
    }

    #[test]
    fn st_point_n_start_and_end_success_paths() {
        let line = geom_from_text("LINESTRING(10 20,30 40,50 60)", Some(4326)).unwrap();

        let second = st_point_n(&line, 2, None).unwrap();
        assert!((st_x(&second).unwrap().unwrap() - 30.0).abs() < 1e-10);
        assert!((st_y(&second).unwrap().unwrap() - 40.0).abs() < 1e-10);
        assert_eq!(st_srid(&second).unwrap(), 4326);

        let override_srid = st_point_n(&line, 2, Some(3857)).unwrap();
        assert_eq!(st_srid(&override_srid).unwrap(), 3857);

        let start = st_start_point(&line).unwrap();
        assert!((st_x(&start).unwrap().unwrap() - 10.0).abs() < 1e-10);

        let end = st_end_point(&line).unwrap();
        assert!((st_x(&end).unwrap().unwrap() - 50.0).abs() < 1e-10);

        let empty = geom_from_text("LINESTRING EMPTY", None).unwrap();
        assert!(st_end_point(&empty).is_err());

        let pt = geom_from_text("POINT(1 1)", None).unwrap();
        assert!(st_point_n(&pt, 1, None).is_err());
    }

    #[test]
    fn st_geometry_n_handles_multi_types_and_bounds() {
        let mp = geom_from_text("MULTIPOINT((1 1),(2 2))", Some(4326)).unwrap();
        let mp_sub = st_geometry_n(&mp, 1).unwrap();
        assert_eq!(st_geometry_type(&mp_sub).unwrap(), "ST_Point");
        assert_eq!(st_srid(&mp_sub).unwrap(), 4326);

        let mls = geom_from_text("MULTILINESTRING((0 0,1 1),(2 2,3 3))", None).unwrap();
        let mls_sub = st_geometry_n(&mls, 2).unwrap();
        assert_eq!(st_geometry_type(&mls_sub).unwrap(), "ST_LineString");

        let mpoly = geom_from_text(
            "MULTIPOLYGON(((0 0,1 0,1 1,0 1,0 0)),((2 2,3 2,3 3,2 3,2 2)))",
            None,
        )
        .unwrap();
        let mpoly_sub = st_geometry_n(&mpoly, 1).unwrap();
        assert_eq!(st_geometry_type(&mpoly_sub).unwrap(), "ST_Polygon");

        let gc =
            geom_from_text("GEOMETRYCOLLECTION(POINT(0 0),LINESTRING(0 0,1 1))", None).unwrap();
        let gc_sub = st_geometry_n(&gc, 2).unwrap();
        assert_eq!(st_geometry_type(&gc_sub).unwrap(), "ST_LineString");

        assert!(st_geometry_n(&mp, 0).is_err());
        assert!(st_geometry_n(&mp, 3).is_err());
    }

    #[test]
    fn st_envelope_for_non_empty_and_empty_geometries() {
        let line = geom_from_text("LINESTRING(1 2,3 4)", Some(3857)).unwrap();
        let env = st_envelope(&line).unwrap();
        assert_eq!(st_geometry_type(&env).unwrap(), "ST_Polygon");
        assert_eq!(st_srid(&env).unwrap(), 3857);

        let empty_point = geom_from_text("POINT EMPTY", Some(3857)).unwrap();
        let env_point = st_envelope(&empty_point).unwrap();
        assert_eq!(st_geometry_type(&env_point).unwrap(), "ST_Point");
        assert!(st_is_empty(&env_point).unwrap());
        assert_eq!(st_srid(&env_point).unwrap(), 3857);

        let empty_gc = geom_from_text("GEOMETRYCOLLECTION EMPTY", None).unwrap();
        let env_gc = st_envelope(&empty_gc).unwrap();
        assert_eq!(st_geometry_type(&env_gc).unwrap(), "ST_GeometryCollection");
        assert!(st_is_empty(&env_gc).unwrap());
    }

    #[test]
    fn st_is_valid_reason_invalid_reports_message() {
        let bowtie = geom_from_text("POLYGON((0 0,2 2,2 0,0 2,0 0))", None).unwrap();
        let reason = st_is_valid_reason(&bowtie).unwrap();
        assert_ne!(reason, "Valid Geometry");
        assert!(!reason.is_empty());
    }
}
