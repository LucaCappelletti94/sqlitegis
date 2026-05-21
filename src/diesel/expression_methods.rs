//! Extension trait for method-style spatial operations on geometry expressions.
//!
//! Import [`GeometryExpressionMethods`] (or `use sqlitegis::diesel::prelude::*`)
//! to call spatial functions as methods on any `Nullable<Geometry>` expression:
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::prelude::*;
//! use diesel::sqlite::Sqlite;
//! use diesel::NullableExpressionMethods;
//! use sqlitegis::diesel::prelude::*;
//!
//! diesel::table! {
//!     features (id) {
//!         id -> Integer,
//!         geom -> Nullable<sqlitegis::diesel::Geometry>,
//!     }
//! }
//!
//! let query = features::table
//!     .filter(features::geom.st_dwithin(st_point(13.4050, 52.5200).nullable(), 1000.0).eq(true))
//!     .select((features::id, features::geom.st_astext()));
//! let sql = debug_query::<Sqlite, _>(&query).to_string().to_lowercase();
//! assert!(sql.contains("st_dwithin"));
//! # }
//! ```
//!
//! Geometry-pair DE-9IM matching is available in method form:
//!
//! ```rust
//! # #[cfg(feature = "diesel-sqlite")]
//! # {
//! use diesel::debug_query;
//! use diesel::dsl::select;
//! use diesel::sqlite::Sqlite;
//! use diesel::NullableExpressionMethods;
//! use sqlitegis::diesel::prelude::*;
//!
//! let pattern = "T*****FF*";
//!
//! let via_match_geoms = select(
//!     st_point(0.0, 0.0)
//!         .nullable()
//!         .st_relate_match_geoms(st_point(0.0, 0.0).nullable(), pattern),
//! );
//! let geoms_sql = debug_query::<Sqlite, _>(&via_match_geoms).to_string().to_lowercase();
//! assert!(geoms_sql.contains("st_relate"));
//!
//! let via_matrix = select(st_relate_match("T********", pattern));
//! let matrix_sql = debug_query::<Sqlite, _>(&via_matrix).to_string().to_lowercase();
//! assert!(matrix_sql.contains("st_relatematch"));
//! # }
//! ```

use diesel::expression::{AsExpression, Expression};
use diesel::sql_types::{Double, Integer, Nullable};

use crate::diesel::functions;
use crate::diesel::types::Geometry;

/// Method-style access to spatial SQL functions for `Nullable<Geometry>` expressions.
///
/// This trait is automatically implemented for any Diesel expression with
/// `SqlType = Nullable<Geometry>`. Each method delegates to the corresponding
/// free function in [`crate::diesel::functions`].
///
/// For non-nullable `Geometry` columns, call `.nullable()` first. This is
/// the standard Diesel pattern.
pub trait GeometryExpressionMethods: Expression<SqlType = Nullable<Geometry>> + Sized {
    // I/O

    /// Serialize this geometry to WKT text.
    ///
    /// See [`crate::diesel::functions::st_astext()`] for an executable example.
    fn st_astext(self) -> functions::st_astext<Self> {
        functions::st_astext(self)
    }

    /// Serialize this geometry to EWKT text (`SRID=n;WKT`).
    ///
    /// See [`crate::diesel::functions::st_asewkt()`] for an executable example.
    fn st_asewkt(self) -> functions::st_asewkt<Self> {
        functions::st_asewkt(self)
    }

    /// Serialize this geometry to ISO WKB bytes.
    ///
    /// See [`crate::diesel::functions::st_asbinary()`] for an executable example.
    fn st_asbinary(self) -> functions::st_asbinary<Self> {
        functions::st_asbinary(self)
    }

    /// Serialize this geometry to EWKB bytes (preserves SRID).
    ///
    /// See [`crate::diesel::functions::st_asewkb()`] for an executable example.
    fn st_asewkb(self) -> functions::st_asewkb<Self> {
        functions::st_asewkb(self)
    }

