//! Free-function helpers that build R-tree-prefiltered spatial queries.
//!
//! SQLite's query planner cannot rewrite a scalar predicate like
//! `ST_DWithinSphere(geom, ST_Point(...), r)` into a virtual-index plan the
//! way PostgreSQL's GIST operator classes do for PostGIS. The shadow
//! `*_geom_rtree` virtual table has to be JOINed explicitly. These helpers
//! emit that JOIN with the right bounding-box math so callers do not have
//! to type the boilerplate every time.
//!
//! See [`crate::diesel::query_patterns`] Pattern 4 for the prose
//! explanation of the two-stage prefilter+refinement technique.
//!
//! ```
//! use diesel::{Connection, RunQueryDsl, sqlite::SqliteConnection};
//! use diesel::deserialize::QueryableByName;
//! use diesel::sql_types::BigInt;
//! use sqlitegis::diesel::query_helpers::dwithin_sphere_indexed_sql;
//!
//! #[derive(QueryableByName)]
//! struct Hit {
//!     #[diesel(sql_type = BigInt)]
//!     id: i64,
//! }
//!
//! sqlitegis::sqlite::register_on_every_new_connection();
//! let mut c = SqliteConnection::establish(":memory:").unwrap();
//!
//! // Tiny indexed table with one row in Berlin.
//! diesel::sql_query("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)")
//!     .execute(&mut c).unwrap();
//! diesel::sql_query("SELECT CreateSpatialIndex('pts', 'geom')")
//!     .execute(&mut c).unwrap();
//! diesel::sql_query(
//!     "INSERT INTO pts(id, geom) VALUES (1, ST_Point(13.4, 52.5, 4326))",
//! ).execute(&mut c).unwrap();
//!
//! let hits: Vec<Hit> = dwithin_sphere_indexed_sql(
//!     "pts", "geom", (13.4, 52.5), 100_000.0, "t.id",
//! ).load::<Hit>(&mut c).unwrap();
//! assert_eq!(hits.len(), 1);
//! assert_eq!(hits[0].id, 1);
//! ```

/// Conservative degree offsets that enclose a geodesic circle.
///
/// `dlat` is constant (one degree of latitude is ~111.32 km everywhere on
/// the WGS84 ellipsoid). `dlon` scales by `1 / cos(lat)` because one
/// degree of longitude shrinks toward the poles.
///
/// At lat 60 degrees `dlon` is roughly twice the equator value. Near the
/// poles `dlon` would diverge, so [`radius_bbox`] clamps it to 180.0 (the
/// entire longitude range).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RadiusBbox {
    /// Half-width of the bounding box in degrees of longitude.
    pub dlon: f64,
    /// Half-height of the bounding box in degrees of latitude.
    pub dlat: f64,
}

/// Approximate metres per degree of latitude on the WGS84 ellipsoid.
const METRES_PER_DEGREE: f64 = 111_320.0;

/// Compute the degree-offset bounding box for a geodesic radius search.
///
/// `lat_deg` is the latitude of the probe point in WGS84 degrees,
/// `radius_m` is the search radius in metres. The result is a [`RadiusBbox`]
/// that is *guaranteed* to enclose every point within `radius_m` of the
/// probe (it may include extra points, so refine with `ST_DWithinSphere`).
///
/// Worked numbers for a 1000 km radius:
///
/// | latitude | `dlon`  | `dlat` |
/// | -------: | ------: | -----: |
/// | 0°       | 8.98°   | 8.98°  |
/// | 45°      | 12.7°   | 8.98°  |
/// | 60°      | 17.96°  | 8.98°  |
/// | 80°      | 51.7°   | 8.98°  |
/// | 89°      | 180.0°  | 8.98°  |
///
/// The clamp at 180° applies whenever `cos(lat)` falls below 1e-6, which
/// happens within roughly 0.00006° of the pole.
///
/// ```rust
/// use sqlitegis::diesel::query_helpers::radius_bbox;
///
/// let equator = radius_bbox(0.0, 1_000_000.0);
/// let berlin = radius_bbox(52.5, 1_000_000.0);
/// // dlat is constant. dlon grows with |lat| because longitude shrinks.
/// assert!((equator.dlat - berlin.dlat).abs() < 1e-9);
/// assert!(berlin.dlon > equator.dlon);
///
/// // Near the pole dlon saturates at 180 rather than diverging.
/// assert_eq!(radius_bbox(89.9999, 1_000_000.0).dlon, 180.0);
/// ```
pub fn radius_bbox(lat_deg: f64, radius_m: f64) -> RadiusBbox {
    let dlat = radius_m / METRES_PER_DEGREE;
    let cos_lat = lat_deg.to_radians().cos().abs().max(1.0e-6);
    let dlon = (radius_m / (METRES_PER_DEGREE * cos_lat)).min(180.0);
    RadiusBbox { dlon, dlat }
}

