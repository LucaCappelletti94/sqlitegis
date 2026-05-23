"""Shared fixtures for the sqlitegis Python test suite.

The tests exercise the wheel through its public surface only: import
``sqlitegis``, open a connection via ``sqlitegis.connect`` or call
``sqlitegis.register`` on a plain ``sqlite3.connect`` result. Anything
the suite asserts about ``CreateSpatialIndex`` and friends is what
end users will hit when they install the wheel and load it from SQL.
"""

from __future__ import annotations

import sqlite3
from collections.abc import Iterator

import pytest

import sqlitegis


def _interpreter_supports_loadable_extensions() -> bool:
    conn = sqlite3.connect(":memory:")
    try:
        conn.enable_load_extension(True)
        return True
    except AttributeError:
        return False
    finally:
        conn.close()


if not _interpreter_supports_loadable_extensions():
    pytest.skip(
        "This Python build does not support sqlite3.Connection.enable_load_extension; "
        "skipping the entire sqlitegis test suite (install pysqlite3-binary or use a "
        "Python built with --enable-loadable-sqlite-extensions).",
        allow_module_level=True,
    )


@pytest.fixture
def conn() -> Iterator[sqlite3.Connection]:
    """An in-memory connection with sqlitegis pre-registered."""
    c = sqlitegis.connect(":memory:")
    try:
        yield c
    finally:
        c.close()


@pytest.fixture
def places_conn(conn: sqlite3.Connection) -> sqlite3.Connection:
    """A connection seeded with a small `places` table of WGS84 points."""
    conn.executescript("CREATE TABLE places (id INTEGER PRIMARY KEY, name TEXT, geom BLOB);")
    rows = [
        ("a", "POINT(10 20)"),
        ("b", "POINT(30 40)"),
        ("c", "POINT(50 60)"),
    ]
    conn.executemany(
        "INSERT INTO places(name, geom) VALUES (?, ST_GeomFromText(?, 4326))",
        rows,
    )
    return conn
