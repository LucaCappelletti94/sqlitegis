"""Smoke tests for the SQL scalar surface that ships with the wheel.

Each test calls one function group via SQL and checks the returned value.
The catalogue is huge; this suite picks representative members so that a
regression in the FFI wiring (NULL handling, return-setter, arg parsing)
is caught even without exhaustively asserting every function.
"""

from __future__ import annotations

import json
import math
import sqlite3


def test_constructors_and_text_serialiser(conn: sqlite3.Connection) -> None:
    row = conn.execute("SELECT ST_AsText(ST_Point(1.5, 2.5, 4326))").fetchone()
    assert row == ("POINT(1.5 2.5)",)

    row = conn.execute("SELECT ST_AsText(ST_MakeLine(ST_Point(0, 0), ST_Point(1, 1)))").fetchone()
    assert row == ("LINESTRING(0 0,1 1)",)


def test_geojson_round_trip(conn: sqlite3.Connection) -> None:
    row = conn.execute(
        "SELECT ST_AsGeoJSON(ST_GeomFromGeoJSON(?))",
        (json.dumps({"type": "Point", "coordinates": [10.0, 20.0]}),),
    ).fetchone()
    payload = json.loads(row[0])
    assert payload["type"] == "Point"
    assert payload["coordinates"] == [10.0, 20.0]


def test_wkt_round_trip_preserves_srid(conn: sqlite3.Connection) -> None:
    row = conn.execute("SELECT ST_SRID(ST_GeomFromText('POINT(1 2)', 4326))").fetchone()
    assert row == (4326,)


def test_accessors_on_point(conn: sqlite3.Connection) -> None:
    row = conn.execute(
        "SELECT ST_X(g), ST_Y(g), ST_IsEmpty(g), ST_GeometryType(g) "
        "FROM (SELECT ST_Point(7.0, 8.0, 4326) AS g)"
    ).fetchone()
    assert row == (7.0, 8.0, 0, "ST_Point")


def test_measurement_planar_distance(conn: sqlite3.Connection) -> None:
    (d,) = conn.execute("SELECT ST_Distance(ST_Point(0, 0), ST_Point(3, 4))").fetchone()
    assert math.isclose(d, 5.0)


def test_measurement_sphere_distance_is_metres(conn: sqlite3.Connection) -> None:
    # Roughly 111.3 km between (0,0) and (0,1) on the WGS84 sphere.
    (d,) = conn.execute(
        "SELECT ST_DistanceSphere(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326))"
    ).fetchone()
    assert 110_000 < d < 112_000


def test_predicates_intersects_and_dwithin_sphere(conn: sqlite3.Connection) -> None:
    (r,) = conn.execute("SELECT ST_Intersects(ST_Point(0, 0), ST_Point(0, 0))").fetchone()
    assert r == 1
    (r,) = conn.execute("SELECT ST_Intersects(ST_Point(0, 0), ST_Point(1, 1))").fetchone()
    assert r == 0
    (r,) = conn.execute(
        "SELECT ST_DWithinSphere(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326), 200000)"
    ).fetchone()
    assert r == 1
    (r,) = conn.execute(
        "SELECT ST_DWithinSphere(ST_Point(0, 0, 4326), ST_Point(0, 1, 4326), 50000)"
    ).fetchone()
    assert r == 0


def test_null_propagation_through_scalar_functions(conn: sqlite3.Connection) -> None:
    (r,) = conn.execute("SELECT ST_AsText(NULL)").fetchone()
    assert r is None
    (r,) = conn.execute("SELECT ST_X(NULL)").fetchone()
    assert r is None
    (r,) = conn.execute("SELECT ST_Distance(ST_Point(0, 0), NULL)").fetchone()
    assert r is None
    (r,) = conn.execute("SELECT ST_Point(NULL, 1.0)").fetchone()
    assert r is None


def test_buffer_returns_a_polygon(conn: sqlite3.Connection) -> None:
    (kind,) = conn.execute("SELECT ST_GeometryType(ST_Buffer(ST_Point(0, 0), 1.0))").fetchone()
    assert kind == "ST_Polygon"
