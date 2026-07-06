#![no_main]

use libfuzzer_sys::fuzz_target;

use sqlitegis::core::error::Result;
use sqlitegis::core::ewkb::{parse_ewkb_pair, write_ewkb};
use sqlitegis::core::functions::operations::st_sym_difference;

#[path = "common.rs"]
mod common;

use common::{assert_parity, Pair};

/// Reference path that never takes the bytes-only fastpath: always
/// decode both inputs and run the `geo::BooleanOps::xor` algebra.
fn reference_sym_difference(a: &[u8], b: &[u8]) -> Result<Vec<u8>> {
    use geo::algorithm::bool_ops::BooleanOps;
    use geo::{Geometry, MultiPolygon};
    use sqlitegis::SqliteGisError;

    let (ga, gb, srid) = parse_ewkb_pair(a, b)?;
    let ma = match ga {
        Geometry::Polygon(p) => MultiPolygon::new(vec![p]),
        Geometry::MultiPolygon(mp) => mp,
        other => {
            return Err(SqliteGisError::wrong_type(
                "Polygon or MultiPolygon",
                &other,
            ))
        }
    };
    let mb = match gb {
        Geometry::Polygon(p) => MultiPolygon::new(vec![p]),
        Geometry::MultiPolygon(mp) => mp,
        other => {
            return Err(SqliteGisError::wrong_type(
                "Polygon or MultiPolygon",
                &other,
            ))
        }
    };
    let result = ma.xor(&mb);
    write_ewkb(&Geometry::MultiPolygon(result), srid)
}

fuzz_target!(|pair: Pair| {
    let Some((a, b)) = pair.build() else {
        return;
    };
    assert_parity(&a, &b, st_sym_difference, reference_sym_difference);
});
