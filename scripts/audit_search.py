"""Deterministic black-box search checks against an independent Python oracle.

Creates only temporary synthetic catalogs; never opens a user's catalog.
"""
import argparse
import concurrent.futures
import csv
import json
from pathlib import Path
import queue
import random
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request


def fold(value):
    return value.lower().translate(str.maketrans({"č": "c", "ć": "c", "š": "s", "ž": "z", "đ": "dj"}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    rng = random.Random(20261029)
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    checks, failures = [], []

    def record(name, expected, actual):
        passed = expected == actual
        checks.append({"name": name, "pass": passed})
        if not passed:
            failures.append({"name": name, "expected": expected, "actual": actual})

    with tempfile.TemporaryDirectory(prefix="allfiles-audit-search-") as directory:
        root = Path(directory)
        data, docs = root / "catalog", root / "documents"
        data.mkdir()
        docs.mkdir()
        export = root / "input.csv"
        words = ["Alpha", "beta", "gamma", "Čaša", "kuća", "Đuro", "žuto", "Šuma", "emoji🌊", "文件", "bracket[1]", "x*y", "x?y", "café"]
        with export.open("w", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream)
            writer.writerow(["Filename", "File Path", "Size", "Date Modified", "Disk"])
            for i in range(1600):
                writer.writerow([f"{rng.choice(words)}-{i:04d}.{rng.choice(['pdf', 'txt', 'bin'])}",
                                 f"Archive/{rng.choice(words)}/Folder {i % 23}", i % 97,
                                 f"2024-01-{i % 28 + 1:02d} 12:30:00", f"Disc {i % 4}"])
        start = time.perf_counter()
        imported = subprocess.run([binary, "import", "--data", str(data), "--file", str(export)],
                                  capture_output=True, text=True, encoding="utf-8", timeout=60)
        assert imported.returncode == 0, imported.stderr
        for i in range(40):
            text = "keywordalpha keywordbeta shared text 🌊 čaša" if i % 2 == 0 else "keywordgamma unique " + str(i)
            (docs / f"document-{i:03d}.txt").write_text(text, encoding="utf-8")
        scanned = subprocess.run([binary, "scan", "--data", str(data), "--root", str(docs), "--content"],
                                 capture_output=True, text=True, encoding="utf-8", timeout=60)
        assert scanned.returncode == 0, scanned.stderr
        preparation_seconds = time.perf_counter() - start
        connection = sqlite3.connect(data / "catalog.sqlite")
        connection.row_factory = sqlite3.Row
        rows = [dict(row) for row in connection.execute("SELECT f.id,f.name,d.path,f.source_id,f.extension,f.size,f.modified,f.search_name FROM files f JOIN directories d ON f.dir_id=d.id")]
        source_ids = sorted({row["source_id"] for row in rows})
        record("metadata count", 1640, len(rows))
        record("catalog integrity", [("ok",)], [tuple(r) for r in connection.execute("PRAGMA integrity_check")])
        record("catalog foreign keys", [], list(connection.execute("PRAGMA foreign_key_check")))
        actual_stats = tuple(connection.execute("SELECT files,bytes,documents,sources FROM catalog_stats").fetchone())
        expected_stats = tuple(connection.execute("SELECT COUNT(*),SUM(size),SUM(text_status IN ('indexed','truncated')),(SELECT COUNT(*) FROM sources) FROM files").fetchone())
        record("aggregate counters", expected_stats, actual_stats)
        connection.execute("INSERT INTO names(names,rank) VALUES('integrity-check',1)")
        connection.execute("INSERT INTO paths(paths,rank) VALUES('integrity-check',1)")
        connection.rollback()
        connection.close()
        content = sqlite3.connect(data / "content.sqlite")
        record("content integrity", [("ok",)], content.execute("PRAGMA integrity_check").fetchall())
        record("content deduplication", 21, content.execute("SELECT COUNT(*) FROM documents").fetchone()[0])
        record("content link count", 40, content.execute("SELECT COUNT(*) FROM links").fetchone()[0])
        content.execute("INSERT INTO fts(fts) VALUES('integrity-check')")
        content.rollback()
        content.close()

        server = subprocess.Popen([binary, "serve", "--data", str(data), "--port", "0", "--no-browser"],
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        lines = queue.Queue()
        def collect():
            for line in server.stdout:
                lines.put(line)
        threading.Thread(target=collect, daemon=True).start()
        base, token = None, None
        def request(path, body=None):
            headers = {"Content-Type": "application/json", "X-AllFiles-Token": token or ""}
            data_bytes = json.dumps(body).encode() if body is not None else None
            with opener.open(urllib.request.Request(base + path, data=data_bytes, headers=headers), timeout=30) as response:
                return json.load(response)
        try:
            for _ in range(6):
                line = lines.get(timeout=30)
                if line.startswith("Local: "):
                    base = line.strip()[7:]
                    break
            assert base, "Server did not announce local URL"
            token = request("/api/session")["token"]
            queries = ["", "a", "al", "alpha", "beta gamma", "casa", "ĐURO", "suma", "ŽUTO", "文件", "🌊", "[1]", "*", "?", '"Alpha"', "Archive Folder", "café", "missingneedle", "pdf", "bracket[", "]", "x*y", "x?y", "\talpha\n", '""']
            timings = []
            for number in range(540):
                query = queries[number % len(queries)]
                sort = ["name", "size", "modified"][number % 3]
                source = rng.choice(["", "", *source_ids, "not-present"])
                extension = rng.choice(["", "", "pdf", ".TXT", "bin", "missing"])
                offset = rng.choice([0, 1, 15, 100, 2000])
                limit = rng.choice([1, 17, 60, 200])
                terms = [fold(t) for t in query.replace('"', '').split()]
                expected = [row for row in rows if (not source or row["source_id"] == source)
                            and (not extension or row["extension"] == extension.lstrip('.').lower())
                            and all(term in fold(row["name"]) or term in fold(row["path"]) for term in terms)]
                key = ((lambda row: (-row["size"], row["id"])) if sort == "size" else
                       (lambda row: (-row["modified"], row["id"])) if sort == "modified" else
                       (lambda row: (fold(row["name"]), row["id"])))
                expected.sort(key=key)
                params = dict(q=query, mode="files", source=source, ext=extension, sort=sort, offset=offset, limit=limit)
                before = time.perf_counter()
                actual = request("/api/search?" + urllib.parse.urlencode(params))
                timings.append((time.perf_counter() - before) * 1000)
                record(f"file search {number}: {params}",
                       (len(expected), [r["id"] for r in expected[offset:offset+limit]]),
                       (actual["total"], [r["id"] for r in actual["results"]]))
            for query, expected in [("keywordalpha", 20), ("keywordgamma", 20), ("keywordalpha keywordbeta", 20),
                                    ('"keywordalpha keywordbeta"', 20), ('"keywordbeta keywordalpha"', 0), ("casa", 20), ("", 40)]:
                result = request("/api/search?" + urllib.parse.urlencode(dict(q=query, mode="content", limit=100)))
                record("content " + query, expected, result["total"])
            result = request("/api/search?q=keywordalpha&mode=content")
            selected = result["results"][0]["id"]
            text = "keywordalpha keywordbeta shared text 🌊 čaša"
            for offset, limit in [(0, 1), (36, 1), (37, 2), (len(text), 10), (1000, 5), (0, 100)]:
                result = request(f"/api/file?id={selected}&text_offset={offset}&max_chars={limit}")
                record(f"unicode text offset {offset} limit {limit}", text[offset:offset+limit], result["text"])
                record(f"unicode has_more {offset}/{limit}", offset + limit < len(text), result["has_more"])
            # Independent clients, real local HTTP, repeated content/file/status reads.
            def concurrent_read(i):
                if i % 3 == 0:
                    return request("/api/status")["files"] == 1640
                if i % 3 == 1:
                    return request("/api/search?q=keywordalpha&mode=content")["total"] == 20
                return request("/api/search?offset=2000")["results"] == []
            with concurrent.futures.ThreadPoolExecutor(max_workers=12) as pool:
                record("96 concurrent reads on 12 clients", True, all(pool.map(concurrent_read, range(96))))
        finally:
            if base and token and server.poll() is None:
                try:
                    request("/api/shutdown", {})
                except Exception:
                    pass
            try:
                server.wait(timeout=15)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
            record("clean server shutdown", 0, server.returncode)
        report = {"seed": 20261029, "synthetic_files": 1640, "preparation_seconds": preparation_seconds,
                  "checks": len(checks), "passed": sum(c["pass"] for c in checks), "failures": failures,
                  "search_http_median_ms": sorted(timings)[len(timings)//2],
                  "scope": "Windows local HTTP; synthetic inputs; independent substring/sort/filter oracle; content, Unicode, integrity and concurrent reads"}
    output = json.dumps(report, ensure_ascii=False, indent=2) + "\n"
    print(output)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(output, encoding="utf-8")
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
