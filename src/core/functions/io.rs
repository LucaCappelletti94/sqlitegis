//! I/O and serialization functions.
//!
//! ST_AsText, ST_AsEWKT, ST_AsBinary, ST_AsEWKB, ST_AsGeoJSON,
//! ST_GeomFromText, ST_GeomFromWKB, ST_GeomFromEWKB, ST_GeomFromGeoJSON

use geo::{Geometry, Point};
use geozero::wkb::Ewkb;
use geozero::{CoordDimensions, ToGeo, ToJson, ToWkb, ToWkt};
use serde_json::Value;

use crate::core::error::{Result, SqliteGisError};
use crate::core::ewkb::{
    ensure_ewkb_nesting_ok, ensure_xy_only, extract_srid, is_empty_point_blob, parse_ewkb,
    parse_ewkb_header, validate_xy_ewkb_payload, write_ewkb, EWKB_M_FLAG, EWKB_Z_FLAG,
    MAX_NESTING_DEPTH, WKB_POINT,
};

const EMPTY_POINT_GEOJSON: &str = r#"{"type":"Point","coordinates":[]}"#;

fn is_empty_point_wkt(wkt: &str) -> bool {
    let mut parts = wkt.split_whitespace();
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(a), Some(b), None)
            if a.eq_ignore_ascii_case("POINT") && b.eq_ignore_ascii_case("EMPTY")
    )
}

fn is_geometrycollection_single_empty_point_wkt(wkt: &str) -> bool {
    let compact_upper = wkt
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect::<String>();
    compact_upper == "GEOMETRYCOLLECTION(POINTEMPTY)"
}

fn is_empty_point_geojson(json: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return false;
    };
    let Value::Object(obj) = value else {
        return false;
    };
    let is_point = obj.get("type").and_then(Value::as_str) == Some("Point");
    let is_empty_coords = matches!(
        obj.get("coordinates").and_then(Value::as_array),
        Some(coords) if coords.is_empty()
    );
    is_point && is_empty_coords
}

fn read_raw_wkb_type(wkb: &[u8]) -> Result<u32> {
    if wkb.len() < 5 {
        return Err(SqliteGisError::InvalidEwkb(format!(
            "blob too short: got {} bytes, need at least 5",
            wkb.len()
        )));
    }
    let little_endian = match wkb[0] {
        0x01 => true,
        0x00 => false,
        _ => {
            return Err(SqliteGisError::InvalidEwkb(
                "invalid byte order marker".to_string(),
            ))
        }
    };
    let raw_type = if little_endian {
        u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]])
    } else {
        u32::from_be_bytes([wkb[1], wkb[2], wkb[3], wkb[4]])
    };
    Ok(raw_type)
}

fn wkb_has_z_or_m(raw_type: u32) -> (bool, bool) {
    let has_ewkb_z = (raw_type & EWKB_Z_FLAG) != 0;
    let has_ewkb_m = (raw_type & EWKB_M_FLAG) != 0;
    if has_ewkb_z || has_ewkb_m {
        return (has_ewkb_z, has_ewkb_m);
    }

    // ISO WKB: base+1000 => Z, base+2000 => M, base+3000 => ZM.
    let dim_code = raw_type / 1000;
    match dim_code {
        1 => (true, false),
        2 => (false, true),
        3 => (true, true),
        _ => (false, false),
    }
}

// Deserialization helpers

/// Parse WKT (optionally with an SRID) into an EWKB blob.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::geom_from_text;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// assert!(!blob.is_empty());
/// ```
pub fn geom_from_text(wkt: &str, srid: Option<i32>) -> Result<Vec<u8>> {
    if is_empty_point_wkt(wkt) {
        return write_ewkb(&Geometry::Point(Point::new(f64::NAN, f64::NAN)), srid);
    }
    if is_geometrycollection_single_empty_point_wkt(wkt) {
        let gc = geo::GeometryCollection::new_from(vec![]);
        return write_ewkb(&Geometry::GeometryCollection(gc), srid);
    }
    ensure_wkt_nesting_ok(wkt)?;
    let geom: Geometry<f64> = geozero::wkt::Wkt(wkt.as_bytes()).to_geo()?;
    write_ewkb(&geom, srid)
}

