"""Compare complete HTTP searches against copies of the synthetic comparison fixture."""
import argparse
import hashlib
import http.client
import json
import pathlib
import platform
import queue
import shutil
import statistics
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request

p = argparse.ArgumentParser()
p.add_argument("--go", required=True, type=pathlib.Path)
p.add_argument("--rust", required=True, type=pathlib.Path)
p.add_argument("--data", required=True, type=pathlib.Path)
p.add_argument("--output", required=True, type=pathlib.Path)
a = p.parse_args()
cases = json.loads((a.data / "cases.json").read_text())["cases"]
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
results = {"platform": platform.platform(), "machine": platform.machine(), "warmups": 3, "iterations": 15,
           "rounds": [], "scope": "HTTP, full result metadata, count, page, content snippets; synthetic unencrypted warm-cache catalog; no browser or indexing"}
results["input_sha256"] = {name: hashlib.sha256((a.data / name).read_bytes()).hexdigest() for name in ["catalog.sqlite", "content.sqlite"]}
with tempfile.TemporaryDirectory(prefix="allfiles-http-") as tmp:
    for round_number, order in enumerate([["go", "rust"], ["rust", "go"]], 1):
        for name in order:
            directory = pathlib.Path(tmp) / f"{round_number}-{name}"
            directory.mkdir()
            for filename in results["input_sha256"]:
                shutil.copyfile(a.data / filename, directory / filename)
            binary = getattr(a, name).resolve()
            proc = subprocess.Popen([str(binary), "serve", "--data", str(directory), "--port", "0", "--no-browser"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
            lines = queue.Queue()
            def collect():
                for line in proc.stdout:
                    lines.put(line)
            threading.Thread(target=collect, daemon=True).start()
            base, token, connection = None, None, None
            def get(endpoint):
                connection.request("GET", endpoint)
                response=connection.getresponse()
                body=response.read()
                assert response.status==200,body[:200]
                return json.loads(body)
            try:
                for _ in range(6):
                    line = lines.get(timeout=20)
                    if line.startswith("Local: "):
                        base = line.strip()[7:]
                        break
                assert base
                connection=http.client.HTTPConnection("127.0.0.1",urllib.parse.urlsplit(base).port,timeout=30)
                token = get("/api/session")["token"]
                status = get("/api/status")
                assert status["files"] == 100050 and not status["encrypted"]
                row = {"round": round_number, "implementation": name, "version": status["version"],
                       "sqlite_version": status.get("sqlite_version", "3.53.4 (v0.2.0 lockfile)"), "binary_bytes": binary.stat().st_size, "cases": []}
                for case in cases:
                    query = case["args"][0].strip('*"') if case["args"] else ""
                    params = {"q": query, "mode": "content" if case["name"].startswith("content_") else "files", "limit": 50, "offset": 90000 if "offset90k" in case["name"] else 0}
                    endpoint = "/api/search?" + urllib.parse.urlencode(params)
                    samples, internal = [], []
                    for i in range(results["warmups"] + results["iterations"]):
                        start = time.perf_counter()
                        reply = get(endpoint)
                        elapsed = (time.perf_counter() - start) * 1000
                        assert reply["total"] == case["expected_count"]
                        assert [f["id"] for f in reply["results"]] == case["expected_ids"]
                        if i >= results["warmups"]:
                            samples.append(elapsed)
                            internal.append(reply["elapsed_ms"])
                    row["cases"].append({"name": case["name"], "http_median_ms": statistics.median(samples), "search_median_ms": statistics.median(internal), "http_samples_ms": samples})
                results["rounds"].append(row)
                print(name, round_number, [(x["name"], round(x["http_median_ms"], 3)) for x in row["cases"]], flush=True)
            finally:
                if connection:
                    connection.close()
                if token:
                    r = urllib.request.Request(base + "/api/shutdown", data=b"{}", headers={"X-AllFiles-Token": token, "Content-Type": "application/json"})
                    with opener.open(r, timeout=30) as response:
                        response.read()
                proc.wait(timeout=20)
                assert proc.returncode == 0, proc.stderr.read()
for filename, digest in results["input_sha256"].items():
    assert hashlib.sha256((a.data / filename).read_bytes()).hexdigest() == digest
a.output.parent.mkdir(parents=True, exist_ok=True)
a.output.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