    /// Serialize this geometry to GeoJSON text.
    ///
    /// See [`crate::diesel::functions::st_asgeojson()`] for an executable example.
    fn st_asgeojson(self) -> functions::st_asgeojson<Self> {
        functions::st_asgeojson(self)
    }

    // Constructors / transforms

    /// Construct a LineString from this geometry and another Point geometry.
    ///
    /// See [`crate::diesel::functions::st_makeline()`] for an executable example.
    fn st_makeline<T>(self, other: T) -> functions::st_makeline<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_makeline(self, other)
    }

    /// Construct a Polygon from this geometry treated as a shell LineString.
    ///
    /// See [`crate::diesel::functions::st_makepolygon()`] for an executable example.
    fn st_makepolygon(self) -> functions::st_makepolygon<Self> {
        functions::st_makepolygon(self)
    }

    /// Combine this geometry with another into a GeometryCollection.
    ///
    /// See [`crate::diesel::functions::st_collect()`] for an executable example.
    fn st_collect<T>(self, other: T) -> functions::st_collect<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_collect(self, other)
    }

    // Accessors

    /// Return the SRID embedded in the geometry EWKB header.
    ///
    /// See [`crate::diesel::functions::st_srid()`] for an executable example.
    fn st_srid(self) -> functions::st_srid<Self> {
        functions::st_srid(self)
    }

    /// Set (replace) the SRID in the geometry EWKB header.
    ///
    /// See [`crate::diesel::functions::st_setsrid()`] for an executable example.
    fn st_setsrid<S>(self, srid: S) -> functions::st_setsrid<Self, S>
    where
        S: AsExpression<Integer>,
    {
        functions::st_setsrid(self, srid)
    }

    /// Return the OGC geometry type name (e.g. `ST_Point`, `ST_Polygon`).
    ///
    /// See [`crate::diesel::functions::st_geometrytype()`] for an executable example.
    fn st_geometrytype(self) -> functions::st_geometrytype<Self> {
        functions::st_geometrytype(self)
    }

    /// Return the X coordinate of a Point geometry.
    ///
    /// See [`crate::diesel::functions::st_x()`] for an executable example.
    fn st_x(self) -> functions::st_x<Self> {
        functions::st_x(self)
    }

    /// Return the Y coordinate of a Point geometry.
    ///
    /// See [`crate::diesel::functions::st_y()`] for an executable example.
    fn st_y(self) -> functions::st_y<Self> {
        functions::st_y(self)
    }

    /// Return the Z coordinate of a Point geometry when present.
    ///
    /// See [`crate::diesel::functions::st_z()`] for an executable example.
    fn st_z(self) -> functions::st_z<Self> {
        functions::st_z(self)
    }

    /// Return whether the geometry is empty.
    ///
    /// See [`crate::diesel::functions::st_isempty()`] for an executable example.
    fn st_isempty(self) -> functions::st_isempty<Self> {
        functions::st_isempty(self)
    }

    /// Return the number of coordinate dimensions (2, 3, or 4).
    ///
    /// See [`crate::diesel::functions::st_ndims()`] for an executable example.
    fn st_ndims(self) -> functions::st_ndims<Self> {
        functions::st_ndims(self)
    }

    /// Return the coordinate dimension (same as `ST_NDims` for non-curve types).
    ///
    /// See [`crate::diesel::functions::st_coorddim()`] for an executable example.
    fn st_coorddim(self) -> functions::st_coorddim<Self> {
        functions::st_coorddim(self)
    }

    /// Return the Z/M dimensionality flag (0=2D, 1=M, 2=Z, 3=ZM).
    ///
    /// See [`crate::diesel::functions::st_zmflag()`] for an executable example.
    fn st_zmflag(self) -> functions::st_zmflag<Self> {
        functions::st_zmflag(self)
    }

    /// Return the EWKB memory size in bytes.
    ///
    /// See [`crate::diesel::functions::st_memsize()`] for an executable example.
    fn st_memsize(self) -> functions::st_memsize<Self> {
        functions::st_memsize(self)
    }

    /// Return whether geometry is valid.
    ///
    /// See [`crate::diesel::functions::st_isvalid()`] for an executable example.
    fn st_isvalid(self) -> functions::st_isvalid<Self> {
        functions::st_isvalid(self)
    }

    /// Return the validity reason string.
    ///
    /// See [`crate::diesel::functions::st_isvalidreason()`] for an executable example.
    fn st_isvalidreason(self) -> functions::st_isvalidreason<Self> {
        functions::st_isvalidreason(self)
    }

    /// Return the number of points in a LineString.
    ///
    /// See [`crate::diesel::functions::st_numpoints()`] for an executable example.
    fn st_numpoints(self) -> functions::st_numpoints<Self> {
        functions::st_numpoints(self)
    }

    /// Return the total point count across any geometry type.
    ///
    /// See [`crate::diesel::functions::st_npoints()`] for an executable example.
    fn st_npoints(self) -> functions::st_npoints<Self> {
        functions::st_npoints(self)
    }

    /// Return the number of component geometries.
    ///
    /// See [`crate::diesel::functions::st_numgeometries()`] for an executable example.
    fn st_numgeometries(self) -> functions::st_numgeometries<Self> {
        functions::st_numgeometries(self)
    }

    /// Return the number of interior rings in a Polygon.
    ///
    /// See [`crate::diesel::functions::st_numinteriorrings()`] for an executable example.
    fn st_numinteriorrings(self) -> functions::st_numinteriorrings<Self> {
        functions::st_numinteriorrings(self)
    }

    /// Alias for `st_numinteriorrings`: return the number of interior rings in a Polygon.
    ///
    /// See [`crate::diesel::functions::st_numinteriorring()`] for an executable example.
    fn st_numinteriorring(self) -> functions::st_numinteriorring<Self> {
        functions::st_numinteriorring(self)
    }

    /// Return the total number of rings in a Polygon.
    ///
    /// See [`crate::diesel::functions::st_numrings()`] for an executable example.
    fn st_numrings(self) -> functions::st_numrings<Self> {
        functions::st_numrings(self)
    }

    /// Return the topological dimension (0, 1, or 2).
    ///
    /// See [`crate::diesel::functions::st_dimension()`] for an executable example.
    fn st_dimension(self) -> functions::st_dimension<Self> {
        functions::st_dimension(self)
    }

    /// Return the axis-aligned envelope of this geometry.
    ///
    /// See [`crate::diesel::functions::st_envelope()`] for an executable example.
    fn st_envelope(self) -> functions::st_envelope<Self> {
        functions::st_envelope(self)
    }

    /// Return the 1-based Nth point of this LineString.
    ///
    /// See [`crate::diesel::functions::st_pointn()`] for an executable example.
    fn st_pointn<S>(self, n: S) -> functions::st_pointn<Self, S>
    where
        S: AsExpression<Integer>,
    {
        functions::st_pointn(self, n)
    }

    /// Return the first point of this LineString.
    ///
    /// See [`crate::diesel::functions::st_startpoint()`] for an executable example.
    fn st_startpoint(self) -> functions::st_startpoint<Self> {
        functions::st_startpoint(self)
    }

    /// Return the last point of this LineString.
    ///
    /// See [`crate::diesel::functions::st_endpoint()`] for an executable example.
    fn st_endpoint(self) -> functions::st_endpoint<Self> {
        functions::st_endpoint(self)
    }

    /// Return the exterior ring of this Polygon.
    ///
    /// See [`crate::diesel::functions::st_exteriorring()`] for an executable example.
    fn st_exteriorring(self) -> functions::st_exteriorring<Self> {
        functions::st_exteriorring(self)
    }

    /// Return the 1-based Nth interior ring of this Polygon.
    ///
    /// See [`crate::diesel::functions::st_interiorringn()`] for an executable example.
    fn st_interiorringn<S>(self, n: S) -> functions::st_interiorringn<Self, S>
    where
        S: AsExpression<Integer>,
    {
        functions::st_interiorringn(self, n)
    }

    /// Return the 1-based Nth geometry from this collection.
    ///
    /// See [`crate::diesel::functions::st_geometryn()`] for an executable example.
    fn st_geometryn<S>(self, n: S) -> functions::st_geometryn<Self, S>
    where
        S: AsExpression<Integer>,
    {
        functions::st_geometryn(self, n)
    }

    /// Return the X coordinate of the bounding-box minimum corner.
    ///
    /// See [`crate::diesel::functions::st_xmin()`] for an executable example.
    fn st_xmin(self) -> functions::st_xmin<Self> {
        functions::st_xmin(self)
    }

    /// Return the X coordinate of the bounding-box maximum corner.
    ///
    /// See [`crate::diesel::functions::st_xmax()`] for an executable example.
    fn st_xmax(self) -> functions::st_xmax<Self> {
        functions::st_xmax(self)
    }

    /// Return the Y coordinate of the bounding-box minimum corner.
    ///
    /// See [`crate::diesel::functions::st_ymin()`] for an executable example.
    fn st_ymin(self) -> functions::st_ymin<Self> {
        functions::st_ymin(self)
    }

    /// Return the Y coordinate of the bounding-box maximum corner.
    ///
    /// See [`crate::diesel::functions::st_ymax()`] for an executable example.
    fn st_ymax(self) -> functions::st_ymax<Self> {
        functions::st_ymax(self)
    }

    // Measurement

    /// Return the planar area of a polygon geometry.
    ///
    /// See [`crate::diesel::functions::st_area()`] for an executable example.
    fn st_area(self) -> functions::st_area<Self> {
        functions::st_area(self)
    }

    /// Return the planar length of a linestring geometry.
    ///
    /// See [`crate::diesel::functions::st_length()`] for an executable example.
    fn st_length(self) -> functions::st_length<Self> {
        functions::st_length(self)
    }

    /// Alias for `st_length`: return the planar length of a linestring geometry.
    ///
    /// See [`crate::diesel::functions::st_length2d()`] for an executable example.
    fn st_length2d(self) -> functions::st_length2d<Self> {
        functions::st_length2d(self)
    }

    /// Return the planar perimeter of a polygon geometry.
    ///
    /// See [`crate::diesel::functions::st_perimeter()`] for an executable example.
    fn st_perimeter(self) -> functions::st_perimeter<Self> {
        functions::st_perimeter(self)
    }

    /// Alias for `st_perimeter`: return the planar perimeter of a polygon geometry.
    ///
    /// See [`crate::diesel::functions::st_perimeter2d()`] for an executable example.
    fn st_perimeter2d(self) -> functions::st_perimeter2d<Self> {
        functions::st_perimeter2d(self)
    }

    /// Return the minimum Euclidean distance to another geometry.
    ///
    /// See [`crate::diesel::functions::st_distance()`] for an executable example.
    fn st_distance<T>(self, other: T) -> functions::st_distance<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_distance(self, other)
    }

    /// Return the Haversine (spherical) distance in metres to another geometry.
    ///
    /// See [`crate::diesel::functions::st_distancesphere()`] for an executable example.
    fn st_distancesphere<T>(self, other: T) -> functions::st_distancesphere<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_distancesphere(self, other)
    }

    /// Return the geodesic distance in metres to another geometry (Karney algorithm).
    ///
    /// See [`crate::diesel::functions::st_distancespheroid()`] for an executable example.
    fn st_distancespheroid<T>(self, other: T) -> functions::st_distancespheroid<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_distancespheroid(self, other)
    }

    /// Return the Hausdorff distance to another geometry.
    ///
    /// See [`crate::diesel::functions::st_hausdorffdistance()`] for an executable example.
    fn st_hausdorffdistance<T>(self, other: T) -> functions::st_hausdorffdistance<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_hausdorffdistance(self, other)
    }

    /// Return the centroid of this geometry.
    ///
    /// See [`crate::diesel::functions::st_centroid()`] for an executable example.
    fn st_centroid(self) -> functions::st_centroid<Self> {
        functions::st_centroid(self)
    }

    /// Return a point guaranteed to lie on or inside this geometry.
    ///
    /// See [`crate::diesel::functions::st_pointonsurface()`] for an executable example.
    fn st_pointonsurface(self) -> functions::st_pointonsurface<Self> {
        functions::st_pointonsurface(self)
    }

    // Operations

    /// Compute the geometric union of this geometry with another.
    ///
    /// See [`crate::diesel::functions::st_union()`] for an executable example.
    fn st_union<T>(self, other: T) -> functions::st_union<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_union(self, other)
    }

    /// Compute the geometric intersection of this geometry with another.
    ///
    /// See [`crate::diesel::functions::st_intersection()`] for an executable example.
    fn st_intersection<T>(self, other: T) -> functions::st_intersection<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_intersection(self, other)
    }

    /// Compute the geometric difference of this geometry minus another.
    ///
    /// See [`crate::diesel::functions::st_difference()`] for an executable example.
    fn st_difference<T>(self, other: T) -> functions::st_difference<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_difference(self, other)
    }

    /// Compute the symmetric difference of this geometry and another.
    ///
    /// See [`crate::diesel::functions::st_symdifference()`] for an executable example.
    fn st_symdifference<T>(self, other: T) -> functions::st_symdifference<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_symdifference(self, other)
    }

    /// Expand or shrink this geometry by a given distance.
    ///
    /// See [`crate::diesel::functions::st_buffer()`] for an executable example.
    fn st_buffer<D>(self, distance: D) -> functions::st_buffer<Self, D>
    where
        D: AsExpression<Double>,
    {
        functions::st_buffer(self, distance)
    }

    // Predicates

    /// Return whether this geometry shares any interior or boundary points with another.
    ///
    /// See [`crate::diesel::functions::st_intersects()`] for an executable example.
    fn st_intersects<T>(self, other: T) -> functions::st_intersects<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_intersects(self, other)
    }

    /// Return whether this geometry fully contains another.
    ///
    /// See [`crate::diesel::functions::st_contains()`] for an executable example.
    fn st_contains<T>(self, other: T) -> functions::st_contains<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_contains(self, other)
    }

    /// Return whether this geometry is fully contained within another.
    ///
    /// See [`crate::diesel::functions::st_within()`] for an executable example.
    fn st_within<T>(self, other: T) -> functions::st_within<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_within(self, other)
    }

    /// Convenience strict "inside area" helper.
    ///
    /// Delegates to `ST_Within`: boundary-touching geometries are not inside.
    fn inside_area<T>(self, area: T) -> functions::inside_area<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::inside_area(self, area)
    }

    /// Return whether this geometry covers another.
    ///
    /// See [`crate::diesel::functions::st_covers()`] for an executable example.
    fn st_covers<T>(self, other: T) -> functions::st_covers<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_covers(self, other)
    }

    /// Return whether this geometry is covered by another.
    ///
    /// See [`crate::diesel::functions::st_coveredby()`] for an executable example.
    fn st_coveredby<T>(self, other: T) -> functions::st_coveredby<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_coveredby(self, other)
    }

    /// Return whether this geometry shares no points with another.
    ///
    /// See [`crate::diesel::functions::st_disjoint()`] for an executable example.
    fn st_disjoint<T>(self, other: T) -> functions::st_disjoint<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_disjoint(self, other)
    }

    /// Convenience strict "outside area" helper.
    ///
    /// Delegates to `ST_Disjoint`: boundary-touching geometries are not outside.
    fn outside_area<T>(self, area: T) -> functions::outside_area<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::outside_area(self, area)
    }

    /// Return whether this geometry is spatially equal to another.
    ///
    /// See [`crate::diesel::functions::st_equals()`] for an executable example.
    fn st_equals<T>(self, other: T) -> functions::st_equals<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_equals(self, other)
    }

    /// Return whether this geometry and another are within the given Euclidean distance.
    ///
    /// See [`crate::diesel::functions::st_dwithin()`] for an executable example.
    fn st_dwithin<T, D>(self, other: T, distance: D) -> functions::st_dwithin<Self, T, D>
    where
        T: AsExpression<Nullable<Geometry>>,
        D: AsExpression<Double>,
    {
        functions::st_dwithin(self, other, distance)
    }

    /// Return whether this geographic point and another are within the given
    /// distance in metres using Haversine distance.
    ///
    /// See [`crate::diesel::functions::st_dwithinsphere()`] for an executable example.
    fn st_dwithinsphere<T, D>(
        self,
        other: T,
        distance: D,
    ) -> functions::st_dwithinsphere<Self, T, D>
    where
        T: AsExpression<Nullable<Geometry>>,
        D: AsExpression<Double>,
    {
        functions::st_dwithinsphere(self, other, distance)
    }

    /// Return whether this geographic point and another are within the given
    /// distance in metres using geodesic (spheroid) distance.
    ///
    /// See [`crate::diesel::functions::st_dwithinspheroid()`] for an executable example.
    fn st_dwithinspheroid<T, D>(
        self,
        other: T,
        distance: D,
    ) -> functions::st_dwithinspheroid<Self, T, D>
    where
        T: AsExpression<Nullable<Geometry>>,
        D: AsExpression<Double>,
    {
        functions::st_dwithinspheroid(self, other, distance)
    }

    /// Return whether this geometry shares boundary points but no interior points with another.
    ///
    /// See [`crate::diesel::functions::st_touches()`] for an executable example.
    fn st_touches<T>(self, other: T) -> functions::st_touches<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_touches(self, other)
    }

    /// Return whether this geometry crosses another.
    ///
    /// See [`crate::diesel::functions::st_crosses()`] for an executable example.
    fn st_crosses<T>(self, other: T) -> functions::st_crosses<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_crosses(self, other)
    }

    /// Return whether this geometry overlaps another.
    ///
    /// See [`crate::diesel::functions::st_overlaps()`] for an executable example.
    fn st_overlaps<T>(self, other: T) -> functions::st_overlaps<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_overlaps(self, other)
    }

    /// Return the DE-9IM relationship matrix string between this and another geometry.
    ///
    /// See [`crate::diesel::functions::st_relate()`] for an executable example.
    fn st_relate<T>(self, other: T) -> functions::st_relate<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_relate(self, other)
    }

    /// Alias of `ST_Relate(a, b, pattern)` matching core naming.
    ///
    /// See [`crate::diesel::functions::st_relate_match_geoms()`] for an executable example.
    fn st_relate_match_geoms<T, P>(
        self,
        other: T,
        pattern: P,
    ) -> functions::st_relate_match_geoms<Self, T, P>
    where
        T: AsExpression<Nullable<Geometry>>,
        P: AsExpression<diesel::sql_types::Text>,
    {
        functions::st_relate_match_geoms(self, other, pattern)
    }

    // Geography variants

    /// Haversine arc length of a linestring in metres.
    ///
    /// See [`crate::diesel::functions::st_lengthsphere()`] for an executable example.
    fn st_lengthsphere(self) -> functions::st_lengthsphere<Self> {
        functions::st_lengthsphere(self)
    }

    /// Geodesic bearing from this geometry to target in radians (0 = north, clockwise).
    ///
    /// See [`crate::diesel::functions::st_azimuth()`] for an executable example.
    fn st_azimuth<T>(self, target: T) -> functions::st_azimuth<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_azimuth(self, target)
    }

    /// Destination point from this geometry given distance (metres) and azimuth (radians).
    ///
    /// See [`crate::diesel::functions::st_project()`] for an executable example.
    fn st_project<D, A>(self, distance: D, azimuth: A) -> functions::st_project<Self, D, A>
    where
        D: AsExpression<Double>,
        A: AsExpression<Double>,
    {
        functions::st_project(self, distance, azimuth)
    }

    /// Closest point on this geometry to another.
    ///
    /// See [`crate::diesel::functions::st_closestpoint()`] for an executable example.
    fn st_closestpoint<T>(self, other: T) -> functions::st_closestpoint<Self, T>
    where
        T: AsExpression<Nullable<Geometry>>,
    {
        functions::st_closestpoint(self, other)
    }
}

impl<E> GeometryExpressionMethods for E where E: Expression<SqlType = Nullable<Geometry>> + Sized {}
