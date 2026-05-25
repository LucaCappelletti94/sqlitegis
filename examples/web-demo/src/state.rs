//! Default schema applied on first load (also re-applied via the "Reset DB"
//! button when the user wants to start over after editing the textarea).

pub const DEFAULT_SCHEMA_SQL: &str = "\
-- `geom` holds EWKB BLOBs. ST_*Sphere needs SRID=4326 Points.
DROP TABLE IF EXISTS places;
CREATE TABLE places (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL,
  country    TEXT,
  population INTEGER,
  geom       BLOB NOT NULL
);
-- The R-tree shadow `places_geom_rtree` (KNN/bbox queries ~50x faster) is
-- built by CreateSpatialIndex AFTER the dataset finishes loading. Creating it
-- up front would fire the index-maintenance trigger on every one of the 68k
-- inserts; deferring it lets the index populate in a single set-based pass and
-- keeps the bulk load fast.";
