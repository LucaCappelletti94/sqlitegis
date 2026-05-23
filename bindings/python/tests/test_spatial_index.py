"""Round-trip tests for CreateSpatialIndex and DropSpatialIndex.

The wheel ships the same R-tree maintenance machinery the Rust crate
documents on docs.rs. These tests pin the externally observable
behaviour: managed object names, catalog row, trigger maintenance on
INSERT/UPDATE/DELETE, the indexed query plan, and a clean drop.
"""

from __future__ import annotations

import sqlite3

import pytest


def _query_plan(conn: sqlite3.Connection, sql: str, params: tuple[object, ...] = ()) -> list[str]:
    return [row[3] for row in conn.execute("EXPLAIN QUERY PLAN " + sql, params).fetchall()]


def _managed_objects(conn: sqlite3.Connection, prefix: str) -> list[tuple[str, str]]:
    return list(
        conn.execute(
            "SELECT type, name FROM sqlite_master WHERE name LIKE ? ORDER BY name",
            (f"{prefix}%",),
        ).fetchall()
    )


def test_create_spatial_index_builds_rtree_catalog_and_triggers(
    places_conn: sqlite3.Connection,
) -> None:
    (rc,) = places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')").fetchone()
    assert rc == 1

    objects = _managed_objects(places_conn, "places_geom")
    names = {name for _, name in objects}
    assert "places_geom_rtree" in names
    assert "places_geom_insert" in names
    assert "places_geom_update" in names
    assert "places_geom_delete" in names

    catalog_row = places_conn.execute(
        "SELECT prefix, table_name, column_name FROM sqlitegis_spatial_index_catalog"
    ).fetchone()
    assert catalog_row == ("places_geom", "places", "geom")

    (n,) = places_conn.execute("SELECT COUNT(*) FROM places_geom_rtree").fetchone()
    assert n == 3


def test_create_is_idempotent(places_conn: sqlite3.Connection) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    (rc,) = places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')").fetchone()
    assert rc == 1
    # Still exactly the 3 seeded rows.
    (n,) = places_conn.execute("SELECT COUNT(*) FROM places_geom_rtree").fetchone()
    assert n == 3


def test_insert_trigger_propagates_to_rtree(
    places_conn: sqlite3.Connection,
) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    places_conn.execute(
        "INSERT INTO places(name, geom) VALUES (?, ST_GeomFromText(?, 4326))",
        ("d", "POINT(15 25)"),
    )
    (n,) = places_conn.execute("SELECT COUNT(*) FROM places_geom_rtree").fetchone()
    assert n == 4


def test_update_trigger_moves_rtree_entry(places_conn: sqlite3.Connection) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    (rowid,) = places_conn.execute("SELECT rowid FROM places WHERE name = 'a'").fetchone()
    places_conn.execute(
        "UPDATE places SET geom = ST_GeomFromText(?, 4326) WHERE name = 'a'",
        ("POINT(100 100)",),
    )
    bounds = places_conn.execute(
        "SELECT xmin, xmax, ymin, ymax FROM places_geom_rtree WHERE id = ?",
        (rowid,),
    ).fetchone()
    assert bounds == (100.0, 100.0, 100.0, 100.0)


def test_delete_trigger_removes_rtree_entry(places_conn: sqlite3.Connection) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    (rowid,) = places_conn.execute("SELECT rowid FROM places WHERE name = 'b'").fetchone()
    places_conn.execute("DELETE FROM places WHERE name = 'b'")
    row = places_conn.execute("SELECT 1 FROM places_geom_rtree WHERE id = ?", (rowid,)).fetchone()
    assert row is None


def test_indexed_window_query_uses_the_rtree(
    places_conn: sqlite3.Connection,
) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    sql = (
        "SELECT p.name FROM places p "
        "JOIN places_geom_rtree r ON p.rowid = r.id "
        "WHERE r.xmin <= ? AND r.xmax >= ? AND r.ymin <= ? AND r.ymax >= ?"
    )
    params = (35.0, 25.0, 45.0, 35.0)  # window around POINT(30 40)
    plan = _query_plan(places_conn, sql, params)
    assert any("VIRTUAL TABLE" in detail and "r" in detail for detail in plan), plan
    assert any("SEARCH p USING INTEGER PRIMARY KEY" in detail for detail in plan), plan

    matches = [row[0] for row in places_conn.execute(sql, params).fetchall()]
    assert matches == ["b"]


def test_indexed_and_unindexed_results_agree(
    places_conn: sqlite3.Connection,
) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    window = "POLYGON((25 35, 35 35, 35 45, 25 45, 25 35))"
    unindexed = sorted(
        row[0]
        for row in places_conn.execute(
            "SELECT name FROM places WHERE ST_Intersects(geom, ST_GeomFromText(?, 4326))",
            (window,),
        ).fetchall()
    )
    indexed = sorted(
        row[0]
        for row in places_conn.execute(
            "SELECT p.name FROM places p "
            "JOIN places_geom_rtree r ON p.rowid = r.id "
            "WHERE r.xmin <= 35 AND r.xmax >= 25 AND r.ymin <= 45 AND r.ymax >= 35 "
            "  AND ST_Intersects(p.geom, ST_GeomFromText(?, 4326))",
            (window,),
        ).fetchall()
    )
    assert unindexed == indexed == ["b"]


def test_drop_spatial_index_removes_every_managed_object(
    places_conn: sqlite3.Connection,
) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    (rc,) = places_conn.execute("SELECT DropSpatialIndex('places', 'geom')").fetchone()
    assert rc == 1

    objects = _managed_objects(places_conn, "places_geom")
    assert objects == []
    (n,) = places_conn.execute(
        "SELECT COUNT(*) FROM sqlitegis_spatial_index_catalog WHERE prefix = 'places_geom'"
    ).fetchone()
    assert n == 0


def test_drop_on_absent_index_is_a_no_op(places_conn: sqlite3.Connection) -> None:
    (rc,) = places_conn.execute("SELECT DropSpatialIndex('places', 'geom')").fetchone()
    assert rc == 1


def test_create_then_drop_then_create_round_trips(
    places_conn: sqlite3.Connection,
) -> None:
    places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    places_conn.execute("SELECT DropSpatialIndex('places', 'geom')")
    (rc,) = places_conn.execute("SELECT CreateSpatialIndex('places', 'geom')").fetchone()
    assert rc == 1
    (n,) = places_conn.execute("SELECT COUNT(*) FROM places_geom_rtree").fetchone()
    assert n == 3


def test_create_rejects_without_rowid_tables(conn: sqlite3.Connection) -> None:
    conn.executescript("CREATE TABLE wr (k TEXT PRIMARY KEY, geom BLOB) WITHOUT ROWID;")
    with pytest.raises(sqlite3.OperationalError) as excinfo:
        conn.execute("SELECT CreateSpatialIndex('wr', 'geom')")
    assert "WITHOUT ROWID" in str(excinfo.value)
