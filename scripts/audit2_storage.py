"""Second-pass storage lifecycle and optional performance audit.

Uses only synthetic data in TemporaryDirectory. Fault injection, including
missing/corrupt content databases, affects temporary copies only.
"""
import argparse
import contextlib
import csv
import hashlib
import http.client
import json
import pathlib
import platform
import queue
import random
import shutil
import sqlite3
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.parse


class App:
    def __init__(self, binary, data):
        self.binary, self.data = binary, data

    def __enter__(self):
        self.process = subprocess.Popen(
            [str(self.binary), "serve", "--data", str(self.data), "--port", "0", "--no-browser"],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8",
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
        lines = queue.Queue()
        threading.Thread(target=lambda: [lines.put(line) for line in self.process.stdout], daemon=True).start()
        self.errors = []
        threading.Thread(target=lambda: self.errors.extend(self.process.stderr), daemon=True).start()
        self.connection = None
        try:
            for _ in range(10):
                line = lines.get(timeout=20)
                if line.startswith("Local: "):
                    self.base = line.strip()[7:]
                    break
            self.connection = http.client.HTTPConnection("127.0.0.1", urllib.parse.urlsplit(self.base).port, timeout=60)
            self.token = self.request("/api/session")["token"]
            return self
        except BaseException:
            self.process.kill()
            self.process.wait(timeout=10)
            raise

    def request(self, path, body=None, expected=200):
        headers = {}
        encoded = None
        if body is not None:
            headers = {"Content-Type": "application/json", "X-AllFiles-Token": self.token}
            encoded = json.dumps(body).encode()
        self.connection.request("POST" if body is not None else "GET", path, body=encoded, headers=headers)
        response = self.connection.getresponse()
        payload = json.loads(response.read())
        if expected is None:
            return response.status, payload
        assert response.status == expected, (path, response.status, payload)
        return payload

    def search(self, query="", mode="files", **options):
        return self.request("/api/search?" + urllib.parse.urlencode({"q": query, "mode": mode, "limit": 200, **options}))

    def __exit__(self, *unused):
        try:
            if self.process.poll() is None:
                self.request("/api/shutdown", {})
        finally:
            if self.connection:
                self.connection.close()
            try:
                self.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        assert self.process.returncode == 0, self.errors


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=pathlib.Path, required=True)
    parser.add_argument("--output", type=pathlib.Path, required=True)
    parser.add_argument("--bench", action="store_true", help="Include 40 x 512 KiB text and 100k-file/250-media performance fixtures")
    args = parser.parse_args()
    binary = args.binary.resolve()
    cases, benchmarks = [], []

    def record(name, expected, actual, **extra):
        passed = expected == actual
        cases.append({"name": name, "passed": passed, "expected": expected, "actual": actual, **extra})
        print(("PASS " if passed else "FAIL ") + name, flush=True)

    def cli(data, *arguments, require_success=True):
        process = subprocess.run([str(binary), *arguments, "--data", str(data)], capture_output=True,
                                 text=True, encoding="utf-8", timeout=120,
                                 creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        if require_success:
            assert process.returncode == 0, process.stderr[-2000:]
        return {"exit": process.returncode, "stdout": process.stdout[-1000:], "stderr": process.stderr[-1000:]}

    def sql(data, database, statement):
        with contextlib.closing(sqlite3.connect((data / database).as_uri() + "?mode=ro", uri=True)) as connection:
            return [list(row) for row in connection.execute(statement)]

    def available_status(app):
        status = app.request("/api/status")
        found = app.search()
        details = {str(row["id"]): app.request(f'/api/file?id={row["id"]}&max_chars=20000') for row in found["results"]}
        searchable = app.search(mode="content")
        return {"statistics_documents": status["documents"], "content_total": searchable["total"],
                "row_statuses": [row["text_status"] for row in found["results"]],
                "detail_statuses": [details[str(row["id"])]["text_status"] for row in found["results"]]}

    with tempfile.TemporaryDirectory(prefix="allfiles-audit2-storage-") as tmp:
        root = pathlib.Path(tmp)
        data, docs_a, docs_b = root / "data", root / "source-a", root / "source-b"
        for directory in (data, docs_a, docs_b):
            directory.mkdir()
        inputs = [(docs_a, "nameonlyneedle.txt", "commonneedle sharedalpha text 🌊"),
                  (docs_a, "contentonly.txt", "contentneedle uniquealpha"),
                  (docs_a, "shared.txt", "dedupneedle body"),
                  (docs_b, "shared.txt", "dedupneedle body"),
                  (docs_b, "bothneedle.txt", "bothneedle body commonneedle")]
        for directory, name, text in inputs:
            (directory / name).write_text(text, encoding="utf-8")
        for directory, label in [(docs_a, "A"), (docs_b, "B")]:
            cli(data, "scan", "--root", str(directory), "--label", label, "--content")
        record("cross_source_dedup", [[4]], sql(data, "content.sqlite", "SELECT count(*) FROM documents"))
        record("cross_source_links", [[5]], sql(data, "content.sqlite", "SELECT count(*) FROM links"))
        metadata = sql(data, "catalog.sqlite", "SELECT f.id,s.label,f.name FROM files f JOIN sources s ON s.id=f.source_id")
        body_by_id = {item[0]: next(text for directory, name, text in inputs if name == item[2] and ("A" if directory == docs_a else "B") == item[1]) for item in metadata}
        with App(binary, data) as app:
            record("initial_document_counts", {"statistics_documents": 5, "content_total": 5,
                    "row_statuses": ["indexed"] * 5, "detail_statuses": ["indexed"] * 5}, available_status(app))
            for term in ["commonneedle", "nameonlyneedle", "contentneedle", "dedupneedle", "bothneedle", "missingneedle"]:
                names = {item[0] for item in metadata if term in item[2]}
                contents = {identifier for identifier, text in body_by_id.items() if term in text}
                for mode, expected in [("files", names), ("content", contents), ("all", names | contents)]:
                    result = app.search(term, mode)
                    record(f"mode_union_{mode}_{term}", sorted(expected), sorted(row["id"] for row in result["results"]))
            for identifier, text in body_by_id.items():
                record(f"file_text_{identifier}", text, app.request(f"/api/file?id={identifier}&max_chars=20000")["text"])
            full, meta = root / "full-export", root / "metadata-export"
            app.request("/api/export", {"destination": str(full), "include_content": True})
            app.request("/api/export", {"destination": str(meta), "include_content": False})
        for copied, count in [(full, 5), (meta, 0)]:
            with App(binary, copied) as app:
                observed = available_status(app)
                record(copied.name + "_content_count", count, observed["content_total"])
                record(copied.name + "_stat_count", count, observed["statistics_documents"])
                for identifier, text in body_by_id.items():
                    record(f"{copied.name}_text_{identifier}", text if count else "",
                           app.request(f"/api/file?id={identifier}&max_chars=20000")["text"])
            record(copied.name + "_integrity", [["ok"]], sql(copied, "catalog.sqlite", "PRAGMA quick_check"))

        missing = root / "missing-content"
        shutil.copytree(full, missing)
        (missing / "content.sqlite").unlink()
        with App(binary, missing) as app:
            observed = available_status(app)
            record("missing_content_status_consistency", {"statistics_documents": 0, "content_total": 0,
                   "row_statuses": ["not_indexed"] * 5, "detail_statuses": ["not_indexed"] * 5}, observed)
        cli(missing, "scan", "--root", str(docs_a), "--label", "A", "--content")
        with App(binary, missing) as app:
            observed = available_status(app)
            record("partial_content_restore_stat_matches_searchable_count", observed["content_total"], observed["statistics_documents"], statuses=observed)
        cli(missing, "scan", "--root", str(docs_b), "--label", "B", "--content")
        with App(binary, missing) as app:
            record("full_content_restore", {"statistics_documents": 5, "content_total": 5,
                   "row_statuses": ["indexed"] * 5, "detail_statuses": ["indexed"] * 5}, available_status(app))
        record("restored_dedup", [[4]], sql(missing, "content.sqlite", "SELECT count(*) FROM documents"))

        corrupted = root / "corrupted-content"
        shutil.copytree(full, corrupted)
        (corrupted / "content.sqlite").write_bytes(b"deliberately invalid synthetic content database")
        before = hashlib.sha256((corrupted / "content.sqlite").read_bytes()).hexdigest()
        recovery = root / "corruption-recovery"
        with App(binary, corrupted) as app:
            record("corrupt_content_does_not_block_metadata_search", 5, app.search()["total"])
            code, error = app.request("/api/search?mode=content&q=dedupneedle", expected=None)
            record("corrupt_content_reports_failure", True, code != 200, response=error)
            app.request("/api/export", {"destination": str(recovery), "include_content": False})
        after = hashlib.sha256((corrupted / "content.sqlite").read_bytes()).hexdigest()
        record("corruption_read_preserves_content_file", before, after)
        cli(recovery, "scan", "--root", str(docs_a), "--label", "A", "--content")
        cli(recovery, "scan", "--root", str(docs_b), "--label", "B", "--content")
        with App(binary, recovery) as app:
            record("metadata_export_rebuild_recovers_corrupt_content", 5, app.search(mode="content")["total"])

        (docs_a / "shared.txt").unlink()
        cli(data, "scan", "--root", str(docs_a), "--label", "A", "--content")
        record("shared_document_retained_for_other_source", [[4]], sql(data, "content.sqlite", "SELECT count(*) FROM documents"))
        with App(binary, data) as app:
            record("shared_document_search_after_first_delete", 1, app.search("dedupneedle", "content")["total"])
        (docs_b / "shared.txt").unlink()
        cli(data, "scan", "--root", str(docs_b), "--label", "B", "--content")
        record("last_shared_document_link_cleanup", [[3]], sql(data, "content.sqlite", "SELECT count(*) FROM documents"))
        (docs_a / "contentonly.txt").write_text("replacementneedle is the current text", encoding="utf-8")
        cli(data, "scan", "--root", str(docs_a), "--label", "A")
        with App(binary, data) as app:
            record("metadata_rescan_removes_stale_content", 0, app.search("contentneedle", "content")["total"])
            record("metadata_rescan_updates_documents", 2, app.request("/api/status")["documents"])
        cli(data, "scan", "--root", str(docs_a), "--label", "A", "--content")
        with App(binary, data) as app:
            record("content_rescan_indexes_replacement", 1, app.search("replacementneedle", "content")["total"])
            record("cross_source_final_counts", [3, 3, 2], [app.request("/api/status")[key] for key in ("files", "documents", "sources")])

        # FTS matches tokens across punctuation and removes combining Latin
        # diacritics. A snippet should expose the matching region as well.
        snippet_data, snippet_docs = root / "snippet-data", root / "snippet-docs"
        snippet_data.mkdir()
        snippet_docs.mkdir()
        (snippet_docs / "punctuation.txt").write_text("intro " * 15000 + "north west markerend", encoding="utf-8")
        (snippet_docs / "combining.txt").write_text("intro " * 15000 + "re\u0301sume\u0301 markerend", encoding="utf-8")
        (snippet_docs / "unicode-space.txt").write_text("alpha intermediate beta", encoding="utf-8")
        cli(snippet_data, "scan", "--root", str(snippet_docs), "--content")
        with App(binary, snippet_data) as app:
            for term, expected_text in [("north-west", "north west"), ("resume", "re\u0301sume\u0301")]:
                found = app.search(term, "content")
                record("fts_snippet_region_" + term, [1, True],
                       [found["total"], len(found["results"]) == 1 and expected_text in found["results"][0]["snippet"]],
                       snippet=found["results"][0]["snippet"] if found["results"] else None)
            for separator, label in [(" ", "space"), ("\t", "tab"), ("\n", "newline"),
                                     ("\r", "carriage-return"), ("\u00a0", "nbsp"), ("\u2003", "em-space")]:
                found = app.search("alpha" + separator + "beta", "content")
                record("unquoted_content_terms_" + label, 1, found["total"])

        if args.bench:
            rng = random.Random(20261029)
            bench_data, bench_docs = root / "bench-data", root / "bench-docs"
            bench_data.mkdir()
            bench_docs.mkdir()
            for index in range(40):
                block = " ".join("".join(rng.choices("abcdefghijklmnopqrstuvwxyz", k=10)) for _ in range(200))
                text = f"earlyneedle document{index} " + (block + " ") * 240 + " tailneedle"
                (bench_docs / f"filenameonlyneedle-{index:03}.txt").write_text(text, encoding="utf-8")
            started = time.perf_counter()
            cli(bench_data, "scan", "--root", str(bench_docs), "--content")
            preparation = time.perf_counter() - started
            with App(binary, bench_data) as app:
                for label, query, mode in [("filename_files", "filenameonlyneedle", "files"),
                                            ("filename_all_no_content_match", "filenameonlyneedle", "all"),
                                            ("early_content_match", "earlyneedle", "content"),
                                            ("late_content_match", "tailneedle", "content"),
                                            ("all_empty_query", "", "all")]:
                    samples, internal = [], []
                    for iteration in range(6):
                        started = time.perf_counter()
                        found = app.search(query, mode)
                        elapsed = (time.perf_counter() - started) * 1000
                        assert found["total"] == 40
                        if iteration:
                            samples.append(elapsed)
                            internal.append(found["elapsed_ms"])
                    benchmarks.append({"name": label, "http_ms": samples, "median_http_ms": statistics.median(samples),
                                       "median_search_ms": statistics.median(internal), "fixture_documents": 40,
                                       "fixture_plaintext_bytes": sum(path.stat().st_size for path in bench_docs.iterdir()),
                                       "fixture_preparation_seconds": preparation})
            source_data, source_file = root / "sources-bench", root / "sources.csv"
            source_data.mkdir()
            with source_file.open("w", encoding="utf-8", newline="") as stream:
                writer = csv.writer(stream)
                writer.writerow(["Filename", "Size", "Disk"])
                for index in range(100000):
                    writer.writerow([f"folder{index % 13}/file-{index:06}.bin", index + 1, f"Source {index % 250:03}"])
            cli(source_data, "import", "--file", str(source_file))
            with App(binary, source_data) as app:
                paged = app.request("/api/sources?offset=0&limit=200")["sources"] + app.request("/api/sources?offset=200&limit=200")["sources"]
                record("many_source_pagination_counts", [250, 100000, 5000050000],
                       [len({source["id"] for source in paged}), sum(source["file_count"] for source in paged), sum(source["total_size"] for source in paged)])
                for endpoint in ["/api/status", "/api/sources?offset=0&limit=200", "/api/sources"]:
                    samples = []
                    for iteration in range(6):
                        started = time.perf_counter()
                        app.request(endpoint)
                        elapsed = (time.perf_counter() - started) * 1000
                        if iteration:
                            samples.append(elapsed)
                    benchmarks.append({"name": endpoint, "http_ms": samples, "median_http_ms": statistics.median(samples), "fixture_sources": 250, "fixture_files": 100000})
            # Controlled candidate on this generated fixture only. New builds
            # already create this index; report that as a repeated measurement,
            # rather than claiming that an index was added by the experiment.
            database = source_data / "catalog.sqlite"
            before_bytes = database.stat().st_size
            with contextlib.closing(sqlite3.connect(database)) as connection:
                index_preexisted = any(row[1] == "files_source_size" for row in connection.execute("PRAGMA index_list(files)"))
                connection.execute("CREATE INDEX IF NOT EXISTS files_source_size ON files(source_id,size)")
                connection.commit()
            with App(binary, source_data) as app:
                indexes = sql(source_data, "catalog.sqlite", "PRAGMA index_list(files)")
                plan_query = "SELECT s.id,(SELECT COUNT(*) FROM files f WHERE f.source_id=s.id),(SELECT COALESCE(SUM(f.size),0) FROM files f WHERE f.source_id=s.id) FROM sources s ORDER BY s.label,s.id LIMIT 200"
                plan = sql(source_data, "catalog.sqlite", "EXPLAIN QUERY PLAN " + plan_query)
                all_sources = app.request("/api/sources")["sources"]
                record("covering_source_index_keeps_counts", [250, 100000, 5000050000],
                       [len(all_sources), sum(source["file_count"] for source in all_sources), sum(source["total_size"] for source in all_sources)])
                for endpoint in ["/api/sources?offset=0&limit=200", "/api/sources"]:
                    samples = []
                    for iteration in range(6):
                        started = time.perf_counter()
                        app.request(endpoint)
                        elapsed = (time.perf_counter() - started) * 1000
                        if iteration:
                            samples.append(elapsed)
                    benchmarks.append({"name": ("existing-covering-index repeat " if index_preexisted else "covering-index ") + endpoint, "http_ms": samples,
                                       "median_http_ms": statistics.median(samples), "fixture_sources": 250,
                                       "fixture_files": 100000, "catalog_bytes_before": before_bytes,
                                       "catalog_bytes_after": database.stat().st_size, "query_plan": plan,
                                       "query_plan_sql": plan_query, "covering_index_preexisted": index_preexisted,
                                       "query_plan_engine": "Python SQLite " + sqlite3.sqlite_version,
                                       "indexes_after_app_open": indexes,
                                       "scope": ("Covering index already exists in this build; repeated timings are not an index before/after experiment. " if index_preexisted else "Additional covering index only on the temporary fixture. ") + "Both indexes remain; file sizes include free pages and do not estimate a clean replacement or write cost. Query plan is for the stated correlated summary SQL using Python SQLite. No production edits."})

    report = {"binary": str(binary), "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "platform": platform.platform(), "scope": "Synthetic lifecycle, fault injection on copies, HTTP mode union and optional warm-cache performance; no user data",
              "summary": {"checks": len(cases), "passed": sum(case["passed"] for case in cases), "failed": sum(not case["passed"] for case in cases)},
              "cases": cases, "benchmarks": benchmarks}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
    return 1 if report["summary"]["failed"] else 0


if __name__ == "__main__":
    raise SystemExit(main())
