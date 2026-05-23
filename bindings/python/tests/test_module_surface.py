"""Test the public sqlitegis Python surface: extension_path, register, connect."""

from __future__ import annotations

import importlib.metadata
import os
import sqlite3
from pathlib import Path

import pytest

import sqlitegis


def test_version_attribute_matches_installed_metadata() -> None:
    assert isinstance(sqlitegis.__version__, str)
    assert sqlitegis.__version__
    assert sqlitegis.__version__ == importlib.metadata.version("sqlitegis")


def test_version_falls_back_when_metadata_is_missing(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """If the installed dist-info is gone (in-tree import before build),
    __version__ falls back to a marker rather than crashing on import."""
    import importlib

    def _raise(name: str) -> str:
        raise importlib.metadata.PackageNotFoundError(name)

    monkeypatch.setattr(importlib.metadata, "version", _raise)
    try:
        importlib.reload(sqlitegis)
        assert sqlitegis.__version__ == "0.0.0+local"
    finally:
        # Restore the real metadata-derived __version__ for subsequent tests.
        monkeypatch.undo()
        importlib.reload(sqlitegis)


def test_extension_path_exists() -> None:
    path = sqlitegis.extension_path()
    assert os.path.isfile(path), path
    assert os.path.getsize(path) > 0
    assert path.endswith((".so", ".dylib", ".dll"))


def test_extension_path_raises_when_binary_missing(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    """If the wheel was packaged without the cdylib, extension_path must
    raise FileNotFoundError with the searched paths in the message."""
    empty_bin = tmp_path / "empty_bin"
    empty_bin.mkdir()
    monkeypatch.setattr(sqlitegis, "_BIN", empty_bin)
    with pytest.raises(FileNotFoundError) as excinfo:
        sqlitegis.extension_path()
    msg = str(excinfo.value)
    assert "libsqlitegis.so" in msg
    assert "libsqlitegis.dylib" in msg
    assert "sqlitegis.dll" in msg


def test_register_loads_scalars_into_an_existing_connection() -> None:
    conn = sqlite3.connect(":memory:")
    try:
        with pytest.raises(sqlite3.OperationalError):
            conn.execute("SELECT ST_AsText(ST_Point(1.0, 2.0, 4326))")
        sqlitegis.register(conn)
        row = conn.execute("SELECT ST_AsText(ST_Point(1.0, 2.0, 4326))").fetchone()
        assert row == ("POINT(1 2)",)
    finally:
        conn.close()


def test_connect_returns_a_ready_connection() -> None:
    conn = sqlitegis.connect(":memory:")
    try:
        row = conn.execute("SELECT ST_AsText(ST_Point(3.0, 4.0))").fetchone()
        assert row == ("POINT(3 4)",)
    finally:
        conn.close()


def test_connect_forwards_kwargs_to_sqlite3(tmp_path: Path) -> None:
    db_path = tmp_path / "places.sqlite"
    conn = sqlitegis.connect(str(db_path), isolation_level=None)
    try:
        assert conn.isolation_level is None
        conn.execute("CREATE TABLE t (g BLOB)")
        conn.execute("INSERT INTO t VALUES (ST_Point(0, 0, 4326))")
        row = conn.execute("SELECT ST_X(g), ST_Y(g) FROM t").fetchone()
        assert row == (0.0, 0.0)
    finally:
        conn.close()
    assert db_path.is_file()
