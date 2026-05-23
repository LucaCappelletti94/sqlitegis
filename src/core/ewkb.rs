//! EWKB (Extended Well-Known Binary) parser and writer.
//!
//! Wire format:
//!   \[0x01|0x00\]: byte order marker (little-endian or big-endian)
//!   \[u32\]: geometry type with flags (in the declared byte order)
//!     Bit 29 (0x20000000): SRID present
//!     Bit 31 (0x80000000): Z dimension
//!     Bit 30 (0x40000000): M dimension
//!     Bits 0-28: geometry type (1=Point, 2=LineString, etc.)
//!   \[i32\]: SRID (only when SRID flag set, in declared byte order)
//!   \[rest\]: ISO WKB geometry payload

use geo::{Coord, Geometry, Point, Rect};
use geozero::wkb::Ewkb;
use geozero::{CoordDimensions, ToGeo, ToWkb};

use crate::core::error::{Result, SqliteGisError};

/// EWKB type flag: SRID is present immediately after the type word.
pub const EWKB_SRID_FLAG: u32 = 0x20000000;
/// EWKB type flag: coordinates include a Z dimension.
pub const EWKB_Z_FLAG: u32 = 0x80000000;
/// EWKB type flag: coordinates include an M (measure) dimension.
pub const EWKB_M_FLAG: u32 = 0x40000000;

/// ISO WKB geometry type code: Point.
pub const WKB_POINT: u32 = 1;
/// ISO WKB geometry type code: LineString.
pub const WKB_LINESTRING: u32 = 2;
/// ISO WKB geometry type code: Polygon.
pub const WKB_POLYGON: u32 = 3;
/// ISO WKB geometry type code: MultiPoint.
pub const WKB_MULTIPOINT: u32 = 4;
/// ISO WKB geometry type code: MultiLineString.
pub const WKB_MULTILINESTRING: u32 = 5;
/// ISO WKB geometry type code: MultiPolygon.
pub const WKB_MULTIPOLYGON: u32 = 6;
/// ISO WKB geometry type code: GeometryCollection.
pub const WKB_GEOMETRYCOLLECTION: u32 = 7;

fn read_f64(bytes: [u8; 8], little_endian: bool) -> f64 {
    if little_endian {
        f64::from_le_bytes(bytes)
    } else {
        f64::from_be_bytes(bytes)
    }
}

/// Reject Z/M coordinate layouts when the operation can only process XY.
///
/// ```
/// use sqlitegis::core::ewkb::ensure_xy_only;
/// use sqlitegis::SqliteGisError;
///
/// assert!(ensure_xy_only(false, false).is_ok());
/// assert!(matches!(
///     ensure_xy_only(true, false),
///     Err(SqliteGisError::UnsupportedDimensions { dimensions: "Z" }),
/// ));
/// ```
pub fn ensure_xy_only(has_z: bool, has_m: bool) -> Result<()> {
    let dimensions = if has_z && has_m {
        "ZM"
    } else if has_z {
        "Z"
    } else if has_m {
        "M"
    } else {
        return Ok(());
    };
    Err(SqliteGisError::UnsupportedDimensions { dimensions })
}

fn point_is_empty_with_header(blob: &[u8], header: &EwkbHeader) -> Result<bool> {
    if header.geom_type != WKB_POINT {
        return Ok(false);
    }

    let dims = 2 + usize::from(header.has_z) + usize::from(header.has_m);
    let needed = header.data_offset + 8 * dims;
    if blob.len() < needed {
        return Err(SqliteGisError::InvalidEwkb(format!(
            "point payload truncated: got {} bytes",
            blob.len()
        )));
    }

    let mut x_bytes = [0u8; 8];
    x_bytes.copy_from_slice(&blob[header.data_offset..header.data_offset + 8]);
    let mut y_bytes = [0u8; 8];
    y_bytes.copy_from_slice(&blob[header.data_offset + 8..header.data_offset + 16]);

    let x = read_f64(x_bytes, header.little_endian);
    let y = read_f64(y_bytes, header.little_endian);
    Ok(x.is_nan() && y.is_nan())
}

/// Return true when the EWKB blob encodes `POINT EMPTY`.
///
/// ```
/// use sqlitegis::core::ewkb::is_empty_point_blob;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let empty = geom_from_text("POINT EMPTY", None).unwrap();
/// assert!(is_empty_point_blob(&empty).unwrap());
///
/// let real = geom_from_text("POINT(1 2)", None).unwrap();
/// assert!(!is_empty_point_blob(&real).unwrap());
/// ```
pub fn is_empty_point_blob(blob: &[u8]) -> Result<bool> {
    let header = parse_ewkb_header(blob)?;
    point_is_empty_with_header(blob, &header)
}

/// Validate EWKB header + payload without forcing XY-only dimensions.
///
/// This helper is intended for metadata-oriented functions that must verify
/// wire correctness but do not need to deserialize into an XY geometry.
///
/// ```
/// use sqlitegis::core::ewkb::validate_ewkb_payload;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let hdr = validate_ewkb_payload(&blob).unwrap();
/// assert_eq!(hdr.srid, Some(4326));
/// ```
pub fn validate_ewkb_payload(blob: &[u8]) -> Result<EwkbHeader> {
    let header = parse_ewkb_header(blob)?;
    if !point_is_empty_with_header(blob, &header)? {
        let _: Geometry<f64> = Ewkb(blob).to_geo()?;
    }
    Ok(header)
}