/// Reject WKT whose parenthesis nesting exceeds [`MAX_NESTING_DEPTH`]. geozero's
/// WKT parser recurses per nested `GEOMETRYCOLLECTION` with no bound and
/// overflows the stack (an uncatchable abort) around depth 5000, so this
/// up-front cap is the only defense. Parenthesis depth is a conservative proxy.
// TODO(georust/wkt, no PR yet): root cause is unbounded recursion in the `wkt`
// crate's `GeometryCollection::from_tokens`. Revisit once fixed upstream.
fn ensure_wkt_nesting_ok(wkt: &str) -> Result<()> {
    let mut depth: usize = 0;
    for byte in wkt.bytes() {
        match byte {
            b'(' => {
                depth += 1;
                if depth > MAX_NESTING_DEPTH {
                    return Err(SqliteGisError::InvalidInput(format!(
                        "WKT nesting exceeds the maximum depth of {MAX_NESTING_DEPTH}"
                    )));
                }
            }
            b')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

/// Parse ISO WKB bytes (optionally override SRID) into an EWKB blob.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_binary, geom_from_wkb};
/// use sqlitegis::core::ewkb::extract_srid;
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// let wkb = as_binary(&blob).unwrap();
/// let restored = geom_from_wkb(&wkb, Some(4326)).unwrap();
/// assert_eq!(extract_srid(&restored), Some(4326));
/// ```
pub fn geom_from_wkb(wkb: &[u8], srid: Option<i32>) -> Result<Vec<u8>> {
    let raw_type = read_raw_wkb_type(wkb)?;
    let (has_z, has_m) = wkb_has_z_or_m(raw_type);
    ensure_xy_only(has_z, has_m)?;
    if is_empty_point_blob(wkb)? {
        return write_ewkb(&Geometry::Point(Point::new(f64::NAN, f64::NAN)), srid);
    }
    ensure_ewkb_nesting_ok(wkb)?;
    let geom: Geometry<f64> = Ewkb(wkb).to_geo()?;
    write_ewkb(&geom, srid)
}

/// Validate and pass through an EWKB blob without rewriting bytes.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, geom_from_ewkb};
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let passthrough = geom_from_ewkb(&blob).unwrap();
/// assert_eq!(blob, passthrough);
/// ```
pub fn geom_from_ewkb(ewkb: &[u8]) -> Result<Vec<u8>> {
    let _ = validate_xy_ewkb_payload(ewkb)?;
    Ok(ewkb.to_vec())
}

/// Parse a GeoJSON string into an EWKB blob (SRID = 4326 by default, per spec).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::geom_from_geojson;
/// use sqlitegis::core::ewkb::extract_srid;
///
/// let blob = geom_from_geojson(r#"{"type":"Point","coordinates":[1,2]}"#, None).unwrap();
/// assert_eq!(extract_srid(&blob), Some(4326));
/// ```
pub fn geom_from_geojson(json: &str, srid: Option<i32>) -> Result<Vec<u8>> {
    let effective_srid = srid.or(Some(4326));
    match geozero::geojson::GeoJson(json).to_geo() {
        Ok(geom) => write_ewkb(&geom, effective_srid),
        Err(_) if is_empty_point_geojson(json) => write_ewkb(
            &Geometry::Point(Point::new(f64::NAN, f64::NAN)),
            effective_srid,
        ),
        Err(e) => Err(SqliteGisError::Geozero(e)),
    }
}

// Serialization helpers

/// Convert an EWKB blob to WKT text.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_text};
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// let wkt = as_text(&blob).unwrap();
/// assert!(wkt.contains("POINT"));
/// ```
pub fn as_text(blob: &[u8]) -> Result<String> {
    if is_empty_point_blob(blob)? {
        return Ok("POINT EMPTY".to_string());
    }
    ensure_ewkb_nesting_ok(blob)?;
    Ok(Ewkb(blob).to_wkt()?)
}

/// Convert an EWKB blob to EWKT text (`SRID=n;WKT`).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_ewkt};
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let ewkt = as_ewkt(&blob).unwrap();
/// assert!(ewkt.starts_with("SRID=4326;"));
/// ```
pub fn as_ewkt(blob: &[u8]) -> Result<String> {
    let srid = extract_srid(blob);
    if is_empty_point_blob(blob)? {
        if let Some(s) = srid {
            return Ok(format!("SRID={s};POINT EMPTY"));
        }
        return Ok("POINT EMPTY".to_string());
    }
    ensure_ewkb_nesting_ok(blob)?;
    let wkt = Ewkb(blob).to_wkt()?;
    if let Some(s) = srid {
        Ok(format!("SRID={s};{wkt}"))
    } else {
        Ok(wkt)
    }
}

/// Convert an EWKB blob to ISO WKB bytes (strips SRID).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_binary};
/// use sqlitegis::core::ewkb::extract_srid;
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let wkb = as_binary(&blob).unwrap();
/// // ISO WKB has no SRID
/// assert_eq!(extract_srid(&wkb), None);
/// ```
pub fn as_binary(blob: &[u8]) -> Result<Vec<u8>> {
    let header = parse_ewkb_header(blob)?;
    ensure_xy_only(header.has_z, header.has_m)?;
    if is_empty_point_blob(blob)? {
        let mut out = Vec::with_capacity(21);
        if header.little_endian {
            out.push(0x01);
            out.extend_from_slice(&WKB_POINT.to_le_bytes());
            out.extend_from_slice(&f64::NAN.to_le_bytes());
            out.extend_from_slice(&f64::NAN.to_le_bytes());
        } else {
            out.push(0x00);
            out.extend_from_slice(&WKB_POINT.to_be_bytes());
            out.extend_from_slice(&f64::NAN.to_be_bytes());
            out.extend_from_slice(&f64::NAN.to_be_bytes());
        }
        return Ok(out);
    }
    let (geom, _srid) = parse_ewkb(blob)?;
    geom.to_wkb(CoordDimensions::xy())
        .map_err(SqliteGisError::Geozero)
}

/// Return the EWKB blob as-is (identity for well-formed input).
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_ewkb};
///
/// let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
/// let copy = as_ewkb(&blob).unwrap();
/// assert_eq!(blob.len(), copy.len());
/// ```
pub fn as_ewkb(blob: &[u8]) -> Result<Vec<u8>> {
    geom_from_ewkb(blob)
}

/// Convert an EWKB blob to GeoJSON text.
///
/// # Example
///
/// ```
/// use sqlitegis::core::functions::io::{geom_from_text, as_geojson};
///
/// let blob = geom_from_text("POINT(1 2)", None).unwrap();
/// let json = as_geojson(&blob).unwrap();
/// assert!(json.contains("Point"));
/// assert!(json.contains("coordinates"));
/// ```
pub fn as_geojson(blob: &[u8]) -> Result<String> {
    if is_empty_point_blob(blob)? {
        return Ok(EMPTY_POINT_GEOJSON.to_string());
    }
    ensure_ewkb_nesting_ok(blob)?;
    Ok(Ewkb(blob).to_json()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geom_from_text_rejects_deeply_nested_wkt() {
        // geozero's WKT parser recurses per nested GeometryCollection with no
        // bound and overflows the stack. A crafted deep string must be rejected
        // with an error rather than aborting the process.
        let mut wkt = String::new();
        for _ in 0..10_000 {
            wkt.push_str("GEOMETRYCOLLECTION(");
        }
        wkt.push_str("POINT(1 2)");
        for _ in 0..10_000 {
            wkt.push(')');
        }
        assert!(matches!(
            geom_from_text(&wkt, None),
            Err(SqliteGisError::InvalidInput(_))
        ));

        // A shallow GeometryCollection still parses.
        let ok = geom_from_text("GEOMETRYCOLLECTION(POINT(1 2))", None);
        assert!(ok.is_ok());
    }

    #[test]
    fn geom_from_wkb_rejects_iso_wkb_z_point() {
        // Type 1001 = POINT Z in ISO WKB (little-endian: base 1 + 1000)
        let mut wkb = vec![0x01u8];
        wkb.extend_from_slice(&1001u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes()); // x
        wkb.extend_from_slice(&2.0f64.to_le_bytes()); // y
        wkb.extend_from_slice(&3.0f64.to_le_bytes()); // z
        let err = geom_from_wkb(&wkb, None).unwrap_err();
        assert!(
            err.to_string().contains("unsupported"),
            "expected unsupported error, got: {err}"
        );
    }

    #[test]
    fn geom_from_wkb_rejects_iso_wkb_m_point() {
        // Type 2001 = POINT M in ISO WKB
        let mut wkb = vec![0x01u8];
        wkb.extend_from_slice(&2001u32.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        assert!(geom_from_wkb(&wkb, None).is_err());
    }

    #[test]
    fn geom_from_wkb_rejects_iso_wkb_zm_point() {
        // Type 3001 = POINT ZM in ISO WKB
        let mut wkb = vec![0x01u8];
        wkb.extend_from_slice(&3001u32.to_le_bytes());
        for _ in 0..4 {
            wkb.extend_from_slice(&1.0f64.to_le_bytes());
        }
        assert!(geom_from_wkb(&wkb, None).is_err());
    }

    #[test]
    fn invalid_wkt_returns_err() {
        assert!(geom_from_text("NOT_VALID_WKT", None).is_err());
    }

    #[test]
    fn invalid_wkb_returns_err() {
        assert!(geom_from_wkb(&[0xFF, 0x00], None).is_err());
    }

    #[test]
    fn geom_from_wkb_rejects_z_and_m_dimensions() {
        let mut wkb = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG | EWKB_M_FLAG;
        wkb.extend_from_slice(&typ.to_le_bytes());
        wkb.extend_from_slice(&1.0f64.to_le_bytes());
        wkb.extend_from_slice(&2.0f64.to_le_bytes());
        wkb.extend_from_slice(&3.0f64.to_le_bytes());
        wkb.extend_from_slice(&4.0f64.to_le_bytes());
        assert!(geom_from_wkb(&wkb, None).is_err());
    }

    #[test]
    fn invalid_geojson_returns_err() {
        assert!(geom_from_geojson("{not json}", None).is_err());
    }

    #[test]
    fn geojson_default_srid_4326() {
        let blob = geom_from_geojson(r#"{"type":"Point","coordinates":[1,2]}"#, None).unwrap();
        assert_eq!(extract_srid(&blob), Some(4326));
    }

    #[test]
    fn geojson_custom_srid_overrides() {
        let blob =
            geom_from_geojson(r#"{"type":"Point","coordinates":[1,2]}"#, Some(3857)).unwrap();
        assert_eq!(extract_srid(&blob), Some(3857));
    }

    #[test]
    fn as_ewkt_with_srid() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let ewkt = as_ewkt(&blob).unwrap();
        assert!(ewkt.starts_with("SRID=4326;"));
        assert!(ewkt.contains("POINT"));
    }

    #[test]
    fn as_ewkt_without_srid() {
        let blob = geom_from_text("POINT(1 2)", None).unwrap();
        let ewkt = as_ewkt(&blob).unwrap();
        assert!(!ewkt.contains("SRID="));
    }

    #[test]
    fn as_binary_strips_srid() {
        let blob = geom_from_text("POINT(1 2)", Some(4326)).unwrap();
        let wkb = as_binary(&blob).unwrap();
        assert_eq!(extract_srid(&wkb), None);
    }

    #[test]
    fn as_binary_rejects_z_and_m_dimensions() {
        let mut ewkb = vec![0x01];
        let typ = WKB_POINT | EWKB_Z_FLAG | EWKB_M_FLAG;
        ewkb.extend_from_slice(&typ.to_le_bytes());
        ewkb.extend_from_slice(&1.0f64.to_le_bytes());
        ewkb.extend_from_slice(&2.0f64.to_le_bytes());
        ewkb.extend_from_slice(&3.0f64.to_le_bytes());
        ewkb.extend_from_slice(&4.0f64.to_le_bytes());
        assert!(as_binary(&ewkb).is_err());
    }

    #[test]
    fn roundtrip_wkb() {
        let blob = geom_from_text("POINT(3 4)", Some(4326)).unwrap();
        let wkb = as_binary(&blob).unwrap();
        let restored = geom_from_wkb(&wkb, Some(4326)).unwrap();
        let (g1, _) = parse_ewkb(&blob).unwrap();
        let (g2, _) = parse_ewkb(&restored).unwrap();
        assert_eq!(format!("{g1:?}"), format!("{g2:?}"));
    }

    #[test]
    fn roundtrip_geojson() {
        let blob = geom_from_text("POINT(1 2)", None).unwrap();
        let json = as_geojson(&blob).unwrap();
        let restored = geom_from_geojson(&json, None).unwrap();
        let (g1, _) = parse_ewkb(&blob).unwrap();
        let (g2, _) = parse_ewkb(&restored).unwrap();
        assert_eq!(format!("{g1:?}"), format!("{g2:?}"));
    }

    #[test]
    fn point_empty_geojson_is_postgis_compatible() {
        let blob = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        let json = as_geojson(&blob).unwrap();
        assert_eq!(json, EMPTY_POINT_GEOJSON);

        let restored = geom_from_geojson(&json, None).unwrap();
        assert_eq!(as_text(&restored).unwrap(), "POINT EMPTY");
        assert_eq!(extract_srid(&restored), Some(4326));
    }

    #[test]
    fn geom_from_geojson_accepts_point_empty_coordinates_array() {
        let blob = geom_from_geojson(EMPTY_POINT_GEOJSON, Some(3857)).unwrap();
        assert_eq!(as_text(&blob).unwrap(), "POINT EMPTY");
        assert_eq!(extract_srid(&blob), Some(3857));
    }

    #[test]
    fn as_text_roundtrip() {
        let blob = geom_from_text("LINESTRING(0 0,1 1,2 2)", None).unwrap();
        let wkt = as_text(&blob).unwrap();
        assert!(wkt.contains("LINESTRING"));
    }

    #[test]
    fn geom_from_ewkb_rejects_z_dimension_payload() {
        let mut blob = vec![0x01];
        let typ = crate::core::ewkb::EWKB_Z_FLAG | crate::core::ewkb::WKB_POINT;
        blob.extend_from_slice(&typ.to_le_bytes());
        blob.extend_from_slice(&1.0f64.to_le_bytes());
        blob.extend_from_slice(&2.0f64.to_le_bytes());
        blob.extend_from_slice(&3.0f64.to_le_bytes());
        let err = geom_from_ewkb(&blob).expect_err("Z payload must be rejected");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    #[test]
    fn geom_from_ewkb_rejects_big_endian_zm_payload() {
        let mut blob = vec![0x00];
        let typ = crate::core::ewkb::EWKB_Z_FLAG
            | crate::core::ewkb::EWKB_M_FLAG
            | crate::core::ewkb::WKB_POINT;
        blob.extend_from_slice(&typ.to_be_bytes());
        blob.extend_from_slice(&1.0f64.to_be_bytes());
        blob.extend_from_slice(&2.0f64.to_be_bytes());
        blob.extend_from_slice(&3.0f64.to_be_bytes());
        blob.extend_from_slice(&4.0f64.to_be_bytes());

        let err = geom_from_ewkb(&blob).expect_err("ZM payload must be rejected");
        assert!(format!("{err}").contains("unsupported coordinate dimensions"));
    }

    #[test]
    fn geom_from_ewkb_and_as_ewkb_roundtrip() {
        let blob = geom_from_text("LINESTRING(0 0,1 1)", Some(4326)).unwrap();
        let normalized = geom_from_ewkb(&blob).unwrap();
        let copied = as_ewkb(&normalized).unwrap();

        let (g1, srid1) = parse_ewkb(&blob).unwrap();
        let (g2, srid2) = parse_ewkb(&copied).unwrap();
        assert_eq!(format!("{g1:?}"), format!("{g2:?}"));
        assert_eq!(srid1, srid2);
    }

    #[test]
    fn point_empty_is_supported_in_wkt_and_text_outputs() {
        let blob = geom_from_text("POINT EMPTY", Some(4326)).unwrap();
        assert_eq!(as_text(&blob).unwrap(), "POINT EMPTY");
        assert_eq!(as_ewkt(&blob).unwrap(), "SRID=4326;POINT EMPTY");
        assert!(crate::core::functions::accessors::st_is_empty(&blob).unwrap());
        assert_eq!(
            crate::core::functions::accessors::st_geometry_type(&blob).unwrap(),
            "ST_Point"
        );
    }

    #[test]
    fn point_empty_binary_roundtrip() {
        let blob = geom_from_text("POINT EMPTY", None).unwrap();
        let wkb = as_binary(&blob).unwrap();
        let restored = geom_from_wkb(&wkb, None).unwrap();
        assert_eq!(as_text(&restored).unwrap(), "POINT EMPTY");
    }

    #[test]
    fn geom_from_text_accepts_geometrycollection_with_empty_point() {
        let blob = geom_from_text("GEOMETRYCOLLECTION(POINT EMPTY)", Some(4326)).unwrap();
        assert_eq!(extract_srid(&blob), Some(4326));
        assert_eq!(
            crate::core::functions::accessors::st_num_geometries(&blob).unwrap(),
            0
        );
        assert!(crate::core::functions::accessors::st_is_empty(&blob).unwrap());
        assert_eq!(
            crate::core::functions::accessors::st_npoints(&blob).unwrap(),
            0
        );
        assert_eq!(as_text(&blob).unwrap(), "GEOMETRYCOLLECTION EMPTY");
    }

    #[test]
    fn geom_from_wkb_rejects_invalid_byte_order_marker() {
        let wkb = [0x02u8, 0, 0, 0, 0];
        let err = geom_from_wkb(&wkb, None).expect_err("must reject 0x02 byte order");
        assert!(
            matches!(err, SqliteGisError::InvalidEwkb(ref s) if s.contains("byte order marker"))
        );
    }

    #[test]
    fn geom_from_wkb_accepts_big_endian_input() {
        // Big-endian ISO WKB POINT(1 2). Exercises the 0x00 byte-order branch
        // and the from_be_bytes path inside `read_raw_wkb_type`.
        let mut wkb = vec![0x00u8];
        wkb.extend_from_slice(&WKB_POINT.to_be_bytes());
        wkb.extend_from_slice(&1.0f64.to_be_bytes());
        wkb.extend_from_slice(&2.0f64.to_be_bytes());
        let blob = geom_from_wkb(&wkb, Some(4326)).expect("BE POINT(1 2) must parse");
        assert_eq!(extract_srid(&blob), Some(4326));
        assert_eq!(as_text(&blob).unwrap(), "POINT(1 2)");
    }

    #[test]
    fn geom_from_geojson_rejects_non_object_json() {
        // Valid JSON, but not an Object. Exercises the `Value::Object` else-arm
        // inside `is_empty_point_geojson`.
        let err = geom_from_geojson("[1,2,3]", None).expect_err("array json must error");
        assert!(matches!(err, SqliteGisError::Geozero(_)));
    }

    #[test]
    fn as_ewkt_returns_plain_point_empty_for_no_srid_input() {
        let blob = geom_from_text("POINT EMPTY", None).unwrap();
        assert_eq!(as_ewkt(&blob).unwrap(), "POINT EMPTY");
    }

    #[test]
    fn as_binary_emits_be_point_empty_for_be_input() {
        // Hand-built big-endian EWKB POINT EMPTY: byte-order 0x00, type WKB_POINT
        // big-endian, two NaN big-endian f64s.
        let mut blob = vec![0x00u8];
        blob.extend_from_slice(&WKB_POINT.to_be_bytes());
        blob.extend_from_slice(&f64::NAN.to_be_bytes());
        blob.extend_from_slice(&f64::NAN.to_be_bytes());

        let out = as_binary(&blob).expect("be POINT EMPTY round-trips");
        assert_eq!(out.len(), 21);
        assert_eq!(out[0], 0x00);
        assert_eq!(&out[1..5], &WKB_POINT.to_be_bytes());
        assert!(f64::from_be_bytes(out[5..13].try_into().unwrap()).is_nan());
        assert!(f64::from_be_bytes(out[13..21].try_into().unwrap()).is_nan());
    }
}