/// Build a [`diesel::sql_query`] that runs a radius search through the
/// R-tree shadow table.
///
/// The query is the standard two-stage pattern: an R-tree bounding-box
/// prefilter narrows candidates to `O(log N + k)` rows, then
/// `ST_DWithinSphere` refines to the exact geodesic circle.
///
/// `table` and `geom_column` are interpolated into the SQL inside `[...]`
/// brackets (so reserved words and column names with spaces still parse).
/// They are *not* bound parameters: callers must pass trusted identifiers,
/// the same contract `CreateSpatialIndex` already imposes. All numeric
/// values (`probe`, `radius_m`, bbox bounds) are formatted into the SQL as
/// `f64` literals, which is injection-safe.
///
/// `select_cols` is the projection list to splice between `SELECT` and
/// `FROM`. Reference base-table columns as `t.<col>` (the table is aliased
/// `t`), and the R-tree side as `r.<col>` if needed.
///
/// # Panics
///
/// Never panics on its own. The returned `SqlQuery` may error at
/// `.load()` time if the named table or its shadow R-tree do not exist.
///
/// # Example
///
/// ```
/// use diesel::{Connection, RunQueryDsl, sqlite::SqliteConnection};
/// use diesel::deserialize::QueryableByName;
/// use diesel::sql_types::BigInt;
/// use sqlitegis::diesel::query_helpers::dwithin_sphere_indexed_sql;
///
/// #[derive(QueryableByName)]
/// struct Hit { #[diesel(sql_type = BigInt)] id: i64 }
///
/// sqlitegis::sqlite::register_on_every_new_connection();
/// let mut c = SqliteConnection::establish(":memory:").unwrap();
///
/// diesel::sql_query("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)")
///     .execute(&mut c).unwrap();
/// diesel::sql_query("SELECT CreateSpatialIndex('pts', 'geom')")
///     .execute(&mut c).unwrap();
/// diesel::sql_query(
///     "INSERT INTO pts(id, geom) VALUES (1, ST_Point(13.4, 52.5, 4326))",
/// ).execute(&mut c).unwrap();
///
/// // Probe within 100 km of Berlin.
/// let hits: Vec<Hit> = dwithin_sphere_indexed_sql(
///     "pts", "geom", (13.4, 52.5), 100_000.0, "t.id",
/// ).load::<Hit>(&mut c).unwrap();
/// assert_eq!(hits.len(), 1);
/// ```
pub fn dwithin_sphere_indexed_sql(
    table: &str,
    geom_column: &str,
    probe: (f64, f64),
    radius_m: f64,
    select_cols: &str,
) -> diesel::query_builder::SqlQuery {
    diesel::sql_query(dwithin_sphere_indexed_sql_string(
        table,
        geom_column,
        probe,
        radius_m,
        select_cols,
    ))
}