/// Validate EWKB header + payload and enforce XY-only coordinate dimensions.
///
/// Rejects Z, M, and ZM geometries via [`ensure_xy_only`] after validating
/// the wire format. The XY-only contract matches what every spatial
/// function in `crate::core::functions` accepts on input.
///
/// ```
/// use sqlitegis::core::ewkb::validate_xy_ewkb_payload;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let xy = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let hdr = validate_xy_ewkb_payload(&xy).unwrap();
/// assert!(!hdr.has_z && !hdr.has_m);
/// ```
pub fn validate_xy_ewkb_payload(blob: &[u8]) -> Result<EwkbHeader> {
    let header = validate_ewkb_payload(blob)?;
    ensure_xy_only(header.has_z, header.has_m)?;
    Ok(header)
}

/// Parsed EWKB header metadata.
#[derive(Debug, Clone)]
pub struct EwkbHeader {
    /// Base geometry type code (1=Point, 2=LineString, up to 7=GeometryCollection).
    pub geom_type: u32,
    /// SRID embedded in the EWKB, if the SRID flag is set.
    pub srid: Option<i32>,
    /// Whether the geometry has Z coordinates.
    pub has_z: bool,
    /// Whether the geometry has M coordinates.
    pub has_m: bool,
    /// Byte offset where the geometry payload starts (after header + optional SRID).
    pub data_offset: usize,
    /// Whether numeric header fields are encoded in little-endian order.
    pub little_endian: bool,
}

/// Peek at the EWKB header without fully parsing the geometry.
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::parse_ewkb_header;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let hdr = parse_ewkb_header(&blob).unwrap();
/// assert_eq!(hdr.geom_type, 1); // WKB_POINT
/// assert_eq!(hdr.srid, Some(4326));
/// ```
pub fn parse_ewkb_header(blob: &[u8]) -> Result<EwkbHeader> {
    if blob.len() < 5 {
        return Err(SqliteGisError::InvalidEwkb(format!(
            "blob too short: got {} bytes, need at least 5",
            blob.len()
        )));
    }

    let little_endian = match blob[0] {
        0x01 => true,
        0x00 => false,
        _ => {
            return Err(SqliteGisError::InvalidEwkb(
                "invalid byte order marker".to_string(),
            ))
        }
    };

    let read_u32 = |bytes: [u8; 4]| {
        if little_endian {
            u32::from_le_bytes(bytes)
        } else {
            u32::from_be_bytes(bytes)
        }
    };
    let read_i32 = |bytes: [u8; 4]| {
        if little_endian {
            i32::from_le_bytes(bytes)
        } else {
            i32::from_be_bytes(bytes)
        }
    };

    let raw_type = read_u32([blob[1], blob[2], blob[3], blob[4]]);
    let has_srid = (raw_type & EWKB_SRID_FLAG) != 0;
    let has_z = (raw_type & EWKB_Z_FLAG) != 0;
    let has_m = (raw_type & EWKB_M_FLAG) != 0;
    let geom_type = raw_type & 0x1FFFFFFF;

    let mut offset = 5usize;
    let srid = if has_srid {
        if blob.len() < 9 {
            return Err(SqliteGisError::InvalidEwkb(
                "SRID flag set but blob too short".to_string(),
            ));
        }
        let s = read_i32([blob[5], blob[6], blob[7], blob[8]]);
        offset += 4;
        Some(s)
    } else {
        None
    };

    Ok(EwkbHeader {
        geom_type,
        srid,
        has_z,
        has_m,
        data_offset: offset,
        little_endian,
    })
}

/// Extract only the SRID from an EWKB blob (cheap, no geometry parsing).
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::extract_srid;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// assert_eq!(extract_srid(&blob), Some(4326));
///
/// let no_srid = geom_from_text("POINT(1 2)", None).unwrap();
/// assert_eq!(extract_srid(&no_srid), None);
/// ```
pub fn extract_srid(blob: &[u8]) -> Option<i32> {
    parse_ewkb_header(blob).ok().and_then(|h| h.srid)
}

/// Enforce equal SRIDs for binary geometry operations.
///
/// Returns the shared SRID when both inputs are compatible.
pub fn ensure_matching_srid(left: Option<i32>, right: Option<i32>) -> Result<Option<i32>> {
    let l = left.unwrap_or(0);
    let r = right.unwrap_or(0);
    if l != r {
        return Err(SqliteGisError::InvalidInput(format!(
            "operation on mixed SRID geometries ({l} != {r})"
        )));
    }

    if left.is_none() && right.is_none() {
        Ok(None)
    } else {
        Ok(Some(l))
    }
}

/// Parse an EWKB blob into a `geo::Geometry<f64>`.
/// Returns `(geometry, srid)`.
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::parse_ewkb;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let (geom, srid) = parse_ewkb(&blob).unwrap();
/// assert_eq!(srid, Some(4326));
/// ```
pub fn parse_ewkb(blob: &[u8]) -> Result<(Geometry<f64>, Option<i32>)> {
    let header = parse_ewkb_header(blob)?;
    ensure_xy_only(header.has_z, header.has_m)?;
    if point_is_empty_with_header(blob, &header)? {
        return Ok((Geometry::Point(Point::new(f64::NAN, f64::NAN)), header.srid));
    }
    let geom = Ewkb(blob).to_geo()?;
    Ok((geom, header.srid))
}

/// Parse two EWKB blobs and enforce matching SRID.
///
/// Returns `(left_geometry, right_geometry, shared_srid)`.
pub fn parse_ewkb_pair(a: &[u8], b: &[u8]) -> Result<(Geometry<f64>, Geometry<f64>, Option<i32>)> {
    let (ga, srid_a) = parse_ewkb(a)?;
    let (gb, srid_b) = parse_ewkb(b)?;
    let srid = ensure_matching_srid(srid_a, srid_b)?;
    Ok((ga, gb, srid))
}

