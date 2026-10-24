//! Diesel SQL type definitions and `FromSql` / `ToSql` implementations.
//!
//! Both `Geometry` and `Geography` map to `Binary` (BLOB) in SQLite and
//! to PostGIS's native `geometry` / `geography` types in PostgreSQL,
//! storing EWKB-encoded geometry.

use crate::core::geometry_kind::GeometryKind;

// SQL types

/// Diesel SQL type for a geometry column (stored as EWKB BLOB).
///
/// ```rust
/// diesel::table! {
///     features (id) {
///         id   -> Integer,
///         geom -> sqlitegis::diesel::Geometry,
///     }
/// }
///
/// let _table = features::table;
/// ```
#[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId, Debug, Clone, Copy)]
#[diesel(sqlite_type(name = "Binary"))]
#[diesel(postgres_type(name = "geometry"))]
pub struct Geometry;

/// Diesel SQL type for a geography column (stored as EWKB BLOB, SRID = 4326).
///
/// Same wire format as [`Geometry`], but `FromSql` enforces SRID 4326.
/// Spatial functions use spherical/geodesic algorithms.
#[derive(diesel::sql_types::SqlType, diesel::query_builder::QueryId, Debug, Clone, Copy)]
#[diesel(sqlite_type(name = "Binary"))]
#[diesel(postgres_type(name = "geography"))]
pub struct Geography;

/// Defines a wrapper of a [`GeometryKind`] bound to one spatial SQL type.
macro_rules! geo_value {
    ($(#[$meta:meta])* $name:ident, $sql_type:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, diesel::expression::AsExpression, diesel::deserialize::FromSqlRow,
        )]
        #[diesel(sql_type = $sql_type)]
        #[cfg_attr(
            feature = "serde",
            derive(serde::Serialize, serde::Deserialize),
            serde(transparent)
        )]
        pub struct $name<G = geo::Geometry<f64>>(pub G);

        impl<G: GeometryKind> From<G> for $name<G> {
            fn from(geometry: G) -> Self {
                Self(geometry)
            }
        }
    };
}

geo_value!(
    /// A [`geo::Geometry<f64>`] bindable to [`Geometry`] columns, nullable or
    /// NOT NULL. It is written without an SRID and read whatever its SRID.
    ///
    /// The parameter narrows the value to one [`GeometryKind`], as a PostGIS
    /// `geometry(Point)` column does: `GeometryValue<geo::Point<f64>>` holds
    /// only points, and reading another geometry into it fails.
    ///
    /// ```rust
    /// use diesel::prelude::*;
    /// use sqlitegis::diesel::GeometryValue;
    ///
    /// diesel::table! {
    ///     parcels (id) {
    ///         id    -> Integer,
    ///         shape -> sqlitegis::diesel::Geometry,
    ///     }
    /// }
    ///
    /// let corner = GeometryValue::from(geo::Geometry::Point(geo::Point::new(1.0, 2.0)));
    /// let _insert = diesel::insert_into(parcels::table).values(parcels::shape.eq(&corner));
    /// assert_eq!(corner.0, geo::Geometry::Point(geo::Point::new(1.0, 2.0)));
    /// ```
    ///
    /// A [`Geography`] column rejects it at compile time.
    ///
    /// ```compile_fail
    /// use diesel::prelude::*;
    /// use sqlitegis::diesel::GeometryValue;
    ///
    /// diesel::table! {
    ///     places (id) {
    ///         id       -> Integer,
    ///         location -> sqlitegis::diesel::Geography,
    ///     }
    /// }
    ///
    /// let corner = GeometryValue::from(geo::Geometry::Point(geo::Point::new(1.0, 2.0)));
    /// let _insert = diesel::insert_into(places::table).values(places::location.eq(&corner));
    /// ```
    GeometryValue,
    Geometry
);