/// Render the SQL string that [`dwithin_sphere_indexed_sql`] wraps.
///
/// Same inputs and contract, useful when the caller needs the raw SQL
/// (for logging, for prepending `EXPLAIN QUERY PLAN`, or for piping it
/// through `diesel::sql_query` together with extra binds).
///
/// ```rust
/// use sqlitegis::diesel::query_helpers::dwithin_sphere_indexed_sql_string;
///
/// let sql = dwithin_sphere_indexed_sql_string(
///     "places", "geom", (13.4, 52.5), 1_000_000.0, "t.id, t.name",
/// );
/// assert!(sql.contains("JOIN [places_geom_rtree]"));
/// assert!(sql.contains("ST_DWithinSphere"));
/// ```
pub fn dwithin_sphere_indexed_sql_string(
    table: &str,
    geom_column: &str,
    probe: (f64, f64),
    radius_m: f64,
    select_cols: &str,
) -> String {
    let (lon, lat) = probe;
    let bbox = radius_bbox(lat, radius_m);
    format!(
        "SELECT {select_cols} \
         FROM [{table}] t \
         JOIN [{table}_{geom_column}_rtree] r ON t.rowid = r.id \
         WHERE r.xmax >= {x_min} AND r.xmin <= {x_max} \
           AND r.ymax >= {y_min} AND r.ymin <= {y_max} \
           AND ST_DWithinSphere(t.[{geom_column}], \
                                ST_Point({lon}, {lat}, 4326), {radius_m})",
        x_min = lon - bbox.dlon,
        x_max = lon + bbox.dlon,
        y_min = lat - bbox.dlat,
        y_max = lat + bbox.dlat,
    )
}

/// Build a [`diesel::sql_query`] that runs an envelope-window search
/// through the R-tree shadow table.
///
/// The query is the standard two-stage pattern: an R-tree bounding-box
/// JOIN narrows candidates whose stored bbox overlaps the window, then
/// `ST_Intersects` against `ST_MakeEnvelope(...)` refines to the exact
/// intersection. For point-only datasets the refinement is a no-op (a
/// point's bbox is the point itself), but it keeps the helper correct
/// for arbitrary geometry types (e.g. L-shaped polygons whose bbox
/// overlaps the window but whose geometry does not).
///
/// `window` is `(xmin, ymin, xmax, ymax)` in the same CRS the geometry
/// column uses (typically WGS84 degrees, SRID 4326).
///
/// `table` and `geom_column` follow the same identifier-safety contract
/// as [`dwithin_sphere_indexed_sql`]: bracketed into the SQL, must be
/// caller-trusted strings. Numeric inputs are formatted as `f64`
/// literals, which is injection-safe.
///
/// # Example
///
/// ```
/// use diesel::{Connection, RunQueryDsl, sqlite::SqliteConnection};
/// use diesel::deserialize::QueryableByName;
/// use diesel::sql_types::BigInt;
/// use sqlitegis::diesel::query_helpers::intersects_window_indexed_sql;
///
/// #[derive(QueryableByName)]
/// struct Hit { #[diesel(sql_type = BigInt)] id: i64 }
///
/// sqlitegis::sqlite::register_on_every_new_connection();
/// let mut c = SqliteConnection::establish(":memory:").unwrap();
///
/// diesel::sql_query("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)")
///     .execute(&mut c).unwrap();
/// diesel::sql_query("SELECT CreateSpatialIndex('pts', 'geom')")
///     .execute(&mut c).unwrap();
/// diesel::sql_query(
///     "INSERT INTO pts(id, geom) VALUES (1, ST_Point(13.4, 52.5, 4326))",
/// ).execute(&mut c).unwrap();
///
/// // 30 by 30 degree window around (13.4, 52.5).
/// let hits: Vec<Hit> = intersects_window_indexed_sql(
///     "pts", "geom", (-1.6, 37.5, 28.4, 67.5), "t.id",
/// ).load::<Hit>(&mut c).unwrap();
/// assert_eq!(hits.len(), 1);
/// ```
pub fn intersects_window_indexed_sql(
    table: &str,
    geom_column: &str,
    window: (f64, f64, f64, f64),
    select_cols: &str,
) -> diesel::query_builder::SqlQuery {
    diesel::sql_query(intersects_window_indexed_sql_string(
        table,
        geom_column,
        window,
        select_cols,
    ))
}