/// Compute the planar minimum bounding rectangle of an EWKB blob without
/// allocating a [`Geometry`] enum.
///
/// Walks the EWKB byte payload, reads only the X/Y coordinates, and tracks
/// running min/max. For the "many points vs one window" filter shape this
/// is roughly 10-100x cheaper per call than `parse_ewkb(...).bounding_rect()`
/// because it skips the heap-allocating decode entirely.
///
/// Returns `Ok(None)` when the geometry is empty (empty Point with NaN
/// coordinates, empty LineString, empty Polygon, or any container whose
/// elements are all empty). Returns `Err` only for malformed blobs.
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::extract_mbr;
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0, 10 0, 10 5, 0 5, 0 0))", None).unwrap();
/// let mbr = extract_mbr(&blob).unwrap().unwrap();
/// assert_eq!(mbr.min().x, 0.0);
/// assert_eq!(mbr.min().y, 0.0);
/// assert_eq!(mbr.max().x, 10.0);
/// assert_eq!(mbr.max().y, 5.0);
///
/// let empty = geom_from_text("POINT EMPTY", None).unwrap();
/// assert!(extract_mbr(&empty).unwrap().is_none());
/// ```
pub fn extract_mbr(blob: &[u8]) -> Result<Option<Rect<f64>>> {
    let header = parse_ewkb_header(blob)?;
    let mut acc: BboxAcc = None;
    walk_for_mbr(
        blob,
        header.data_offset,
        header.geom_type,
        header.has_z,
        header.has_m,
        header.little_endian,
        &mut acc,
    )?;
    Ok(acc
        .map(|(mnx, mny, mxx, mxy)| Rect::new(Coord { x: mnx, y: mny }, Coord { x: mxx, y: mxy })))
}

/// Running (min_x, min_y, max_x, max_y) accumulator. `None` means no
/// finite coordinates have been seen yet.
type BboxAcc = Option<(f64, f64, f64, f64)>;

fn update_bbox(acc: &mut BboxAcc, x: f64, y: f64) {
    // Skip NaN coordinates (PostGIS-style empty Points).
    if x.is_nan() || y.is_nan() {
        return;
    }
    match acc {
        Some((mnx, mny, mxx, mxy)) => {
            if x < *mnx {
                *mnx = x;
            }
            if y < *mny {
                *mny = y;
            }
            if x > *mxx {
                *mxx = x;
            }
            if y > *mxy {
                *mxy = y;
            }
        }
        None => *acc = Some((x, y, x, y)),
    }
}

fn read_f64_at(blob: &[u8], offset: usize, little_endian: bool) -> Result<f64> {
    if blob.len() < offset + 8 {
        return Err(SqliteGisError::InvalidEwkb(format!(
            "blob truncated reading f64 at offset {offset}"
        )));
    }
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&blob[offset..offset + 8]);
    Ok(read_f64(bytes, little_endian))
}

fn read_u32_at(blob: &[u8], offset: usize, little_endian: bool) -> Result<u32> {
    if blob.len() < offset + 4 {
        return Err(SqliteGisError::InvalidEwkb(format!(
            "blob truncated reading u32 at offset {offset}"
        )));
    }
    let bytes = [
        blob[offset],
        blob[offset + 1],
        blob[offset + 2],
        blob[offset + 3],
    ];
    Ok(if little_endian {
        u32::from_le_bytes(bytes)
    } else {
        u32::from_be_bytes(bytes)
    })
}

/// Walk the WKB payload at `offset` for a geometry of the given type +
/// dimensions, updating `acc` with each (X, Y) it sees. Returns the offset
/// just past this geometry's payload (so container types can chain).
fn walk_for_mbr(
    blob: &[u8],
    mut offset: usize,
    geom_type: u32,
    has_z: bool,
    has_m: bool,
    little_endian: bool,
    acc: &mut BboxAcc,
) -> Result<usize> {
    let coord_size = 16 + 8 * usize::from(has_z) + 8 * usize::from(has_m);

    match geom_type {
        WKB_POINT => {
            let x = read_f64_at(blob, offset, little_endian)?;
            let y = read_f64_at(blob, offset + 8, little_endian)?;
            update_bbox(acc, x, y);
            offset += coord_size;
        }
        WKB_LINESTRING => {
            let npoints = read_u32_at(blob, offset, little_endian)? as usize;
            offset += 4;
            for _ in 0..npoints {
                let x = read_f64_at(blob, offset, little_endian)?;
                let y = read_f64_at(blob, offset + 8, little_endian)?;
                update_bbox(acc, x, y);
                offset += coord_size;
            }
        }
        WKB_POLYGON => {
            let nrings = read_u32_at(blob, offset, little_endian)? as usize;
            offset += 4;
            for _ in 0..nrings {
                let npoints = read_u32_at(blob, offset, little_endian)? as usize;
                offset += 4;
                for _ in 0..npoints {
                    let x = read_f64_at(blob, offset, little_endian)?;
                    let y = read_f64_at(blob, offset + 8, little_endian)?;
                    update_bbox(acc, x, y);
                    offset += coord_size;
                }
            }
        }
        WKB_MULTIPOINT | WKB_MULTILINESTRING | WKB_MULTIPOLYGON | WKB_GEOMETRYCOLLECTION => {
            let count = read_u32_at(blob, offset, little_endian)? as usize;
            offset += 4;
            for _ in 0..count {
                // Each nested element carries its own WKB mini-header
                // (byte-order byte + 4-byte type). EWKB's SRID flag is only
                // valid at the top level; nested elements use plain WKB.
                if blob.len() < offset + 5 {
                    return Err(SqliteGisError::InvalidEwkb(format!(
                        "nested WKB header truncated at offset {offset}"
                    )));
                }
                let nested_le = match blob[offset] {
                    0x01 => true,
                    0x00 => false,
                    other => {
                        return Err(SqliteGisError::InvalidEwkb(format!(
                            "invalid nested byte-order marker {other} at offset {offset}"
                        )));
                    }
                };
                let nested_type = read_u32_at(blob, offset + 1, nested_le)?;
                let nested_geom_type = nested_type & 0x1FFFFFFF;
                let nested_has_z = (nested_type & EWKB_Z_FLAG) != 0;
                let nested_has_m = (nested_type & EWKB_M_FLAG) != 0;
                offset += 5;
                offset = walk_for_mbr(
                    blob,
                    offset,
                    nested_geom_type,
                    nested_has_z,
                    nested_has_m,
                    nested_le,
                    acc,
                )?;
            }
        }
        other => {
            return Err(SqliteGisError::InvalidEwkb(format!(
                "unsupported geometry type code {other} during MBR extraction"
            )));
        }
    }

    Ok(offset)
}