geo_value!(
    /// A [`geo::Geometry<f64>`] bindable to [`Geography`] columns, nullable or
    /// NOT NULL. It is written with SRID 4326 and reading it requires SRID 4326.
    ///
    /// The parameter narrows the value to one [`GeometryKind`], as a PostGIS
    /// `geography(Point)` column does: `GeographyValue<geo::Point<f64>>` holds
    /// only points, and reading another geometry into it fails.
    ///
    /// ```rust
    /// use diesel::prelude::*;
    /// use sqlitegis::diesel::GeographyValue;
    ///
    /// diesel::table! {
    ///     places (id) {
    ///         id       -> Integer,
    ///         location -> sqlitegis::diesel::Geography,
    ///     }
    /// }
    ///
    /// let berlin = GeographyValue::from(geo::Geometry::Point(geo::Point::new(13.4, 52.5)));
    /// let _insert = diesel::insert_into(places::table).values(places::location.eq(&berlin));
    /// assert_eq!(berlin.0, geo::Geometry::Point(geo::Point::new(13.4, 52.5)));
    /// ```
    ///
    /// A [`Geometry`] column rejects it at compile time.
    ///
    /// ```compile_fail
    /// use diesel::prelude::*;
    /// use sqlitegis::diesel::GeographyValue;
    ///
    /// diesel::table! {
    ///     parcels (id) {
    ///         id    -> Integer,
    ///         shape -> sqlitegis::diesel::Geometry,
    ///     }
    /// }
    ///
    /// let berlin = GeographyValue::from(geo::Geometry::Point(geo::Point::new(13.4, 52.5)));
    /// let _insert = diesel::insert_into(parcels::table).values(parcels::shape.eq(&berlin));
    /// ```
    GeographyValue,
    Geography
);

#[cfg(any(feature = "diesel-sqlite", feature = "diesel-postgres"))]
type DynError = Box<dyn std::error::Error + Send + Sync>;

#[cfg(any(feature = "diesel-sqlite", feature = "diesel-postgres"))]
fn parse_blob_with_srid_constraint(
    blob: &[u8],
    required_srid: Option<i32>,
) -> std::result::Result<geo::Geometry<f64>, DynError> {
    let (geom, srid) = crate::core::ewkb::parse_ewkb(blob).map_err(|e| Box::new(e) as DynError)?;
    if let Some(expected) = required_srid {
        match srid {
            Some(actual) if actual == expected => {}
            Some(other) => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("geography EWKB must use SRID {expected} (got {other})"),
                )));
            }
            None => {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("geography EWKB must include SRID {expected}"),
                )));
            }
        }
    }
    Ok(geom)
}

// SQLite FromSql / ToSql

#[cfg(feature = "diesel-sqlite")]
mod sqlite_impls {
    use super::*;
    use diesel::deserialize::{self, FromSql};
    use diesel::serialize::{self, IsNull, Output, ToSql};
    use diesel::sql_types::Binary;
    use diesel::sqlite::Sqlite;
    // SQLite Output does NOT implement std::io::Write.
    // Binary values are passed via `out.set_value(value)` where value
    // implements `Into<SqliteBindValue>` (e.g. &[u8], Vec<u8>).

    // --- Vec<u8> (raw EWKB bytes) ---

    macro_rules! impl_raw_bytes {
        ($sql_type:ty) => {
            impl FromSql<$sql_type, Sqlite> for Vec<u8> {
                fn from_sql(
                    bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    <Vec<u8> as FromSql<Binary, Sqlite>>::from_sql(bytes)
                }
            }

            impl ToSql<$sql_type, Sqlite> for Vec<u8> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
                    out.set_value(self.as_slice());
                    Ok(IsNull::No)
                }
            }
        };
    }

    impl_raw_bytes!(Geometry);
    impl_raw_bytes!(Geography);

    impl ToSql<Geometry, Sqlite> for [u8] {
        fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
            out.set_value(self);
            Ok(IsNull::No)
        }
    }

    // --- geo::Geometry<f64> ---
    //
    // The only semantic difference between Geometry and Geography here is the
    // SRID arg: Geometry writes no SRID and accepts any. Geography forces
    // 4326 on write and requires 4326 on read.

    macro_rules! impl_geo_geometry_sqlite {
        ($sql_type:ty, $value:ident, $srid:expr) => {
            impl FromSql<$sql_type, Sqlite> for geo::Geometry<f64> {
                fn from_sql(
                    bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    let blob = <Vec<u8> as FromSql<Binary, Sqlite>>::from_sql(bytes)?;
                    super::parse_blob_with_srid_constraint(&blob, $srid).map_err(Into::into)
                }
            }

            impl ToSql<$sql_type, Sqlite> for geo::Geometry<f64> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
                    let blob = crate::core::ewkb::write_ewkb(self, $srid)
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                    out.set_value(blob);
                    Ok(IsNull::No)
                }
            }

            impl<G: GeometryKind> FromSql<$sql_type, Sqlite> for $value<G> {
                fn from_sql(
                    bytes: <Sqlite as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    let geometry =
                        <geo::Geometry<f64> as FromSql<$sql_type, Sqlite>>::from_sql(bytes)?;
                    Ok($value(G::from_geometry(geometry)?))
                }
            }

            impl<G: GeometryKind> ToSql<$sql_type, Sqlite> for $value<G> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Sqlite>) -> serialize::Result {
                    out.set_value(self.0.to_ewkb($srid)?);
                    Ok(IsNull::No)
                }
            }
        };
    }

    impl_geo_geometry_sqlite!(Geometry, GeometryValue, None);
    impl_geo_geometry_sqlite!(Geography, GeographyValue, Some(4326));
}

