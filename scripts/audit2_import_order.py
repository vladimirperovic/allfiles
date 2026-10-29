"""Compare grouped/interleaved ordering of the same synthetic 20k-file export.

All four imports and optional SQL microbenchmarks use temporary generated
catalogs only. No existing user catalog is opened or modified.
"""
import argparse
import contextlib
import csv
import hashlib
import json
import pathlib
import platform
import sqlite3
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    args = parser.parse_args()
    binary = args.binary.resolve()
    expected_rows = sorted((f"Media {index % 100:03}", f"Folder {index % 13}",
                            f"file-{index:06}.bin", index + 1) for index in range(20000))
    expected_hash = hashlib.sha256(json.dumps(expected_rows).encode()).hexdigest()
    samples, queries = [], []
    with tempfile.TemporaryDirectory(prefix="allfiles-audit2-order-") as tmp:
        root = pathlib.Path(tmp)
        paths = {}
        for name, indexes in [("interleaved", list(range(20000))),
                              ("grouped", sorted(range(20000), key=lambda index: (index % 100, index)))]:
            path = root / (name + ".csv")
            paths[name] = path
            with path.open("w", encoding="utf-8", newline="") as stream:
                writer = csv.writer(stream)
                writer.writerow(["Filename", "Size", "Disk"])
                for index in indexes:
                    writer.writerow([f"Folder {index % 13}/file-{index:06}.bin", index + 1, f"Media {index % 100:03}"])
        for round_number, order in enumerate([["grouped", "interleaved"], ["interleaved", "grouped"]], 1):
            for name in order:
                data = root / f"{round_number}-{name}"
                data.mkdir()
                started = time.perf_counter()
                result = subprocess.run([str(binary), "import", "--data", str(data), "--file", str(paths[name])],
                                        capture_output=True, text=True, encoding="utf-8", timeout=90,
                                        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
                elapsed = time.perf_counter() - started
                assert result.returncode == 0, result.stderr[-1000:]
                with contextlib.closing(sqlite3.connect(data / "catalog.sqlite")) as connection:
                    rows = connection.execute("SELECT s.label,d.path,f.name,f.size FROM files f JOIN sources s ON s.id=f.source_id JOIN directories d ON d.id=f.dir_id ORDER BY s.label,d.path,f.name,f.size").fetchall()
                    digest = hashlib.sha256(json.dumps(rows).encode()).hexdigest()
                    totals = connection.execute("SELECT files,bytes,sources FROM catalog_stats").fetchone()
                assert digest == expected_hash, (name, round_number, digest)
                assert totals == (20000, 200010000, 100), totals
                samples.append({"round": round_number, "order": name, "seconds": elapsed,
                                "metadata_sha256": digest, "totals": list(totals)})
                print(name, round_number, round(elapsed, 4), flush=True)

        # Query shapes are compared on this generated catalog, using Python's
        # SQLite. These are SQL-level measurements, not Rust HTTP benchmarks.
        with contextlib.closing(sqlite3.connect(data / "catalog.sqlite")) as connection:
            # Isolate SQL/index shapes regardless of which indexes the tested
            # executable now creates. This is only our generated fixture.
            connection.execute("DROP INDEX IF EXISTS files_source_size")
            connection.execute("CREATE INDEX IF NOT EXISTS files_source ON files(source_id)")
            connection.commit()
            fields = "s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status"
            joined = f"SELECT {fields},COUNT(f.id),COALESCE(SUM(f.size),0) FROM (SELECT * FROM sources ORDER BY label,id LIMIT 100) s LEFT JOIN files f ON f.source_id=s.id GROUP BY s.id ORDER BY s.label,s.id"
            correlated = f"SELECT {fields},(SELECT COUNT(*) FROM files f WHERE f.source_id=s.id),(SELECT COALESCE(SUM(f.size),0) FROM files f WHERE f.source_id=s.id) FROM sources s ORDER BY s.label,s.id LIMIT 100"
            expected = connection.execute(joined).fetchall()
            for label, query in [("legacy_group_join", joined), ("correlated_counts", correlated),
                                 ("covering_group_join", joined), ("covering_correlated_counts", correlated)]:
                if label == "covering_group_join":
                    connection.execute("DROP INDEX files_source")
                    connection.execute("CREATE INDEX files_source_size ON files(source_id,size)")
                    connection.commit()
                elapsed = []
                for iteration in range(6):
                    started = time.perf_counter()
                    actual = connection.execute(query).fetchall()
                    duration = (time.perf_counter() - started) * 1000
                    assert actual == expected, label
                    if iteration:
                        elapsed.append(duration)
                queries.append({"name": label, "median_ms": statistics.median(elapsed), "samples_ms": elapsed,
                                "query_plan": [list(row) for row in connection.execute("EXPLAIN QUERY PLAN " + query)]})
    report = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "platform": platform.platform(), "fixture_files": 20000, "fixture_media": 100,
              "identical_records_verified": True, "rounds": samples,
              "median_seconds": {name: statistics.median(row["seconds"] for row in samples if row["order"] == name)
                                 for name in ["grouped", "interleaved"]},
              "sources_sql_microbench": {"sqlite_version": sqlite3.sqlite_version, "cases": queries,
                  "scope": "Warm-cache SQL on a generated temporary catalog; Python SQLite, not production Rust HTTP timings."},
              "scope": "Full CLI import wall time including parsing, syncs, startup and exit; four distinct temporary catalogs; reversed order in round two."}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["median_seconds"]))


if __name__ == "__main__":
    main()