fn patch_wkb_with_srid(iso_wkb: &[u8], srid_val: i32) -> Result<Vec<u8>> {
    if iso_wkb.len() < 5 {
        return Err(SqliteGisError::InvalidEwkb(
            "WKB output too short".to_string(),
        ));
    }
    let little_endian = match iso_wkb[0] {
        0x01 => true,
        0x00 => false,
        _ => {
            return Err(SqliteGisError::InvalidEwkb(
                "invalid byte order marker".to_string(),
            ))
        }
    };
    let raw_type = if little_endian {
        u32::from_le_bytes([iso_wkb[1], iso_wkb[2], iso_wkb[3], iso_wkb[4]])
    } else {
        u32::from_be_bytes([iso_wkb[1], iso_wkb[2], iso_wkb[3], iso_wkb[4]])
    };
    let ewkb_type = raw_type | EWKB_SRID_FLAG;

    // ISO WKB: [byte_order(1)][type_u32(4)][payload...]
    // EWKB:    [byte_order(1)][type_u32_with_flag(4)][srid_i32(4)][payload...]
    let mut out = Vec::with_capacity(iso_wkb.len() + 4);
    out.push(iso_wkb[0]);
    if little_endian {
        out.extend_from_slice(&ewkb_type.to_le_bytes());
        out.extend_from_slice(&srid_val.to_le_bytes());
    } else {
        out.extend_from_slice(&ewkb_type.to_be_bytes());
        out.extend_from_slice(&srid_val.to_be_bytes());
    }
    out.extend_from_slice(&iso_wkb[5..]);
    Ok(out)
}

/// Serialise a `geo::Geometry<f64>` to EWKB with an optional SRID.
///
/// If `srid` is `None`, produces standard ISO WKB (no SRID flag).
///
/// # Example
///
/// ```
/// use geo::{Geometry, Point};
/// use sqlitegis::core::ewkb::{write_ewkb, parse_ewkb};
///
/// let geom = Geometry::Point(Point::new(1.0, 2.0));
/// let blob = write_ewkb(&geom, Some(4326)).unwrap();
/// let (parsed, srid) = parse_ewkb(&blob).unwrap();
/// assert_eq!(srid, Some(4326));
/// ```
pub fn write_ewkb(geom: &Geometry<f64>, srid: Option<i32>) -> Result<Vec<u8>> {
    if let Geometry::Point(p) = geom {
        if p.x().is_nan() && p.y().is_nan() {
            let mut out = Vec::with_capacity(if srid.is_some() { 25 } else { 21 });
            out.push(0x01);
            let mut geom_type = WKB_POINT;
            if srid.is_some() {
                geom_type |= EWKB_SRID_FLAG;
            }
            out.extend_from_slice(&geom_type.to_le_bytes());
            if let Some(srid_val) = srid {
                out.extend_from_slice(&srid_val.to_le_bytes());
            }
            out.extend_from_slice(&f64::NAN.to_le_bytes());
            out.extend_from_slice(&f64::NAN.to_le_bytes());
            return Ok(out);
        }
    }

    // Use geozero to produce ISO WKB (XY only for now)
    let iso_wkb = geom
        .to_wkb(CoordDimensions::xy())
        .map_err(SqliteGisError::Geozero)?;

    if let Some(srid_val) = srid {
        patch_wkb_with_srid(&iso_wkb, srid_val)
    } else {
        Ok(iso_wkb)
    }
}

/// Rewrite the SRID in an existing EWKB blob without re-parsing the geometry.
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::{set_srid, extract_srid};
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let updated = set_srid(&blob, 3857).unwrap();
/// assert_eq!(extract_srid(&updated), Some(3857));
/// ```
pub fn set_srid(blob: &[u8], new_srid: i32) -> Result<Vec<u8>> {
    // Validate full payload before rewriting header bytes so malformed EWKB
    // cannot be silently "fixed" by adding/replacing an SRID.
    let header = validate_ewkb_payload(blob)?;

    let mut out = Vec::with_capacity(blob.len() + 4);
    out.push(if header.little_endian { 0x01 } else { 0x00 });

    let raw_type = if header.little_endian {
        u32::from_le_bytes([blob[1], blob[2], blob[3], blob[4]])
    } else {
        u32::from_be_bytes([blob[1], blob[2], blob[3], blob[4]])
    };
    let ewkb_type = raw_type | EWKB_SRID_FLAG;
    if header.little_endian {
        out.extend_from_slice(&ewkb_type.to_le_bytes());
        out.extend_from_slice(&new_srid.to_le_bytes());
    } else {
        out.extend_from_slice(&ewkb_type.to_be_bytes());
        out.extend_from_slice(&new_srid.to_be_bytes());
    }

    // Skip old SRID bytes if they were present, copy remaining payload
    out.extend_from_slice(&blob[header.data_offset..]);
    Ok(out)
}

