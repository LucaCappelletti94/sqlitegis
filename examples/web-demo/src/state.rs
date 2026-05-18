//! Default schema applied on first load (also re-applied via the "Reset DB"
//! button when the user wants to start over after editing the textarea).

pub const DEFAULT_SCHEMA_SQL: &str = "\
-- Schema for the geolite browser demo.
--
-- `geom` stores EWKB BLOBs (PostGIS wire format). Geodesic functions
-- (ST_DistanceSphere, ST_DWithinSphere) require SRID=4326 Point inputs,
-- which the loader produces via ST_Point(lon, lat, 4326).
DROP TABLE IF EXISTS places;
CREATE TABLE places (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    country    TEXT,
    population INTEGER,
    geom       BLOB NOT NULL
);

-- R-tree spatial index. KNN and bbox queries planned through the
-- `places_geolite_rtree` shadow table see ~50x speedups vs. full scan.
SELECT CreateSpatialIndex('places', 'geom');
";
