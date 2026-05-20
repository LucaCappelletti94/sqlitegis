//! # Index-Aware Spatial Query Patterns
//!
//! SQLite R-tree indexes require **explicit JOINs** -- unlike PostGIS where GiST
//! indexes are used transparently by the query planner. This module documents
//! copy-pasteable SQL templates and [`diesel::sql_query`] examples for every
//! common spatial query pattern.
//!
//! ## How SQLite Spatial Indexes Work
//!
//! Calling `CreateSpatialIndex('my_table', 'geom')` creates:
//!
//! - An R-tree virtual table named `my_table_geom_rtree` with columns
//!   `(id, xmin, xmax, ymin, ymax)` storing 32-bit float bounding boxes.
//! - Triggers that keep the R-tree in sync on INSERT/UPDATE/DELETE.
//! - `CreateSpatialIndex` / `DropSpatialIndex` are geolite-specific SQLite
//!   helper functions. Unlike PostGIS index DDL, they validate ownership of
//!   managed trigger/index names before mutating schema objects.
//! - Ownership markers are persisted in `geolite_spatial_index_catalog`.
//!   Helpers fail closed if managed objects exist but ownership markers are
//!   missing or externally modified.
//! - Index lifecycle stays on the raw SQL path (`diesel::sql_query`) on
//!   purpose. No typed wrappers are exported in
//!   `geolite::diesel::functions` for these two lifecycle helpers.
//!
//! Every spatial query follows a **two-stage** pattern:
//!
//! 1. **PREFILTER** -- JOIN against the R-tree to narrow candidates using
//!    bounding-box overlap. This is O(log N).
//! 2. **REFINEMENT** -- apply the exact spatial predicate (e.g., `ST_Intersects`)
//!    on the remaining candidates.
//!
//! ## R-tree Float Precision
//!
//! The R-tree stores coordinates as **32-bit floats** (approximately 7 significant digits).
//! For most use cases this is fine, but be aware that very small geometries or
//! coordinates far from the origin may lose sub-metre precision in the
//! prefilter. The refinement step always uses full 64-bit EWKB coordinates.
//!
//! ---
//!
//! ## Pattern 1: Intersects Window
//!
//! **Use case:** Find all geometries that intersect a rectangular search window
//! (e.g., map viewport).
//!
//! ### SQL Template
//!
//! ```sql
//! -- PREFILTER: R-tree bbox overlap
//! SELECT t.* FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmax >= :xmin AND r.xmin <= :xmax
//!   AND r.ymax >= :ymin AND r.ymin <= :ymax
//! -- REFINEMENT: exact predicate
//!   AND ST_Intersects(t.geom, ST_MakeEnvelope(:xmin, :ymin, :xmax, :ymax)) = 1
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sql_types::Double;
//! use diesel::sqlite::Sqlite;
//!
//! let (xmin, ymin, xmax, ymax) = (2.0, 2.0, 5.0, 5.0);
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, t.name FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmax >= ? AND r.xmin <= ? \
//!        AND r.ymax >= ? AND r.ymin <= ? \
//!        AND ST_Intersects(t.geom, ST_MakeEnvelope(?, ?, ?, ?)) = 1",
//! )
//! .bind::<Double, _>(xmin)
//! .bind::<Double, _>(xmax)
//! .bind::<Double, _>(ymin)
//! .bind::<Double, _>(ymax)
//! .bind::<Double, _>(xmin)
//! .bind::<Double, _>(ymin)
//! .bind::<Double, _>(xmax)
//! .bind::<Double, _>(ymax);
//!
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("my_table_geom_rtree"));
//! assert!(sql.contains("ST_Intersects"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! -- PostGIS: GiST index is used automatically via && operator
//! SELECT * FROM my_table
//! WHERE geom && ST_MakeEnvelope(:xmin, :ymin, :xmax, :ymax)
//!   AND ST_Intersects(geom, ST_MakeEnvelope(:xmin, :ymin, :xmax, :ymax));
//! ```
//!
//! ### Notes
//!
//! - The R-tree prefilter uses inverted comparisons (`r.xmax >= :xmin`) to
//!   check for bbox overlap, not containment.
//! - For point-only tables the refinement step is technically redundant (bbox
//!   = exact geometry), but including it keeps the pattern uniform.
//!
//! ---
//!
//! ## Pattern 2: Inside Polygon
//!
//! **Use case:** Find all features whose geometry lies entirely within a search
//! polygon (e.g., "all POIs inside this admin boundary").
//!
//! ### SQL Template
//!
//! ```sql
//! -- PREFILTER: R-tree bbox overlap with search polygon's envelope
//! SELECT t.* FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmax >= ST_XMin(:search_poly) AND r.xmin <= ST_XMax(:search_poly)
//!   AND r.ymax >= ST_YMin(:search_poly) AND r.ymin <= ST_YMax(:search_poly)
//! -- REFINEMENT: exact within test
//!   AND ST_Within(t.geom, :search_poly) = 1
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sqlite::Sqlite;
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, t.name FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmax >= 0.0 AND r.xmin <= 10.0 \
//!        AND r.ymax >= 0.0 AND r.ymin <= 10.0 \
//!        AND ST_Within(t.geom, ST_GeomFromText('POLYGON((0 0,10 0,10 10,0 10,0 0))')) = 1",
//! );
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("ST_Within"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! SELECT * FROM my_table
//! WHERE ST_Within(geom, :search_poly);
//! ```
//!
//! ### Notes
//!
//! - `ST_Within(A, B)` returns true when A is completely inside B (boundary
//!   touching counts as NOT within in the strict DE-9IM sense -- use
//!   `ST_CoveredBy` if you want boundary-inclusive semantics).
//! - The prefilter envelope should be the **search polygon's** bounding box,
//!   not the candidate's.
//!
//! ---
//!
//! ## Pattern 3: Contains Point (Reverse Geocoding)
//!
//! **Use case:** Given a point, find which polygon(s) contain it (e.g.,
//! "which admin region is this coordinate in?").
//!
//! ### SQL Template
//!
//! ```sql
//! -- PREFILTER: R-tree point containment
//! SELECT t.* FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmin <= :px AND r.xmax >= :px
//!   AND r.ymin <= :py AND r.ymax >= :py
//! -- REFINEMENT: exact containment test
//!   AND ST_Contains(t.geom, ST_Point(:px, :py)) = 1
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sql_types::Double;
//! use diesel::sqlite::Sqlite;
//!
//! let (px, py) = (5.0, 5.0);
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, t.name FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmin <= ? AND r.xmax >= ? \
//!        AND r.ymin <= ? AND r.ymax >= ? \
//!        AND ST_Contains(t.geom, ST_Point(?, ?)) = 1",
//! )
//! .bind::<Double, _>(px)
//! .bind::<Double, _>(px)
//! .bind::<Double, _>(py)
//! .bind::<Double, _>(py)
//! .bind::<Double, _>(px)
//! .bind::<Double, _>(py);
//!
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("ST_Contains"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! SELECT * FROM my_table
//! WHERE ST_Contains(geom, ST_Point(:px, :py));
//! ```
//!
//! ### Notes
//!
//! - The R-tree prefilter checks that the point falls inside each candidate's
//!   bounding box -- this is a very tight filter for convex polygons.
//! - For multipolygons or concave shapes, the refinement step eliminates
//!   false positives from bbox-only matching.
//!
//! ---
//!
//! ## Pattern 4: Geodesic Radius Search
//!
//! **Use case:** Find all features within N metres of a WGS84 point (e.g.,
//! "restaurants within 5 km of me").
//!
//! Since R-trees store coordinates in the geometry's native CRS (degrees for
//! WGS84 / SRID 4326), we compute a **degree-offset bounding box** that
//! conservatively encloses the geodesic circle:
//!
//! ```text
//! dlat = radius_m / 111_320.0
//! dlon = radius_m / (111_320.0 * cos(lat_radians))
//! ```
//!
//! This is always slightly larger than the true geodesic circle, so the
//! prefilter never produces false negatives.
//!
//! ### SQL Template
//!
//! ```sql
//! -- :lat, :lon = center point (degrees)
//! -- :radius_m  = search radius in metres
//! -- :dlat, :dlon = precomputed degree offsets (see formula above)
//!
//! -- PREFILTER: degree-offset bbox
//! SELECT t.* FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmax >= :lon - :dlon AND r.xmin <= :lon + :dlon
//!   AND r.ymax >= :lat - :dlat AND r.ymin <= :lat + :dlat
//! -- REFINEMENT: exact geodesic distance
//!   AND ST_DWithinSphere(t.geom, ST_Point(:lon, :lat, 4326), :radius_m) = 1
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sql_types::Double;
//! use diesel::sqlite::Sqlite;
//!
//! let (lon, lat) = (-0.1278f64, 51.5074f64); // London
//! let radius_m = 400_000.0f64;
//! let dlat = radius_m / 111_320.0;
//! let dlon = radius_m / (111_320.0 * lat.to_radians().cos());
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, t.name FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmax >= ? AND r.xmin <= ? \
//!        AND r.ymax >= ? AND r.ymin <= ? \
//!        AND ST_DWithinSphere(t.geom, ST_Point(?, ?, 4326), ?) = 1",
//! )
//! .bind::<Double, _>(lon - dlon) // xmax >= lon - dlon
//! .bind::<Double, _>(lon + dlon) // xmin <= lon + dlon
//! .bind::<Double, _>(lat - dlat) // ymax >= lat - dlat
//! .bind::<Double, _>(lat + dlat) // ymin <= lat + dlat
//! .bind::<Double, _>(lon)
//! .bind::<Double, _>(lat)
//! .bind::<Double, _>(radius_m);
//!
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("ST_DWithinSphere"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! SELECT * FROM my_table
//! WHERE ST_DWithin(geom::geography, ST_Point(:lon, :lat, 4326)::geography, :radius_m);
//! ```
//!
//! ### Notes
//!
//! - Use `ST_DWithinSpheroid` instead of `ST_DWithinSphere` for higher
//!   accuracy (Karney algorithm on WGS84 ellipsoid vs. Haversine on sphere).
//! - The `dlon` formula diverges near the poles (`cos(lat) -> 0`). For polar
//!   queries, clamp `dlon` to 360 deg or use a full-scan fallback.
//! - All geometries must have SRID 4326 for the geodesic functions.
//!
//! ---
//!
//! ## Pattern 5: KNN Nearest-N (Planar)
//!
//! **Use case:** Find the N closest features to a point using planar
//! (Euclidean) distance.
//!
//! SQLite R-tree has **no native KNN support** (unlike PostGIS `<->`
//! operator). The pattern is:
//!
//! 1. Pick a generous bounding box around the query point.
//! 2. R-tree prefilter to get candidates.
//! 3. Compute exact distance for each candidate.
//! 4. `ORDER BY distance LIMIT N`.
//!
//! ### SQL Template
//!
//! ```sql
//! -- :px, :py = query point
//! -- :half_w   = half-width of search box (must be large enough to contain N results)
//!
//! -- PREFILTER: bbox around query point
//! SELECT t.*, ST_Distance(t.geom, ST_Point(:px, :py)) AS dist
//! FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmax >= :px - :half_w AND r.xmin <= :px + :half_w
//!   AND r.ymax >= :py - :half_w AND r.ymin <= :py + :half_w
//! ORDER BY dist
//! LIMIT :n
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sql_types::{Double, Integer};
//! use diesel::sqlite::Sqlite;
//!
//! let (px, py) = (5.0, 5.0);
//! let half_w = 3.0;
//! let n = 5i32;
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, ST_Distance(t.geom, ST_Point(?, ?)) AS dist \
//!      FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmax >= ? AND r.xmin <= ? \
//!        AND r.ymax >= ? AND r.ymin <= ? \
//!      ORDER BY dist LIMIT ?",
//! )
//! .bind::<Double, _>(px)
//! .bind::<Double, _>(py)
//! .bind::<Double, _>(px - half_w)
//! .bind::<Double, _>(px + half_w)
//! .bind::<Double, _>(py - half_w)
//! .bind::<Double, _>(py + half_w)
//! .bind::<Integer, _>(n);
//!
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("ORDER BY dist LIMIT ?"));
//! assert!(sql.contains("ST_Distance"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! -- PostGIS: native KNN via <-> operator uses GiST index directly
//! SELECT *, geom <-> ST_Point(:px, :py) AS dist
//! FROM my_table
//! ORDER BY dist
//! LIMIT :n;
//! ```
//!
//! ### Notes
//!
//! - If fewer than N results are returned, the search box was too small --
//!   see [Pattern 7: Iterative KNN Widening](#pattern-7-iterative-knn-widening).
//! - The initial `half_w` should be chosen based on data density. For a
//!   uniform 1-unit grid, `half_w = sqrt(N)` is a reasonable starting point.
//!
//! ---
//!
//! ## Pattern 6: KNN Nearest-N (Geodesic)
//!
//! **Use case:** Find the N closest features to a WGS84 point using real-world
//! distances in metres.
//!
//! Same approach as planar KNN, but using degree-offset bounding boxes and
//! geodesic distance functions.
//!
//! ### SQL Template
//!
//! ```sql
//! -- :lon, :lat = query point (degrees, SRID 4326)
//! -- :dlat, :dlon = degree offsets for search radius (see Pattern 4)
//!
//! SELECT t.*, ST_DistanceSphere(t.geom, ST_Point(:lon, :lat, 4326)) AS dist
//! FROM my_table t
//! JOIN my_table_geom_rtree r ON t.rowid = r.id
//! WHERE r.xmax >= :lon - :dlon AND r.xmin <= :lon + :dlon
//!   AND r.ymax >= :lat - :dlat AND r.ymin <= :lat + :dlat
//! ORDER BY dist
//! LIMIT :n
//! ```
//!
//! ### Diesel Example
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::sql_types::{Double, Integer};
//! use diesel::sqlite::Sqlite;
//!
//! let (lon, lat) = (2.3522f64, 48.8566f64); // Paris
//! let search_radius_m = 1_000_000.0f64; // 1000 km
//! let dlat = search_radius_m / 111_320.0;
//! let dlon = search_radius_m / (111_320.0 * lat.to_radians().cos());
//! let n = 3i32;
//!
//! let query = diesel::sql_query(
//!     "SELECT t.id, ST_DistanceSphere(t.geom, ST_Point(?, ?, 4326)) AS dist \
//!      FROM my_table t \
//!      JOIN my_table_geom_rtree r ON t.rowid = r.id \
//!      WHERE r.xmax >= ? AND r.xmin <= ? \
//!        AND r.ymax >= ? AND r.ymin <= ? \
//!      ORDER BY dist LIMIT ?",
//! )
//! .bind::<Double, _>(lon)
//! .bind::<Double, _>(lat)
//! .bind::<Double, _>(lon - dlon)
//! .bind::<Double, _>(lon + dlon)
//! .bind::<Double, _>(lat - dlat)
//! .bind::<Double, _>(lat + dlat)
//! .bind::<Integer, _>(n);
//!
//! let sql = debug_query::<Sqlite, _>(&query).to_string();
//! assert!(sql.contains("ST_DistanceSphere"));
//! assert!(sql.contains("ORDER BY dist LIMIT ?"));
//! # }
//! ```
//!
//! ### PostGIS Equivalent
//!
//! ```sql
//! SELECT *, geom::geography <-> ST_Point(:lon, :lat, 4326)::geography AS dist
//! FROM my_table
//! ORDER BY dist
//! LIMIT :n;
//! ```
//!
//! ### Notes
//!
//! - Use `ST_DistanceSpheroid` for ellipsoidal accuracy instead of
//!   `ST_DistanceSphere` (Haversine).
//! - The degree-offset bbox is intentionally conservative -- it may include
//!   candidates outside the true geodesic circle, but the `ORDER BY`
//!   ensures correct ranking.
//!
//! ---
//!
//! ## Pattern 7: Iterative KNN Widening
//!
//! **Use case:** KNN search in sparse or unevenly distributed data where the
//! initial search box may not contain enough results.
//!
//! This is an **application-level** retry pattern -- no SQL changes needed.
//!
//! ### Algorithm
//!
//! ```rust
//! fn run_knn_query(_px: f64, _py: f64, half_w: f64, n: i32) -> Result<Vec<i32>, &'static str> {
//!     let approx = if half_w < 2.0 {
//!         2
//!     } else if half_w < 4.0 {
//!         4
//!     } else {
//!         n as usize
//!     };
//!     Ok((0..approx.min(n as usize)).map(|i| i as i32).collect())
//! }
//!
//! let (px, py) = (5.0, 5.0);
//! let mut half_w = 1.0;
//! let max_half_w = 8.0;
//! let n = 5i32;
//!
//! let final_results = loop {
//!     let results = run_knn_query(px, py, half_w, n).expect("mock query should succeed");
//!     if results.len() >= n as usize {
//!         break results;
//!     }
//!     half_w *= 2.0;
//!     if half_w > max_half_w {
//!         break results; // return best effort
//!     }
//! };
//!
//! assert_eq!(final_results.len(), n as usize);
//! ```
//!
//! ### Notes
//!
//! - Doubling the search box each iteration is a good default. The number of
//!   R-tree candidates grows quadratically with `half_w`, so this converges
//!   quickly.
//! - For geodesic KNN, double both `dlat` and `dlon` symmetrically.
//! - Set a reasonable `max_half_w` to avoid full-table scans on empty datasets.
//! - In practice, 2-3 iterations suffice for most real-world data
//!   distributions.
//!
//! ---
//!
//! ## Geodesic Radius: Input Type Restrictions
//!
//! `ST_DWithinSphere`, `ST_DWithinSpheroid`, `ST_DistanceSphere`, and
//! `ST_DistanceSpheroid` currently accept **Point geometries only**. Both
//! inputs must be non-empty Points with SRID 4326.
//!
//! This is a current geolite subset, not full PostGIS parity. PostGIS docs
//! describe broader non-Point support for `ST_DistanceSphere` and
//! `ST_DistanceSpheroid`. geolite is currently narrower. The underlying
//! `geo` crate (v0.32) only implements `Distance<Point, Point>` for its
//! `Haversine` and `Geodesic` metric spaces (unlike Euclidean, which
//! supports all geometry combinations).
//!
//! ### Workarounds for Non-Point Geometries
//!
//! - **Planar distance** (`ST_Distance`, `ST_DWithin`) works with all
//!   geometry types. If your data is in a projected CRS (e.g., UTM), use
//!   planar distance directly.
//! - **Point extraction**: use `ST_Centroid` or `ST_PointOnSurface` to
//!   reduce a polygon/linestring to a representative point, then use
//!   geodesic distance on that point.
//! - **Vertex-pair approximation**: for LineString<->Point geodesic distance,
//!   compute `ST_DistanceSphere` for each vertex and take the minimum.
//!   This is an approximation (misses points between vertices).
//!
//! ### Future Direction
//!
//! Geometry<->geometry geodesic distance would require either upstream
//! support in the `geo` crate (Haversine/Geodesic `Distance` impls for
//! `LineString`, `Polygon`, etc.) or a custom implementation that
//! iterates vertex pairs. This is deferred until either the `geo` crate
//! adds broader support or a concrete use case justifies the implementation
//! cost.
//!
//! ---
//!
//! ## Type-Aware Index Strategy: Research Findings
//!
//! **Question:** Does separating geometries by type (Point vs. LineString
//! vs. Polygon) into distinct tables -- each with its own R-tree -- improve
//! query performance compared to a single mixed-type table with one R-tree?
//!
//! ### Benchmark Setup
//!
//! - **Mixed table:** 10,000 rows -- 7,000 Points + 2,000 LineStrings +
//!   1,000 Polygons, all with spatial index.
//! - **Homogeneous tables:** 7,000 Points (separate table + index) and
//!   1,000 Polygons (separate table + index).
//! - **Queries tested:**
//!   1. "Find all Points in window \[40,40\]-\[60,60\]" (intersects window)
//!   2. "Find Polygon containing point (25, 9)" (reverse geocoding)
//!
//! ### Results (release mode, best-of-20 runs)
//!
//! | Scenario | Homogeneous | Mixed + type filter | Mixed, no filter |
//! |----------|-------------|---------------------|------------------|
//! | Points in window | 360 us | 495 us | 457 us |
//! | Reverse geocode | 11 us | 15 us | 13 us |
//!
//! ### Conclusions
//!
//! 1. **Overhead is only 1.3-1.4x** -- mostly due to the R-tree being
//!    larger (10K vs. 7K/1K entries), not type mismatch.
//! 2. **Adding `ST_GeometryType()` filter is counterproductive** -- the
//!    type-check cost exceeds the savings from skipping predicates on
//!    wrong-type geometries.
//! 3. **The R-tree prefilter is highly selective** -- after bbox narrowing,
//!    only a small number of candidates reach the refinement step. Running
//!    `ST_Contains` or `ST_Intersects` on a few extra non-matching
//!    geometries is negligible.
//!
//! **Recommendation:** Keep the current single-R-tree-per-column design.
//! Type-aware partitioning adds DDL complexity, trigger maintenance
//! overhead, and application-level routing logic with no meaningful query
//! speedup. If a future workload with millions of rows and highly skewed
//! type distributions shows a different profile, revisit with fresh
//! benchmarks.
