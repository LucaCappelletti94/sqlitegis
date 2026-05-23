"""Python entry point for the SQLiteGIS loadable extension.

The package ships a platform-specific ``libsqlitegis`` cdylib under
``sqlitegis/_bin/`` and exposes three functions:

- :func:`extension_path` -- absolute path to the bundled cdylib.
- :func:`register` -- enable loadable extensions on a ``sqlite3.Connection``
  and load SQLiteGIS into it.
- :func:`connect` -- ``sqlite3.connect`` wrapper that pre-registers SQLiteGIS.

The SQL surface (``ST_Point``, ``ST_AsText``, ``CreateSpatialIndex``,
``ST_DWithinSphere``, ...) is identical to the one documented on docs.rs at
https://docs.rs/sqlitegis. This module deliberately does not wrap any of
the spatial functions in Python; users call them through SQL strings.

Quick start::

    import sqlitegis
    conn = sqlitegis.connect(":memory:")
    cur = conn.cursor()
    cur.execute("SELECT ST_AsText(ST_Point(1.0, 2.0, 4326))")
    print(cur.fetchone()[0])  # POINT(1 2)
"""

from __future__ import annotations

import sqlite3
from importlib.metadata import PackageNotFoundError
from importlib.metadata import version as _pkg_version
from pathlib import Path
from typing import Any

try:
    __version__ = _pkg_version("sqlitegis")
except PackageNotFoundError:
    # In-tree imports (hatchling build sandbox, editable installs before
    # the dist-info has been generated) have no installed metadata yet.
    __version__ = "0.0.0+local"

__all__ = ["__version__", "connect", "extension_path", "register"]

_BIN = Path(__file__).parent / "_bin"
_CANDIDATES = ("libsqlitegis.so", "libsqlitegis.dylib", "sqlitegis.dll")


def extension_path() -> str:
    """Return the absolute path to the bundled SQLiteGIS cdylib.

    Raises ``FileNotFoundError`` if the wheel was built without the native
    binary (should never happen for a release wheel; can happen if you
    installed an sdist on an unsupported platform).
    """
    for name in _CANDIDATES:
        candidate = _BIN / name
        if candidate.is_file():
            return str(candidate)
    searched = "\n  ".join(str(_BIN / n) for n in _CANDIDATES)
    raise FileNotFoundError(
        "SQLiteGIS native extension was not found inside the installed "
        "wheel. Looked for:\n  " + searched
    )


def register(conn: sqlite3.Connection) -> None:
    """Load the SQLiteGIS spatial functions onto an existing connection.

    Enables ``enable_load_extension(True)`` on the connection first; this
    requires that the Python interpreter was built with loadable-extension
    support (this is the default on Debian and most Linux distributions, but
    not on every Python.org installer for macOS or Windows). If your
    interpreter raises an ``AttributeError`` here, switch to a Python build
    with extensions enabled (e.g. via the ``pysqlite3-binary`` package).
    """
    conn.enable_load_extension(True)
    conn.load_extension(extension_path())


def connect(*args: Any, **kwargs: Any) -> sqlite3.Connection:
    """Open a ``sqlite3`` connection with SQLiteGIS pre-registered.

    All positional and keyword arguments are forwarded to
    :func:`sqlite3.connect`. After the connection is open, :func:`register`
    is called on it before it is returned.
    """
    conn: sqlite3.Connection = sqlite3.connect(*args, **kwargs)
    register(conn)
    return conn
