//! Diesel SQL type definitions and `FromSql` / `ToSql` implementations.
//!
//! Both `Geometry` and `Geography` map to `Binary` (BLOB) in SQLite and
//! to PostGIS's native `geometry` / `geography` types in PostgreSQL,
//! storing EWKB-encoded geometry.

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
    // SRID arg: Geometry writes no SRID and accepts any; Geography forces
    // 4326 on write and requires 4326 on read.

    macro_rules! impl_geo_geometry_sqlite {
        ($sql_type:ty, $srid:expr) => {
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
        };
    }

    impl_geo_geometry_sqlite!(Geometry, None);
    impl_geo_geometry_sqlite!(Geography, Some(4326));
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
        ($sql_type:ty, $srid:expr) => {
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
        };
    }

    impl_geo_geometry_pg!(Geometry, None);
    impl_geo_geometry_pg!(Geography, Some(4326));
}
