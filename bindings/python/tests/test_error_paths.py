"""Negative-path assertions for the DDL helpers.

These pin the user-visible error messages so behaviour drift in the
Rust-side validation is caught at PR time.
"""

from __future__ import annotations

import sqlite3

import pytest


@pytest.mark.parametrize(
    "sql, expected",
    [
        (
            "SELECT CreateSpatialIndex(NULL, 'geom')",
            "table name must not be NULL",
        ),
        (
            "SELECT CreateSpatialIndex('places', NULL)",
            "column name must not be NULL",
        ),
        (
            "SELECT CreateSpatialIndex('bad name', 'geom')",
            "invalid table name",
        ),
        (
            "SELECT CreateSpatialIndex('places', 'g-eom')",
            "invalid column name",
        ),
    ],
)
def test_create_rejects_bad_identifiers_and_nulls(
    places_conn: sqlite3.Connection, sql: str, expected: str
) -> None:
    with pytest.raises(sqlite3.OperationalError) as excinfo:
        places_conn.execute(sql)
    assert expected in str(excinfo.value)


def test_create_rejects_missing_table(conn: sqlite3.Connection) -> None:
    with pytest.raises(sqlite3.OperationalError):
        conn.execute("SELECT CreateSpatialIndex('does_not_exist', 'geom')")


def test_drop_rejects_bad_identifiers(places_conn: sqlite3.Connection) -> None:
    with pytest.raises(sqlite3.OperationalError) as excinfo:
        places_conn.execute("SELECT DropSpatialIndex(NULL, 'geom')")
    assert "table name must not be NULL" in str(excinfo.value)