/// Return a static string naming the variant of a `geo::Geometry` value (for diagnostics).
///
/// ```
/// use sqlitegis::core::ewkb::{geometry_type_name, parse_ewkb};
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POLYGON((0 0, 1 0, 1 1, 0 0))", None).unwrap();
/// let (geom, _srid) = parse_ewkb(&blob).unwrap();
/// assert_eq!(geometry_type_name(&geom), "Polygon");
/// ```
pub fn geometry_type_name(geom: &Geometry<f64>) -> &'static str {
    match geom {
        Geometry::Point(_) => "Point",
        Geometry::Line(_) => "Line",
        Geometry::LineString(_) => "LineString",
        Geometry::Polygon(_) => "Polygon",
        Geometry::MultiPoint(_) => "MultiPoint",
        Geometry::MultiLineString(_) => "MultiLineString",
        Geometry::MultiPolygon(_) => "MultiPolygon",
        Geometry::GeometryCollection(_) => "GeometryCollection",
        Geometry::Rect(_) => "Rect",
        Geometry::Triangle(_) => "Triangle",
    }
}

/// Return a human-readable geometry type name (PostGIS convention).
///
/// # Example
///
/// ```
/// use sqlitegis::core::ewkb::{geom_type_name, WKB_POINT, WKB_POLYGON};
///
/// assert_eq!(geom_type_name(WKB_POINT), "ST_Point");
/// assert_eq!(geom_type_name(WKB_POLYGON), "ST_Polygon");
/// assert_eq!(geom_type_name(999), "ST_Unknown");
/// ```
pub fn geom_type_name(raw_type: u32) -> &'static str {
    match raw_type & 0x1FFF_FFFF {
        WKB_POINT => "ST_Point",
        WKB_LINESTRING => "ST_LineString",
        WKB_POLYGON => "ST_Polygon",
        WKB_MULTIPOINT => "ST_MultiPoint",
        WKB_MULTILINESTRING => "ST_MultiLineString",
        WKB_MULTIPOLYGON => "ST_MultiPolygon",
        WKB_GEOMETRYCOLLECTION => "ST_GeometryCollection",
        _ => "ST_Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::functions::io::geom_from_text;

    #[test]
    fn header_blob_too_short() {
        assert!(parse_ewkb_header(&[0x01, 0x02]).is_err());
        assert!(parse_ewkb_header(&[]).is_err());
    }

    #[test]
    fn header_big_endian_point_without_srid() {
        // big-endian: byte-order + type(1) + x(1.0) + y(2.0)
        let mut blob = vec![0x00];
        blob.extend_from_slice(&WKB_POINT.to_be_bytes());
        blob.extend_from_slice(&1.0f64.to_be_bytes());
        blob.extend_from_slice(&2.0f64.to_be_bytes());

        let hdr = parse_ewkb_header(&blob).unwrap();
        assert_eq!(hdr.geom_type, WKB_POINT);
        assert_eq!(hdr.srid, None);
        assert!(!hdr.has_z);
        assert!(!hdr.has_m);
        assert_eq!(hdr.data_offset, 5);
        assert!(!hdr.little_endian);
    }

    #[test]
    fn header_big_endian_point_with_srid() {
        // big-endian EWKB type with SRID flag.
        let mut blob = vec![0x00];
        let typ = WKB_POINT | EWKB_SRID_FLAG;
        blob.extend_from_slice(&typ.to_be_bytes());
        blob.extend_from_slice(&4326i32.to_be_bytes());
        blob.extend_from_slice(&1.0f64.to_be_bytes());
        blob.extend_from_slice(&2.0f64.to_be_bytes());

        let hdr = parse_ewkb_header(&blob).unwrap();
        assert_eq!(hdr.geom_type, WKB_POINT);
        assert_eq!(hdr.srid, Some(4326));
        assert_eq!(hdr.data_offset, 9);
        assert!(!hdr.little_endian);
    }

    #[test]
    fn header_invalid_byte_order_marker() {
        assert!(parse_ewkb_header(&[0x02, 0x01, 0x00, 0x00, 0x00]).is_err());
    }

    #[test]
    fn header_srid_flag_but_truncated() {
        // byte order + type word with SRID flag, but no SRID bytes
        let mut blob = vec![0x01];
        let raw_type = WKB_POINT | EWKB_SRID_FLAG;
        blob.extend_from_slice(&raw_type.to_le_bytes());
        assert!(parse_ewkb_header(&blob).is_err());
    }

    #[test]
    fn header_valid_point_with_srid() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let hdr = parse_ewkb_header(&blob).unwrap();
        assert_eq!(hdr.geom_type, WKB_POINT);
        assert_eq!(hdr.srid, Some(4326));
        assert!(!hdr.has_z);
        assert!(!hdr.has_m);
        assert_eq!(hdr.data_offset, 9); // 1 + 4 + 4
    }

    #[test]
    fn header_valid_point_without_srid() {
        let blob = geom_from_text("POINT(1 2)", None).unwrap();
        let hdr = parse_ewkb_header(&blob).unwrap();
        assert_eq!(hdr.geom_type, WKB_POINT);
        assert_eq!(hdr.srid, None);
        assert_eq!(hdr.data_offset, 5); // 1 + 4
    }

    #[test]
    fn extract_srid_empty_blob() {
        assert_eq!(extract_srid(&[]), None);
    }

    #[test]
    fn extract_srid_malformed_blob() {
        assert_eq!(extract_srid(&[0xFF, 0xFF]), None);
    }

    #[test]
    fn write_ewkb_without_srid() {
        let geom = geo::Geometry::Point(geo::Point::new(1.0, 2.0));
        let blob = write_ewkb(&geom, None).unwrap();
        assert_eq!(extract_srid(&blob), None);
        // ISO WKB: byte order(1) + type(4) + x(8) + y(8) = 21 bytes
        assert_eq!(blob.len(), 21);
    }

    #[test]
    fn write_ewkb_with_srid() {
        let geom = geo::Geometry::Point(geo::Point::new(1.0, 2.0));
        let blob = write_ewkb(&geom, Some(4326)).unwrap();
        assert_eq!(extract_srid(&blob), Some(4326));
        // EWKB: byte order(1) + type(4) + srid(4) + x(8) + y(8) = 25 bytes
        assert_eq!(blob.len(), 25);
    }

    #[test]
    fn set_srid_replaces_existing() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let updated = set_srid(&blob, 3857).unwrap();
        assert_eq!(extract_srid(&updated), Some(3857));
        // Geometry should still parse correctly
        let (_, srid) = parse_ewkb(&updated).unwrap();
        assert_eq!(srid, Some(3857));
    }

    #[test]
    fn set_srid_adds_to_blob_without_srid() {
        let blob = geom_from_text("POINT(1 2)", None).unwrap();
        let updated = set_srid(&blob, 4326).unwrap();
        assert_eq!(extract_srid(&updated), Some(4326));
    }

    #[test]
    fn set_srid_rejects_truncated_point_payload() {
        // byte-order + Point type + only one coordinate (x), missing y
        let mut truncated = vec![0x01];
        truncated.extend_from_slice(&WKB_POINT.to_le_bytes());
        truncated.extend_from_slice(&1.0f64.to_le_bytes());

        set_srid(&truncated, 4326).expect_err("truncated payload must error");
    }

    #[test]
    fn set_srid_rejects_malformed_non_empty_payload() {
        // byte-order + LineString type + point count, but no coordinate payload
        let mut malformed = vec![0x01];
        malformed.extend_from_slice(&WKB_LINESTRING.to_le_bytes());
        malformed.extend_from_slice(&1u32.to_le_bytes());

        set_srid(&malformed, 3857).expect_err("malformed payload must error");
    }

    #[test]
    fn set_srid_allows_valid_empty_point_blob() {
        let empty = geom_from_text("POINT EMPTY", None).unwrap();
        let updated = set_srid(&empty, 4326).unwrap();

        let (geom, srid) = parse_ewkb(&updated).unwrap();
        assert_eq!(srid, Some(4326));
        match geom {
            Geometry::Point(p) => {
                assert!(p.x().is_nan());
                assert!(p.y().is_nan());
            }
            other => panic!("expected Point, got {other:?}"),
        }
    }

    #[test]
    fn geom_type_name_all_types() {
        assert_eq!(geom_type_name(WKB_POINT), "ST_Point");
        assert_eq!(geom_type_name(WKB_LINESTRING), "ST_LineString");
        assert_eq!(geom_type_name(WKB_POLYGON), "ST_Polygon");
        assert_eq!(geom_type_name(WKB_MULTIPOINT), "ST_MultiPoint");
        assert_eq!(geom_type_name(WKB_MULTILINESTRING), "ST_MultiLineString");
        assert_eq!(geom_type_name(WKB_MULTIPOLYGON), "ST_MultiPolygon");
        assert_eq!(
            geom_type_name(WKB_GEOMETRYCOLLECTION),
            "ST_GeometryCollection"
        );
        assert_eq!(geom_type_name(42), "ST_Unknown");
    }

    #[test]
    fn parse_ewkb_roundtrip() {
        let blob = geom_from_text("LINESTRING(0 0, 1 1, 2 2)", Some(4326)).unwrap();
        let (geom, srid) = parse_ewkb(&blob).unwrap();
        assert_eq!(srid, Some(4326));
        let blob2 = write_ewkb(&geom, srid).unwrap();
        let (geom2, srid2) = parse_ewkb(&blob2).unwrap();
        assert_eq!(srid, srid2);
        assert_eq!(format!("{geom:?}"), format!("{geom2:?}"));
    }

    #[test]
    fn parse_big_endian_ewkb_point() {
        let mut blob = vec![0x00];
        let typ = WKB_POINT | EWKB_SRID_FLAG;
        blob.extend_from_slice(&typ.to_be_bytes());
        blob.extend_from_slice(&4326i32.to_be_bytes());
        blob.extend_from_slice(&10.0f64.to_be_bytes());
        blob.extend_from_slice(&(-20.0f64).to_be_bytes());

        let (geom, srid) = parse_ewkb(&blob).unwrap();
        assert_eq!(srid, Some(4326));
        assert_eq!(
            geom,
            Geometry::Point(geo::Point::new(10.0, -20.0)),
            "big-endian EWKB should parse into XY geometry"
        );
    }

    #[test]
    fn parse_ewkb_with_zm_point_is_rejected() {
        let mut blob = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG | EWKB_M_FLAG;
        blob.extend_from_slice(&typ.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes()); // Z
        blob.extend_from_slice(&4.0f64.to_le_bytes()); // M

        let err = parse_ewkb(&blob).expect_err("Z/M payloads must not be flattened to XY");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    #[test]
    fn set_srid_preserves_big_endian_header_order() {
        let mut blob = vec![0x00];
        blob.extend_from_slice(&WKB_POINT.to_be_bytes());
        blob.extend_from_slice(&7.0f64.to_be_bytes());
        blob.extend_from_slice(&8.0f64.to_be_bytes());

        let updated = set_srid(&blob, 4326).unwrap();
        assert_eq!(updated[0], 0x00, "byte-order marker must stay big-endian");
        assert_eq!(extract_srid(&updated), Some(4326));

        let (geom, srid) = parse_ewkb(&updated).unwrap();
        assert_eq!(srid, Some(4326));
        assert_eq!(geom, Geometry::Point(geo::Point::new(7.0, 8.0)));
    }

    #[test]
    fn parse_ewkb_invalid_blob() {
        assert!(parse_ewkb(&[0x01, 0x02]).is_err());
    }

    #[test]
    fn ensure_matching_srid_accepts_equal() {
        assert_eq!(
            ensure_matching_srid(Some(4326), Some(4326)).unwrap(),
            Some(4326)
        );
        assert_eq!(ensure_matching_srid(None, None).unwrap(), None);
    }

    #[test]
    fn ensure_matching_srid_treats_unknown_and_zero_as_compatible() {
        assert_eq!(ensure_matching_srid(None, Some(0)).unwrap(), Some(0));
        assert_eq!(ensure_matching_srid(Some(0), None).unwrap(), Some(0));
    }

    #[test]
    fn ensure_matching_srid_rejects_mismatch() {
        assert!(ensure_matching_srid(Some(4326), Some(3857)).is_err());
        assert!(ensure_matching_srid(Some(4326), None).is_err());
    }

    #[test]
    fn parse_ewkb_pair_requires_matching_srid() {
        let a = crate::core::functions::io::geom_from_text("POINT(0 0)", Some(4326)).unwrap();
        let b = crate::core::functions::io::geom_from_text("POINT(1 1)", Some(4326)).unwrap();
        assert!(parse_ewkb_pair(&a, &b).is_ok());

        let mixed = crate::core::functions::io::geom_from_text("POINT(1 1)", Some(3857)).unwrap();
        assert!(parse_ewkb_pair(&a, &mixed).is_err());
    }

    #[test]
    fn parse_ewkb_pair_accepts_unknown_and_zero_srid() {
        let a = crate::core::functions::io::geom_from_text("POINT(0 0)", None).unwrap();
        let b = crate::core::functions::io::geom_from_text("POINT(1 1)", Some(0)).unwrap();
        let pair = parse_ewkb_pair(&a, &b).expect("None and SRID=0 should be compatible");
        assert_eq!(pair.2, Some(0));
    }

    #[test]
    fn parse_empty_point() {
        let blob =
            write_ewkb(&Geometry::Point(Point::new(f64::NAN, f64::NAN)), Some(4326)).unwrap();
        let (geom, srid) = parse_ewkb(&blob).unwrap();
        assert_eq!(srid, Some(4326));
        match geom {
            Geometry::Point(p) => {
                assert!(p.x().is_nan());
                assert!(p.y().is_nan());
            }
            other => panic!("expected point, got {other:?}"),
        }
        assert!(is_empty_point_blob(&blob).unwrap());
    }

    #[test]
    fn patch_wkb_with_srid_little_endian() {
        let mut iso = vec![0x01];
        iso.extend_from_slice(&WKB_POINT.to_le_bytes());
        iso.extend_from_slice(&1.0f64.to_le_bytes());
        iso.extend_from_slice(&2.0f64.to_le_bytes());

        let ewkb = patch_wkb_with_srid(&iso, 4326).unwrap();
        let hdr = parse_ewkb_header(&ewkb).unwrap();
        assert!(hdr.little_endian);
        assert_eq!(hdr.srid, Some(4326));
    }

    #[test]
    fn patch_wkb_with_srid_big_endian() {
        let mut iso = vec![0x00];
        iso.extend_from_slice(&WKB_POINT.to_be_bytes());
        iso.extend_from_slice(&1.0f64.to_be_bytes());
        iso.extend_from_slice(&2.0f64.to_be_bytes());

        let ewkb = patch_wkb_with_srid(&iso, 4326).unwrap();
        let hdr = parse_ewkb_header(&ewkb).unwrap();
        assert!(!hdr.little_endian);
        assert_eq!(hdr.srid, Some(4326));

        let (geom, srid) = parse_ewkb(&ewkb).unwrap();
        assert_eq!(srid, Some(4326));
        assert_eq!(geom, Geometry::Point(Point::new(1.0, 2.0)));
    }

    #[test]
    fn patch_wkb_with_srid_rejects_short_input() {
        assert!(patch_wkb_with_srid(&[0x01], 4326).is_err());
    }

    #[test]
    fn patch_wkb_with_srid_rejects_invalid_byte_order_marker() {
        let mut blob = vec![0x02];
        blob.extend_from_slice(&WKB_POINT.to_le_bytes());
        let err = patch_wkb_with_srid(&blob, 4326).expect_err("must reject 0x02");
        assert!(
            matches!(err, SqliteGisError::InvalidEwkb(ref s) if s.contains("byte order marker"))
        );
    }

    #[test]
    fn validate_ewkb_payload_accepts_valid_blob() {
        let blob = crate::core::functions::io::geom_from_text("LINESTRING(0 0,1 1)", Some(4326))
            .expect("valid EWKB");
        let header = validate_ewkb_payload(&blob).expect("valid payload");
        assert_eq!(header.geom_type, WKB_LINESTRING);
        assert_eq!(header.srid, Some(4326));
    }

    #[test]
    fn validate_ewkb_payload_rejects_malformed_non_empty_blob() {
        // byte-order + LineString type + point count, but no coordinate payload
        let mut malformed = vec![0x01];
        malformed.extend_from_slice(&WKB_LINESTRING.to_le_bytes());
        malformed.extend_from_slice(&1u32.to_le_bytes());

        validate_ewkb_payload(&malformed).expect_err("malformed payload must error");
    }

    #[test]
    fn validate_xy_ewkb_payload_rejects_zm_blob() {
        let mut blob = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG | EWKB_M_FLAG;
        blob.extend_from_slice(&typ.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes());
        blob.extend_from_slice(&4.0f64.to_le_bytes());

        let err = validate_xy_ewkb_payload(&blob).expect_err("Z/M payload must be rejected");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    // -----------------------------------------------------------------
    // extract_mbr coverage
    // -----------------------------------------------------------------

    /// Assert that `extract_mbr` agrees with the reference path
    /// (`parse_ewkb` + `geo::BoundingRect`) on `wkt`. Both should be
    /// `None` for empty geometries, or matching `Rect` for non-empty.
    fn assert_mbr_matches_reference(wkt: &str) {
        use geo::BoundingRect;

        let blob = geom_from_text(wkt, None).expect("seed blob from WKT");
        let fast = extract_mbr(&blob).expect("fast MBR path must succeed on valid blob");
        let (geom, _) = parse_ewkb(&blob).expect("reference parse must succeed");
        let reference = geom.bounding_rect();

        match (fast, reference) {
            (None, None) => {}
            (Some(f), Some(r)) => {
                assert!(
                    (f.min().x - r.min().x).abs() < 1e-12
                        && (f.min().y - r.min().y).abs() < 1e-12
                        && (f.max().x - r.max().x).abs() < 1e-12
                        && (f.max().y - r.max().y).abs() < 1e-12,
                    "MBR mismatch for {wkt:?}: fast={f:?}, reference={r:?}",
                );
            }
            other => panic!("MBR presence mismatch for {wkt:?}: {other:?}"),
        }
    }

    #[test]
    fn extract_mbr_point_matches_reference() {
        assert_mbr_matches_reference("POINT(1 2)");
        assert_mbr_matches_reference("POINT(-180 -90)");
        assert_mbr_matches_reference("POINT(180 90)");
    }

    #[test]
    fn extract_mbr_empty_point_returns_none() {
        let blob = geom_from_text("POINT EMPTY", None).expect("seed");
        assert!(extract_mbr(&blob).expect("ok").is_none());
    }

    #[test]
    fn extract_mbr_linestring_matches_reference() {
        assert_mbr_matches_reference("LINESTRING(0 0, 10 0, 10 5, 0 5)");
        assert_mbr_matches_reference("LINESTRING(-5 -10, 5 10)");
    }

    #[test]
    fn extract_mbr_polygon_with_hole_matches_reference() {
        // Outer ring + inner hole. The hole's vertices are within the outer
        // ring's bbox so the MBR is the outer ring's extent.
        assert_mbr_matches_reference(
            "POLYGON((0 0, 10 0, 10 10, 0 10, 0 0), (2 2, 4 2, 4 4, 2 4, 2 2))",
        );
    }

    #[test]
    fn extract_mbr_multipoint_matches_reference() {
        assert_mbr_matches_reference("MULTIPOINT((1 2), (5 5), (-3 4))");
    }

    #[test]
    fn extract_mbr_multilinestring_matches_reference() {
        assert_mbr_matches_reference("MULTILINESTRING((0 0, 1 1), (5 5, 6 7, -2 3))");
    }

    #[test]
    fn extract_mbr_multipolygon_matches_reference() {
        assert_mbr_matches_reference(
            "MULTIPOLYGON(((0 0, 1 0, 1 1, 0 1, 0 0)), ((10 10, 20 10, 20 20, 10 20, 10 10)))",
        );
    }

    #[test]
    fn extract_mbr_geometrycollection_matches_reference() {
        assert_mbr_matches_reference(
            "GEOMETRYCOLLECTION(POINT(1 2), LINESTRING(0 0, 5 5), POLYGON((0 0, 2 0, 2 2, 0 2, 0 0)))",
        );
    }

    #[test]
    fn extract_mbr_respects_big_endian_byte_order() {
        // Manually build a big-endian POINT(3 4) blob.
        let mut blob = vec![0x00];
        blob.extend_from_slice(&WKB_POINT.to_be_bytes());
        blob.extend_from_slice(&3.0f64.to_be_bytes());
        blob.extend_from_slice(&4.0f64.to_be_bytes());

        let mbr = extract_mbr(&blob).expect("ok").expect("non-empty");
        assert_eq!(mbr.min().x, 3.0);
        assert_eq!(mbr.min().y, 4.0);
        assert_eq!(mbr.max().x, 3.0);
        assert_eq!(mbr.max().y, 4.0);
    }

    #[test]
    fn extract_mbr_respects_srid_flag_offset() {
        // POINT(7 8) with SRID flag: header is 9 bytes (1 + 4 + 4), coords follow.
        let blob = geom_from_text("POINT(7 8)", Some(4326)).expect("seed");
        let mbr = extract_mbr(&blob).expect("ok").expect("non-empty");
        assert_eq!(mbr.min().x, 7.0);
        assert_eq!(mbr.max().y, 8.0);
    }

    #[test]
    fn extract_mbr_rejects_truncated_point_blob() {
        // Header says POINT but no coordinate bytes follow.
        let mut blob = vec![0x01];
        blob.extend_from_slice(&WKB_POINT.to_le_bytes());
        assert!(extract_mbr(&blob).is_err());
    }

    #[test]
    fn extract_mbr_rejects_truncated_polygon_blob() {
        // Polygon header says 1 ring with 4 points, but no coords follow.
        let mut blob = vec![0x01];
        blob.extend_from_slice(&WKB_POLYGON.to_le_bytes());
        blob.extend_from_slice(&1u32.to_le_bytes()); // nrings
        blob.extend_from_slice(&4u32.to_le_bytes()); // npoints
        assert!(extract_mbr(&blob).is_err());
    }
}