/// Render the SQL string that [`intersects_window_indexed_sql`] wraps.
///
/// Same inputs and contract, useful when the caller needs the raw SQL
/// (for logging, for prepending `EXPLAIN QUERY PLAN`, or for piping it
/// through `diesel::sql_query` together with extra binds).
///
/// ```rust
/// use sqlitegis::diesel::query_helpers::intersects_window_indexed_sql_string;
///
/// let sql = intersects_window_indexed_sql_string(
///     "places", "geom", (-1.6, 37.5, 28.4, 67.5), "t.id, t.name",
/// );
/// assert!(sql.contains("JOIN [places_geom_rtree]"));
/// assert!(sql.contains("ST_MakeEnvelope(-1.6, 37.5, 28.4, 67.5, 4326)"));
/// ```
pub fn intersects_window_indexed_sql_string(
    table: &str,
    geom_column: &str,
    window: (f64, f64, f64, f64),
    select_cols: &str,
) -> String {
    let (xmin, ymin, xmax, ymax) = window;
    format!(
        "SELECT {select_cols} \
         FROM [{table}] t \
         JOIN [{table}_{geom_column}_rtree] r ON t.rowid = r.id \
         WHERE r.xmax >= {xmin} AND r.xmin <= {xmax} \
           AND r.ymax >= {ymin} AND r.ymin <= {ymax} \
           AND ST_Intersects(t.[{geom_column}], \
                             ST_MakeEnvelope({xmin}, {ymin}, {xmax}, {ymax}, 4326))",
    )
}

/// Build a [`diesel::sql_query`] that runs a geodesic nearest-N search
/// through the R-tree shadow table.
///
/// The query JOINs against the R-tree shadow with a cos(lat)-scaled
/// bounding box (same math as [`radius_bbox`]) and then `ORDER BY`s the
/// resulting candidates by `ST_DistanceSphere` to pick the N closest.
/// No `ST_DWithinSphere` refinement is needed: the `ORDER BY ... LIMIT`
/// is itself the refinement.
///
/// `search_radius_m` is the half-width of the bbox prefilter expressed
/// in metres. **The helper assumes the N true nearest neighbours all
/// sit within this radius.** If your dataset is sparse or `limit` is
/// large, the true Nth nearest may lie outside the bbox and the result
/// will be incomplete. For that case use the iterative-widening pattern
/// from [`crate::diesel::query_patterns`] Pattern 7 instead, or pick a
/// `search_radius_m` that is comfortably larger than the expected
/// neighbour distance.
///
/// `table` and `geom_column` follow the same identifier-safety contract
/// as [`dwithin_sphere_indexed_sql`].
///
/// # Example
///
/// ```
/// use diesel::{Connection, RunQueryDsl, sqlite::SqliteConnection};
/// use diesel::deserialize::QueryableByName;
/// use diesel::sql_types::BigInt;
/// use sqlitegis::diesel::query_helpers::nearest_sphere_indexed_sql;
///
/// #[derive(QueryableByName)]
/// struct Hit { #[diesel(sql_type = BigInt)] id: i64 }
///
/// sqlitegis::sqlite::register_on_every_new_connection();
/// let mut c = SqliteConnection::establish(":memory:").unwrap();
///
/// diesel::sql_query("CREATE TABLE pts (id INTEGER PRIMARY KEY, geom BLOB)")
///     .execute(&mut c).unwrap();
/// diesel::sql_query("SELECT CreateSpatialIndex('pts', 'geom')")
///     .execute(&mut c).unwrap();
/// // Berlin then Paris.
/// diesel::sql_query(
///     "INSERT INTO pts(id, geom) VALUES \
///      (1, ST_Point(13.4, 52.5, 4326)), \
///      (2, ST_Point(2.35, 48.85, 4326))",
/// ).execute(&mut c).unwrap();
///
/// // Probe from Berlin: closest is itself (id=1), then Paris (id=2).
/// let hits: Vec<Hit> = nearest_sphere_indexed_sql(
///     "pts", "geom", (13.4, 52.5), 2_000_000.0, 2, "t.id",
/// ).load::<Hit>(&mut c).unwrap();
/// assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), vec![1, 2]);
/// ```
pub fn nearest_sphere_indexed_sql(
    table: &str,
    geom_column: &str,
    probe: (f64, f64),
    search_radius_m: f64,
    limit: usize,
    select_cols: &str,
) -> diesel::query_builder::SqlQuery {
    diesel::sql_query(nearest_sphere_indexed_sql_string(
        table,
        geom_column,
        probe,
        search_radius_m,
        limit,
        select_cols,
    ))
}

