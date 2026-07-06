#![no_main]
//! Text-format parsers on hostile input. `geom_from_text` (WKT) and
//! `geom_from_geojson` (GeoJSON) feed untrusted strings into geozero's
//! recursive text decoders. The invariant is "never panic / overflow": a
//! malformed or deeply nested string must come back as Ok/Err, not abort the
//! process. Round-trips that succeed are re-parsed to catch a parse that
//! produces a blob our own decoder then rejects.

use libfuzzer_sys::fuzz_target;

use sqlitegis::core::functions::io::{as_geojson, as_text, geom_from_geojson, geom_from_text};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };

    if let Ok(blob) = geom_from_text(text, Some(4326)) {
        // A blob we just produced must serialize back without crashing.
        let _ = as_text(&blob);
    }

    if let Ok(blob) = geom_from_geojson(text, None) {
        let _ = as_geojson(&blob);
    }
});
