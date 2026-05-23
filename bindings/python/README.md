# sqlitegis

PostGIS-style spatial functions for [SQLite](https://www.sqlite.org/), packaged as a loadable extension. Pre-built wheels of the [sqlitegis Rust crate](https://docs.rs/sqlitegis).

## Install

```sh
pip install sqlitegis
```

Wheels are published for Linux (`x86_64` + `aarch64`, glibc and musl), macOS (`x86_64` + `arm64`), and Windows (`x86_64`).

## Quick start

```python
import sqlitegis

# Open an in-memory connection with SQLiteGIS pre-registered.
conn = sqlitegis.connect(":memory:")

cur = conn.cursor()
cur.execute("CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB)")
cur.execute("SELECT CreateSpatialIndex('places', 'geom')")
cur.execute(
    "INSERT INTO places(id, geom) VALUES (1, ST_Point(?, ?, 4326))",
    (13.4, 52.5),
)

# Nearest neighbour by geodesic distance, R-tree-prefiltered.
cur.execute(
    """
    SELECT id, ST_AsText(geom)
    FROM places
    WHERE ST_DWithinSphere(geom, ST_Point(?, ?, 4326), ?)
    """,
    (13.5, 52.5, 100_000.0),
)
print(cur.fetchall())  # [(1, 'POINT(13.4 52.5)')]
```

If you already have a `sqlite3.Connection` and want to wire SQLiteGIS into it without using the helper `connect`:

```python
import sqlite3, sqlitegis

conn = sqlite3.connect("places.sqlite")
sqlitegis.register(conn)
```

## SQL surface

Every spatial function the README mentions (`ST_Point`, `ST_AsText`, `ST_DWithinSphere`, `ST_Intersects`, `CreateSpatialIndex`, etc.) is callable from SQL once SQLiteGIS is registered on the connection. See the [Rust crate documentation on docs.rs](https://docs.rs/sqlitegis) for the full catalogue; the function names and arities are identical.

## Loadable-extension support

`sqlitegis.register(conn)` calls `conn.enable_load_extension(True)`. This requires that your Python interpreter was built with loadable-extension support.

- Most distribution-managed Pythons (Debian, Ubuntu, Fedora, Homebrew, Alpine, conda-forge) enable extensions by default.
- `pysqlite3-binary` and `apsw` always support extensions.
- If `conn.enable_load_extension(True)` raises `AttributeError`, your interpreter was built without extension support. Either switch to a build that has it, or install `pysqlite3-binary` and use `import pysqlite3 as sqlite3` ahead of `sqlitegis.register(conn)`.

## License

`MIT OR Apache-2.0`, same as the Rust crate.
