"""Measure stats initialization on a temporary copy of a synthetic catalog.

The input must be a synthetic fixture, not a live user catalog. This measures
the SQL statements with Python's SQLite, not the full Rust application's
startup. The source is never opened for writes and its SHA-256 is checked
before and after. All database mutations happen inside TemporaryDirectory.
"""
import argparse
import contextlib
import hashlib
import json
import pathlib
import platform
import shutil
import sqlite3
import statistics
import tempfile
import time


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data", required=True, type=pathlib.Path,
                        help="Synthetic fixture directory containing catalog.sqlite")
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    source = (args.data / "catalog.sqlite").resolve()
    schema = pathlib.Path(__file__).resolve().parents[1] / "src" / "sql" / "stats.sql"
    sql = schema.read_text(encoding="utf-8")
    before = sha256(source)
    with tempfile.TemporaryDirectory(prefix="allfiles-stats-audit-") as tmp:
        copied = pathlib.Path(tmp) / "catalog.sqlite"
        shutil.copy2(source, copied)
        if sha256(copied) != before:
            raise RuntimeError("Fixture changed while it was being copied")
        with contextlib.closing(sqlite3.connect(copied)) as connection:
            # Ensure the measured runs all start with the cached stat row present.
            connection.executescript(sql)
            files = connection.execute("SELECT files FROM catalog_stats WHERE id=1").fetchone()[0]
            steps = [0]

            def progress():
                steps[0] += 1000
                return 0

            samples = []
            for _ in range(5):
                steps[0] = 0
                connection.set_progress_handler(progress, 1000)
                started = time.perf_counter()
                connection.executescript(sql)
                elapsed = time.perf_counter() - started
                connection.set_progress_handler(None, 0)
                samples.append({"seconds": elapsed, "vm_steps_at_least": steps[0]})
            guarded = []
            for _ in range(5):
                started = time.perf_counter()
                exists = connection.execute("SELECT 1 FROM catalog_stats WHERE id=1").fetchone()
                if not exists:
                    connection.executescript(sql)
                guarded.append(time.perf_counter() - started)
    after = sha256(source)
    report = {
        "fixture": str(source), "fixture_files": files,
        "fixture_untouched": before == after,
        "sha256_before": before, "sha256_after": after,
        "platform": platform.platform(), "python_sqlite_version": sqlite3.sqlite_version,
        "stats_sql_reinitialization": samples,
        "median_reinitialization_seconds": statistics.median(row["seconds"] for row in samples),
        "existence_guard_seconds": guarded,
        "median_guard_seconds": statistics.median(guarded),
        "cached_initialization_bounded": all(row["vm_steps_at_least"] < max(10000, files // 2) for row in samples),
        "scope": "Exact current src/sql/stats.sql on a temporary copy of the synthetic fixture. "
                 "Warm-cache SQL-level timing using Python SQLite, not production Rust startup timing.",
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    if before != after:
        raise RuntimeError("Input fixture hash changed during the benchmark")
    if files >= 10000 and not report["cached_initialization_bounded"]:
        raise AssertionError("Cached statistics initialization still scans the catalog")


if __name__ == "__main__":
    main()
