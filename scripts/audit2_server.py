"""Second-pass HTTP/MCP concurrency probes on a new synthetic catalog only.

No production source is modified. Findings are recorded as evidence, and the
script exits nonzero when a newly audited behavioral defect is reproduced.
"""
import argparse
import concurrent.futures
import csv
import hashlib
import json
from pathlib import Path
import platform
import queue
import socket
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


def collect(stream, output):
    for line in stream:
        output.put(line)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--rows", type=int, default=100000)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    report = {"binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
              "platform": platform.platform(), "synthetic_rows": args.rows,
              "checks": [], "findings": [], "metrics": {}}

    def check(name, passed, **details):
        report["checks"].append({"name": name, "pass": bool(passed), **details})

    with tempfile.TemporaryDirectory(prefix="allfiles-audit2-server-") as directory:
        root = Path(directory)
        data, docs, scanning = root / "data", root / "docs", root / "scanning"
        for folder in [data, docs, scanning]:
            folder.mkdir()
        (docs / "small.txt").write_text("synthetic small document", encoding="utf-8")
        for i in range(20):
            (scanning / f"document-{i}.txt").write_text((f"scanword {i} ") * 10000, encoding="utf-8")
        source = root / "synthetic.csv"
        with source.open("w", newline="", encoding="utf-8") as stream:
            writer = csv.writer(stream)
            writer.writerow(["Filename", "File Path", "Size", "Date Modified", "Disk"])
            for i in range(args.rows):
                writer.writerow([f"alpha-abcdefghijklmnopqrstuvwxyz-document-{i:06d}.txt", f"Archive/Folder{i % 100}",
                                 100, "2024-01-01", "SYNTHETIC"])
        for command in [["import", "--file", str(source)],
                        ["scan", "--root", str(docs), "--label", "Documents", "--content"]]:
            seeded = subprocess.run([binary, command[0], "--data", str(data), *command[1:]],
                                    capture_output=True, timeout=60)
            assert seeded.returncode == 0, seeded.stderr.decode(errors="replace")
        server = subprocess.Popen([binary, "serve", "--data", str(data), "--no-browser"],
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        lines = queue.Queue()
        threading.Thread(target=collect, args=(server.stdout, lines), daemon=True).start()
        base, token = None, None
        for _ in range(5):
            line = lines.get(timeout=20).decode("utf-8")
            if line.startswith("Local: "):
                base = line.strip()[7:]
                break
        assert base
        token = json.load(opener.open(base + "/api/session", timeout=5))["token"]

        def request(path, body=None):
            start = time.perf_counter()
            req = urllib.request.Request(base + path,
                data=json.dumps(body).encode() if body is not None else None,
                headers={"Content-Type": "application/json", "X-AllFiles-Token": token})
            try:
                response = opener.open(req, timeout=60)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                payload = response.read()
                return {"ms": (time.perf_counter() - start) * 1000, "completed_at": time.perf_counter(), "status": response.status,
                        "bytes": len(payload), "json": json.loads(payload)}

        def abandon_search(path, count):
            port = int(base.rsplit(":", 1)[1])
            for _ in range(count):
                connection = socket.create_connection(("127.0.0.1", port), timeout=5)
                connection.sendall(f"GET {path} HTTP/1.1\r\nHost: {base[7:]}\r\nConnection: close\r\n\r\n".encode())
                # Let the server accept the request, then close both directions.
                time.sleep(.03)
                connection.shutdown(socket.SHUT_RDWR)
                connection.close()

        def finish_job():
            for _ in range(500):
                status = request("/api/status")["json"]
                if status.get("job", {}).get("status") != "running":
                    return status
                time.sleep(.02)
            raise AssertionError("synthetic scan did not finish")

        try:
            file_id = request("/api/search?q=small.txt")["json"]["results"][0]["id"]
            paths = {"status": "/api/status", "file": f"/api/file?id={file_id}&max_chars=100",
                     "browse": "/api/browse?" + urllib.parse.urlencode({"path": str(docs)}),
                     "session": "/api/session"}
            baseline = {key: statistics.median(request(path)["ms"] for _ in range(5))
                        for key, path in paths.items()}
            report["metrics"]["baseline_ms"] = baseline
            repeated = {}
            for count in [1, 20, 100, 200]:
                path = "/api/search?" + urllib.parse.urlencode({"q": " ".join(["a"] * count), "limit": 1})
                samples = [request(path) for _ in range(3)]
                check(f"repeated_term_{count}_same_result", all(s["status"] == 200 and
                      s["json"]["total"] == args.rows + 1 for s in samples))
                repeated[str(count)] = {"wall_ms": [s["ms"] for s in samples],
                                       "sql_ms": [s["json"].get("elapsed_ms") for s in samples]}
            report["metrics"]["repeated_equivalent_terms"] = repeated
            # Distinct terms remain expensive after repeated-term deduplication.
            # Every term matches the common filename prefix, with exact result checks.
            alphabet = "abcdefghijklmnopqrstuvwxyz"
            distinct_terms = [alphabet[i:i + width] for width in range(2, 9)
                              for i in range(len(alphabet) - width + 1)]
            slow_query = " ".join(distinct_terms)
            assert len(slow_query) <= 1000 and len(set(distinct_terms)) == len(distinct_terms)
            slow = "/api/search?" + urllib.parse.urlencode({"q": slow_query, "limit": 1})
            calibration = [request(slow) for _ in range(3)]
            slow_ms = statistics.median(sample["ms"] for sample in calibration)
            check("distinct_term_stress_has_expected_results", all(sample["status"] == 200 and
                  sample["json"]["total"] == args.rows for sample in calibration))
            check("cancellation_probe_exercises_nontrivial_work", slow_ms >= 100,
                  calibration_ms=slow_ms, minimum_ms=100)
            report["metrics"]["distinct_term_stress"] = {"term_count": len(distinct_terms),
                "query_chars": len(slow_query), "median_ms": slow_ms,
                "wall_ms": [sample["ms"] for sample in calibration]}

            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                search = pool.submit(request, slow)
                time.sleep(.03)
                old_status = pool.submit(request, "/api/status")
                time.sleep(.03)
                accepted = request("/api/scan", {"root": str(scanning), "label": "New scan", "index_content": True})
                stalled = old_status.result()
                search_result = search.result()
                current = request("/api/status")
            new_id = accepted["json"].get("job", {}).get("id")
            stale_job = stalled["json"].get("job")
            live_job = current["json"].get("job")
            race = {"accepted_job": accepted["json"].get("job"), "accepted_ms": accepted["ms"],
                    "returned_old_job": stale_job, "status_ms": stalled["ms"],
                    "next_status_job": live_job, "search_ms": search_result["ms"],
                    "status_finished_before_acceptance": stalled["completed_at"] <= accepted["completed_at"],
                    "accepted_revision": accepted["json"].get("job_revision"),
                    "current_revision": current["json"].get("job_revision")}
            report["metrics"]["job_status_race"] = race
            stale = (accepted["status"] == 202 and stalled["completed_at"] > accepted["completed_at"]
                     and (stale_job or {}).get("id") != new_id and (live_job or {}).get("id") == new_id)
            check("status_does_not_return_pre_scan_snapshot_after_accepted_scan", not stale)
            if stale:
                report["findings"].append({"id": "status_snapshot_race", "priority": "P2",
                    "evidence": race, "code": ["src/server.rs:278", "web/app.js:117", "web/app.js:178"],
                    "impact": "An in-flight status response can overwrite the newly accepted running job in the UI, hide cancellation and use the wrong polling interval."})
            check("job_instance_revision_contract", bool(accepted["json"].get("instance_id")) and
                  accepted["json"].get("instance_id") == current["json"].get("instance_id") and
                  isinstance(accepted["json"].get("job_revision"), int) and
                  current["json"].get("job_revision", -1) >= accepted["json"].get("job_revision", 0))
            completed = finish_job()
            check("synthetic_scan_finishes", completed["job"]["status"] == "completed")
            check("completed_job_counters_are_fresh", completed["files"] == args.rows + 21 and completed["documents"] == 21)

            abandon_search(slow, 3)
            with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
                pending = {name: pool.submit(request, path) for name, path in paths.items()}
                pending["cancel"] = pool.submit(request, "/api/cancel", {})
                delayed = {key: task.result() for key, task in pending.items()}
            waits = {key: value["ms"] for key, value in delayed.items()}
            report["metrics"]["after_three_disconnected_searches_ms"] = waits
            check("parallel_routes_return_success", all(s["status"] == 200 for s in delayed.values()))
            # Status caching alone must not make this pass: file reads still need
            # the store and prove discarded SQL work has actually stopped.
            unnecessary_work = waits["file"] > max(100, baseline["file"] * 20)
            check("disconnected_searches_release_store_for_file_reads", not unnecessary_work)
            check("status_stays_responsive_under_search_load", waits["status"] <= max(100, baseline["status"] * 20))
            if unnecessary_work:
                report["findings"].append({"id": "disconnected_search_work_continues", "priority": "P2",
                    "evidence": {"baseline_ms": baseline, "after_three_closed_clients_ms": waits},
                    "code": ["src/server.rs:167", "src/server.rs:304", "web/app.js:257"],
                    "impact": "AbortController/network disconnect releases the client but not spawn_blocking SQL. Discarded searches serially hold the shared store and delay current results, file details and status."})

            bridge = subprocess.Popen([binary, "mcp", "--url", base], stdin=subprocess.PIPE,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            responses = queue.Queue()
            threading.Thread(target=collect, args=(bridge.stdout, responses), daemon=True).start()
            def send(value):
                bridge.stdin.write((json.dumps(value) + "\n").encode())
                bridge.stdin.flush()
            try:
                send({"jsonrpc": "2.0", "id": "init", "method": "initialize", "params": {
                    "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "audit2", "version": "1"}}})
                assert json.loads(responses.get(timeout=5))["id"] == "init"
                send({"jsonrpc": "2.0", "method": "notifications/initialized"})
                send({"jsonrpc": "2.0", "id": "cancelled-search", "method": "tools/call", "params": {
                    "name": "allfiles_search", "arguments": {"query": slow_query, "limit": 1}}})
                time.sleep(.03)
                send({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": "cancelled-search"}})
                ping_start = time.perf_counter()
                send({"jsonrpc": "2.0", "id": "ping", "method": "ping", "params": {}})
                ping = json.loads(responses.get(timeout=5))
                check("mcp_ping_survives_search_cancel", ping.get("id") == "ping" and "result" in ping)
                ping_ms = (time.perf_counter() - ping_start) * 1000
                status = request("/api/status")
                file_result = request(paths["file"])
                check("mcp_cancel_stops_backend_store_work", file_result["ms"] <= max(100, baseline["file"] * 20))
                report["metrics"]["mcp_cancel"] = {"ping_ms": ping_ms, "subsequent_status_ms": status["ms"],
                    "subsequent_file_ms": file_result["ms"]}
            finally:
                bridge.stdin.close()
                bridge.wait(timeout=5)
                bridge.stdout.close()
                bridge.stderr.close()

            destination = root / "encrypted-export"
            with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
                export = pool.submit(request, "/api/export", {"destination": str(destination), "include_content": True,
                                     "encrypt": True, "password": "synthetic-audit-password"})
                time.sleep(.03)
                pending = {"status": pool.submit(request, "/api/status"),
                           "cancel": pool.submit(request, "/api/cancel", {}),
                           "browse": pool.submit(request, paths["browse"])}
                under_export = {key: task.result() for key, task in pending.items()}
                exported = export.result()
            check("encrypted_export_finishes", exported["status"] == 200 and exported["json"].get("encrypted") and
                  (destination / "catalog.sqlite").is_file() and (destination / "encryption.json").is_file() and
                  not (destination / ".allfiles-export-incomplete").exists())
            report["metrics"]["encrypted_export"] = {"export_ms": exported["ms"],
                "concurrent_ms": {key: value["ms"] for key, value in under_export.items()},
                "note": "Synchronous export is retained; status can report cached counters while progress and cancellation remain responsive."}
            check("status_and_cancel_remain_responsive_during_export", all(
                under_export[key]["ms"] <= max(100, exported["ms"] * .5) for key in ["status", "cancel"]))
            after_export = request("/api/status")["json"]
            check("export_reservation_is_released", after_export.get("exporting") is False and
                  after_export["files"] == args.rows + 21)

            abandon_search(slow, 3)
            shutdown = request("/api/shutdown", {})
            wait_start = time.perf_counter()
            server.wait(timeout=20)
            report["metrics"]["shutdown_after_three_disconnected_searches"] = {
                "ack_ms": shutdown["ms"], "process_exit_after_ack_ms": (time.perf_counter() - wait_start) * 1000}
            check("shutdown_eventually_exits_cleanly", server.returncode == 0)
            check("shutdown_does_not_wait_for_abandoned_search_backlog",
                  report["metrics"]["shutdown_after_three_disconnected_searches"]["process_exit_after_ack_ms"] <= max(250, slow_ms * .25))
        finally:
            if server.poll() is None:
                try:
                    request("/api/shutdown", {})
                    server.wait(timeout=20)
                except Exception:
                    server.kill()
                    server.wait(timeout=5)
            server.stdout.close()
            server.stderr.close()

    report["summary"] = {"checks": len(report["checks"]), "failed_checks": sum(not c["pass"] for c in report["checks"]),
                         "confirmed_findings": len(report["findings"])}
    text = json.dumps(report, ensure_ascii=False, indent=2)
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(text + "\n", encoding="utf-8")
    print(text)
    return bool(report["summary"]["failed_checks"])


if __name__ == "__main__":
    raise SystemExit(main())