// PostgreSQL FromSql / ToSql

#[cfg(feature = "diesel-postgres")]
mod postgres_impls {
    use super::*;
    use diesel::deserialize::{self, FromSql};
    use diesel::pg::Pg;
    use diesel::serialize::{self, IsNull, Output, ToSql};
    use std::io::Write as IoWrite;

    // PostgreSQL Output implements std::io::Write, so binary data is written
    // via `IoWrite::write_all(out, &bytes)`.

    // --- Vec<u8> (raw EWKB bytes) ---

    macro_rules! impl_raw_bytes_pg {
        ($sql_type:ty) => {
            impl FromSql<$sql_type, Pg> for Vec<u8> {
                fn from_sql(
                    bytes: <Pg as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    Ok(bytes.as_bytes().to_vec())
                }
            }

            impl ToSql<$sql_type, Pg> for Vec<u8> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
                    IoWrite::write_all(out, self)?;
                    Ok(IsNull::No)
                }
            }
        };
    }

    impl_raw_bytes_pg!(Geometry);
    impl_raw_bytes_pg!(Geography);

    impl ToSql<Geometry, Pg> for [u8] {
        fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
            IoWrite::write_all(out, self)?;
            Ok(IsNull::No)
        }
    }

    // --- geo::Geometry<f64> ---
    //
    // Mirror of the sqlite_impls macro: the only intra-type variation is
    // the SRID arg passed to parse_blob_with_srid_constraint / write_ewkb.

    macro_rules! impl_geo_geometry_pg {
        ($sql_type:ty, $value:ident, $srid:expr) => {
            impl FromSql<$sql_type, Pg> for geo::Geometry<f64> {
                fn from_sql(
                    bytes: <Pg as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    super::parse_blob_with_srid_constraint(bytes.as_bytes(), $srid)
                        .map_err(Into::into)
                }
            }

            impl ToSql<$sql_type, Pg> for geo::Geometry<f64> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
                    let blob = crate::core::ewkb::write_ewkb(self, $srid)
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
                    IoWrite::write_all(out, &blob)?;
                    Ok(IsNull::No)
                }
            }

            impl<G: GeometryKind> FromSql<$sql_type, Pg> for $value<G> {
                fn from_sql(
                    bytes: <Pg as diesel::backend::Backend>::RawValue<'_>,
                ) -> deserialize::Result<Self> {
                    let geometry = <geo::Geometry<f64> as FromSql<$sql_type, Pg>>::from_sql(bytes)?;
                    Ok($value(G::from_geometry(geometry)?))
                }
            }

            impl<G: GeometryKind> ToSql<$sql_type, Pg> for $value<G> {
                fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
                    IoWrite::write_all(out, &self.0.to_ewkb($srid)?)?;
                    Ok(IsNull::No)
                }
            }
        };
    }

    impl_geo_geometry_pg!(Geometry, GeometryValue, None);
    impl_geo_geometry_pg!(Geography, GeographyValue, Some(4326));
}