/// Render the SQL string that [`nearest_sphere_indexed_sql`] wraps.
///
/// Same inputs and contract, useful when the caller needs the raw SQL
/// (for logging, for prepending `EXPLAIN QUERY PLAN`, or for piping it
/// through `diesel::sql_query` together with extra binds).
///
/// ```rust
/// use sqlitegis::diesel::query_helpers::nearest_sphere_indexed_sql_string;
///
/// let sql = nearest_sphere_indexed_sql_string(
///     "places", "geom", (13.4, 52.5), 1_000_000.0, 10, "t.id, t.name",
/// );
/// assert!(sql.contains("JOIN [places_geom_rtree]"));
/// assert!(sql.contains("ORDER BY ST_DistanceSphere"));
/// assert!(sql.contains("LIMIT 10"));
/// ```
pub fn nearest_sphere_indexed_sql_string(
    table: &str,
    geom_column: &str,
    probe: (f64, f64),
    search_radius_m: f64,
    limit: usize,
    select_cols: &str,
) -> String {
    let (lon, lat) = probe;
    let bbox = radius_bbox(lat, search_radius_m);
    format!(
        "SELECT {select_cols} \
         FROM [{table}] t \
         JOIN [{table}_{geom_column}_rtree] r ON t.rowid = r.id \
         WHERE r.xmax >= {x_min} AND r.xmin <= {x_max} \
           AND r.ymax >= {y_min} AND r.ymin <= {y_max} \
         ORDER BY ST_DistanceSphere(t.[{geom_column}], \
                                    ST_Point({lon}, {lat}, 4326)) \
         LIMIT {limit}",
        x_min = lon - bbox.dlon,
        x_max = lon + bbox.dlon,
        y_min = lat - bbox.dlat,
        y_max = lat + bbox.dlat,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `dlat` is independent of latitude.
    #[test]
    fn radius_bbox_constant_dlat() {
        let r = 1_000_000.0;
        let expected = r / METRES_PER_DEGREE;
        for lat in [-89.0_f64, -45.0, 0.0, 45.0, 89.0] {
            let bbox = radius_bbox(lat, r);
            assert!(
                (bbox.dlat - expected).abs() < 1e-9,
                "dlat at lat={lat} was {dlat}, expected {expected}",
                dlat = bbox.dlat,
            );
        }
    }

    /// `dlon` grows monotonically as `|lat|` increases.
    #[test]
    fn radius_bbox_dlon_grows_with_latitude() {
        let r = 1_000_000.0;
        let dlons: Vec<f64> = [0.0, 30.0, 45.0, 60.0, 80.0]
            .iter()
            .map(|&lat| radius_bbox(lat, r).dlon)
            .collect();
        for window in dlons.windows(2) {
            assert!(
                window[1] > window[0],
                "dlon sequence should be increasing, got {dlons:?}",
            );
        }
        // Equator: ~8.98 degrees. 45 degrees: dlon ~= 8.98 / cos(45) ~= 12.7.
        let at_45 = radius_bbox(45.0, r).dlon;
        let expected_at_45 = r / (METRES_PER_DEGREE * 45.0_f64.to_radians().cos());
        assert!(
            (at_45 - expected_at_45).abs() < 1e-6,
            "dlon at 45 was {at_45}, expected {expected_at_45}",
        );
    }

    /// Near the pole `dlon` saturates at 180 instead of diverging.
    #[test]
    fn radius_bbox_clamps_near_pole() {
        let bbox = radius_bbox(89.9999, 1_000_000.0);
        assert!(
            bbox.dlon.is_finite(),
            "dlon must stay finite near the pole, got {}",
            bbox.dlon,
        );
        assert_eq!(bbox.dlon, 180.0);
    }

    /// Regression guard for the rendered SQL shape.
    #[test]
    fn dwithin_sphere_indexed_sql_shape() {
        let sql = dwithin_sphere_indexed_sql_string(
            "places",
            "geom",
            (13.4, 52.5),
            1_000_000.0,
            "t.id, t.name",
        );
        assert!(sql.contains("SELECT t.id, t.name"), "SQL was: {sql}");
        assert!(sql.contains("FROM [places] t"), "SQL was: {sql}");
        assert!(
            sql.contains("JOIN [places_geom_rtree] r ON t.rowid = r.id"),
            "SQL was: {sql}",
        );
        assert!(sql.contains("r.xmax >="), "SQL was: {sql}");
        assert!(sql.contains("r.xmin <="), "SQL was: {sql}");
        assert!(sql.contains("r.ymax >="), "SQL was: {sql}");
        assert!(sql.contains("r.ymin <="), "SQL was: {sql}");
        assert!(sql.contains("ST_DWithinSphere(t.[geom]"), "SQL was: {sql}");
        assert!(sql.contains("ST_Point(13.4, 52.5, 4326)"), "SQL was: {sql}",);
        assert!(sql.contains("1000000"), "SQL was: {sql}");
    }

    /// Regression guard for the envelope-window helper's SQL.
    #[test]
    fn intersects_window_indexed_sql_shape() {
        let sql = intersects_window_indexed_sql_string(
            "places",
            "geom",
            (-1.6, 37.5, 28.4, 67.5),
            "t.id, t.name",
        );
        assert!(sql.contains("SELECT t.id, t.name"), "SQL was: {sql}");
        assert!(sql.contains("FROM [places] t"), "SQL was: {sql}");
        assert!(
            sql.contains("JOIN [places_geom_rtree] r ON t.rowid = r.id"),
            "SQL was: {sql}",
        );
        assert!(sql.contains("r.xmax >= -1.6"), "SQL was: {sql}");
        assert!(sql.contains("r.xmin <= 28.4"), "SQL was: {sql}");
        assert!(sql.contains("r.ymax >= 37.5"), "SQL was: {sql}");
        assert!(sql.contains("r.ymin <= 67.5"), "SQL was: {sql}");
        assert!(sql.contains("ST_Intersects(t.[geom]"), "SQL was: {sql}",);
        assert!(
            sql.contains("ST_MakeEnvelope(-1.6, 37.5, 28.4, 67.5, 4326)"),
            "SQL was: {sql}",
        );
    }

    /// Regression guard for the geodesic nearest-N helper's SQL.
    #[test]
    fn nearest_sphere_indexed_sql_shape() {
        let sql = nearest_sphere_indexed_sql_string(
            "places",
            "geom",
            (13.4, 52.5),
            1_000_000.0,
            10,
            "t.id, t.name",
        );
        assert!(sql.contains("SELECT t.id, t.name"), "SQL was: {sql}");
        assert!(sql.contains("FROM [places] t"), "SQL was: {sql}");
        assert!(
            sql.contains("JOIN [places_geom_rtree] r ON t.rowid = r.id"),
            "SQL was: {sql}",
        );
        assert!(sql.contains("r.xmax >="), "SQL was: {sql}");
        assert!(sql.contains("r.xmin <="), "SQL was: {sql}");
        assert!(sql.contains("r.ymax >="), "SQL was: {sql}");
        assert!(sql.contains("r.ymin <="), "SQL was: {sql}");
        assert!(
            sql.contains("ORDER BY ST_DistanceSphere(t.[geom]"),
            "SQL was: {sql}",
        );
        assert!(sql.contains("ST_Point(13.4, 52.5, 4326)"), "SQL was: {sql}",);
        assert!(sql.contains("LIMIT 10"), "SQL was: {sql}");
    }
}
