//! Assertions shared by the function modules' unit tests.

/// Compare WKT against a PostGIS reading to 9 decimal places of each
/// coordinate, which is well under a millimetre in longitude and latitude
/// and far tighter than the difference between the earth models under test.
pub(crate) fn assert_wkt_close(got: &str, want: &str) {
    let nums = |s: &str| -> Vec<f64> {
        s.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == 'e'))
            .filter(|t| !t.is_empty())
            .filter_map(|t| t.parse::<f64>().ok())
            .collect()
    };
    let (g, w) = (nums(got), nums(want));
    assert_eq!(g.len(), w.len(), "shape differs: got {got}, want {want}");
    for (a, b) in g.iter().zip(w.iter()) {
        assert!(
            (a - b).abs() < 1e-9,
            "got {got}, want {want} (differ at {a} vs {b})"
        );
    }
}
