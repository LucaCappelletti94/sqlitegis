"""Performance assertions for `CreateSpatialIndex`.

These tests do two things the rest of the suite does not:

1. Generate a non-trivial dataset (a few tens of thousands of WGS84 points)
   so the unindexed cost is measurable.
2. Time an unindexed `ST_Intersects` (or `ST_DWithinSphere`) scan against
   the equivalent R-tree-prefiltered query and assert the indexed path
   is materially faster.

The threshold is intentionally conservative: hand-measured speedup at
20k rows on a Threadripper is several hundred x; we assert only 10x so
CI runner contention cannot reasonably flip the result. If this test
ever flakes below 10x in CI, something is wrong with the index, not
with the timing.

Warm-up: each query runs twice. The first call primes SQLite's page
cache and the per-statement compiled plan; the second call is the one
we time. This removes most cold-cache variance for the small dataset
sizes we use here.
"""

from __future__ import annotations

import random
import sqlite3
import time

import pytest

# Seed the dataset deterministically so timings are reproducible across
# runs. The actual distribution does not matter for the ratio assertion,
# only that the dataset is dense enough for the unindexed scan to be
# meaningfully slower than the rtree-prefiltered path.
RNG_SEED = 0
POINT_COUNT = 20_000

# Minimum speedup factor that the indexed path must show over the
# unindexed scan. Real-world ratios are 100x+ for this dataset size;
# 10x leaves ~10x headroom for CI-runner contention.
MIN_SPEEDUP = 10.0


def _seed_points(conn: sqlite3.Connection, n: int) -> None:
    rng = random.Random(RNG_SEED)
    rows = [(f"POINT({rng.uniform(-180, 180)} {rng.uniform(-90, 90)})",) for _ in range(n)]
    conn.executescript("CREATE TABLE places (id INTEGER PRIMARY KEY, geom BLOB);")
    conn.executemany("INSERT INTO places(geom) VALUES (ST_GeomFromText(?, 4326))", rows)


def _time_query(
    conn: sqlite3.Connection, sql: str, params: tuple[object, ...]
) -> tuple[float, int]:
    """Run the query once to warm caches, then time the second run.

    Returns (elapsed_seconds, row_count) so the caller can also assert
    that both paths agree on the result set.
    """
    # Warm-up: prime page cache + statement cache.
    n_warm = conn.execute(sql, params).fetchone()[0]
    start = time.perf_counter()
    n_real = conn.execute(sql, params).fetchone()[0]
    elapsed = time.perf_counter() - start
    assert n_real == n_warm, "warm-up and timed run disagreed"
    return elapsed, n_real


def test_indexed_intersects_is_at_least_10x_faster_than_unindexed(
    conn: sqlite3.Connection,
) -> None:
    _seed_points(conn, POINT_COUNT)

    # Small 1deg x 1deg window. Expected hits: roughly (1/360) * (1/180) * N,
    # which is single-digit for the seeded distribution.
    window = "POLYGON((10 20, 11 20, 11 21, 10 21, 10 20))"
    unindexed_sql = (
        "SELECT COUNT(*) FROM places WHERE ST_Intersects(geom, ST_GeomFromText(?, 4326))"
    )

    t_unindexed, n_unindexed = _time_query(conn, unindexed_sql, (window,))

    # Build the index. We do not assert on its build time; only on the
    # subsequent query speedup.
    conn.execute("SELECT CreateSpatialIndex('places', 'geom')")

    indexed_sql = (
        "SELECT COUNT(*) FROM places p "
        "JOIN places_geom_rtree r ON p.rowid = r.id "
        "WHERE r.xmin <= 11 AND r.xmax >= 10 AND r.ymin <= 21 AND r.ymax >= 20 "
        "  AND ST_Intersects(p.geom, ST_GeomFromText(?, 4326))"
    )
    t_indexed, n_indexed = _time_query(conn, indexed_sql, (window,))

    # Correctness gate: indexed path must not silently lose or gain rows.
    assert n_indexed == n_unindexed, (
        f"indexed/unindexed row counts disagree: {n_indexed} vs {n_unindexed}"
    )

    # Performance gate.
    speedup = t_unindexed / max(t_indexed, 1e-9)
    assert speedup >= MIN_SPEEDUP, (
        f"R-tree-prefiltered query was only {speedup:.1f}x faster than the "
        f"unindexed scan (need >= {MIN_SPEEDUP}x). "
        f"t_unindexed={t_unindexed * 1000:.2f} ms, "
        f"t_indexed={t_indexed * 1000:.2f} ms, "
        f"matches={n_indexed}"
    )


def test_indexed_dwithin_sphere_is_at_least_10x_faster_than_unindexed(
    conn: sqlite3.Connection,
) -> None:
    _seed_points(conn, POINT_COUNT)

    # 50 km radius around a point in the Atlantic. The radius is small
    # enough that the rtree window prunes the vast majority of rows.
    center_x, center_y, radius_m = 0.0, 0.0, 50_000.0
    unindexed_sql = (
        "SELECT COUNT(*) FROM places WHERE ST_DWithinSphere(geom, ST_Point(?, ?, 4326), ?)"
    )

    t_unindexed, n_unindexed = _time_query(conn, unindexed_sql, (center_x, center_y, radius_m))

    conn.execute("SELECT CreateSpatialIndex('places', 'geom')")

    # 50 km in latitude is ~0.45 deg; in longitude near the equator also
    # ~0.45 deg. Add a small fudge factor to keep the rtree window a
    # superset of every point that could be in the geodesic radius.
    half_deg = 0.6
    indexed_sql = (
        "SELECT COUNT(*) FROM places p "
        "JOIN places_geom_rtree r ON p.rowid = r.id "
        f"WHERE r.xmin <= {center_x + half_deg} AND r.xmax >= {center_x - half_deg} "
        f"  AND r.ymin <= {center_y + half_deg} AND r.ymax >= {center_y - half_deg} "
        "  AND ST_DWithinSphere(p.geom, ST_Point(?, ?, 4326), ?)"
    )
    t_indexed, n_indexed = _time_query(conn, indexed_sql, (center_x, center_y, radius_m))

    assert n_indexed == n_unindexed, (
        f"indexed/unindexed row counts disagree: {n_indexed} vs {n_unindexed}"
    )

    speedup = t_unindexed / max(t_indexed, 1e-9)
    assert speedup >= MIN_SPEEDUP, (
        f"R-tree-prefiltered DWithinSphere was only {speedup:.1f}x faster "
        f"than the unindexed scan (need >= {MIN_SPEEDUP}x). "
        f"t_unindexed={t_unindexed * 1000:.2f} ms, "
        f"t_indexed={t_indexed * 1000:.2f} ms, "
        f"matches={n_indexed}"
    )


@pytest.mark.parametrize("n", [1_000, 10_000])
def test_create_spatial_index_completes_under_a_few_seconds(
    conn: sqlite3.Connection, n: int
) -> None:
    """Sanity bound on CreateSpatialIndex build time.

    Not a tight benchmark; just guards against a regression that turns
    the catalogue traversal into something accidentally quadratic.
    Build time at 200k rows on a workstation is ~1.5 s, so 5 s for the
    largest size here is a generous ceiling.
    """
    _seed_points(conn, n)
    start = time.perf_counter()
    conn.execute("SELECT CreateSpatialIndex('places', 'geom')")
    elapsed = time.perf_counter() - start
    assert elapsed < 5.0, f"CreateSpatialIndex on {n} rows took {elapsed:.2f} s (>= 5 s ceiling)"
