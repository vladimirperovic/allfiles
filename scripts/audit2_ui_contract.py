"""Live HTTP checks for the frontend contract, using temporary synthetic catalogs."""
import argparse
import hashlib
import json
import os
import pathlib
import queue
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=pathlib.Path)
    parser.add_argument("output", type=pathlib.Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    reports = []
    with tempfile.TemporaryDirectory(prefix="allfiles-ui-contract-") as directory:
        root = pathlib.Path(directory)
        data, docs = root / "data", root / "docs"
        data.mkdir(); docs.mkdir()
        export = root / "media.csv"
        export.write_text("Filename,Size,Disk\n" + "".join(f"file{i:03}.txt,{i},Media{i:03}\n" for i in range(1, 252)), encoding="utf-8")
        imported = subprocess.run([str(binary), "import", "--data", str(data), "--file", str(export)], capture_output=True, timeout=30)
        assert imported.returncode == 0, imported.stderr
        server, base, token = None, None, None

        def start(port=0):
            nonlocal server, base, token
            server = subprocess.Popen([str(binary), "serve", "--data", str(data), "--port", str(port), "--no-browser"], stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
            lines = queue.Queue()
            def collect():
                for line in server.stdout:
                    lines.put(line)
            threading.Thread(target=collect, daemon=True).start()
            for _ in range(5):
                line = lines.get(timeout=15)
                if line.startswith("Local: "):
                    base = line.strip()[7:]
                    token = request("/api/session")["token"]
                    return
            raise AssertionError("Missing local URL")

        def request(path, payload=None, expected=200, override_token=None):
            body = json.dumps(payload).encode() if payload is not None else None
            headers = {"Origin": base}
            if body is not None:
                headers.update({"Content-Type": "application/json", "X-AllFiles-Token": override_token or token})
            try:
                response = opener.open(urllib.request.Request(base + path, data=body, headers=headers), timeout=30)
            except urllib.error.HTTPError as error:
                response = error
            with response:
                result = json.load(response)
                assert response.status == expected, (path, response.status, result)
                return result

        def completed():
            deadline = time.monotonic() + 30
            while time.monotonic() < deadline:
                current = request("/api/status")
                if current["job"]["status"] == "completed":
                    return current
                assert current["job"]["status"] == "running", current
                time.sleep(0.01)
            raise AssertionError("Job timeout")

        def stop():
            if server is not None and server.poll() is None:
                request("/api/shutdown", {})
                server.wait(timeout=15)
                assert server.returncode == 0

        try:
            start()
            with opener.open(base + "/app.js", timeout=10) as response:
                served_app = response.read()
            checked_app = (pathlib.Path(__file__).resolve().parent.parent / "web" / "app.js").read_bytes()
            assert served_app == checked_app, "State probes must use the JavaScript embedded in the tested binary"
            reports.append({"case": "served JavaScript matches the state-probe source", "pass": True, "sha256": hashlib.sha256(served_app).hexdigest()})
            all_sources = request("/api/sources")["sources"]
            first = request("/api/sources?offset=0&limit=200")
            last = request("/api/sources?offset=200&limit=200")
            assert len(all_sources) == 251 and len(first["sources"]) == 200 and len(last["sources"]) == 51
            assert first["has_more"] and not last["has_more"] and first["total"] == 251
            assert [s["id"] for s in all_sources] == [s["id"] for s in first["sources"] + last["sources"]]
            reports.append({"case": "unpaged and paged sources above 200", "pass": True, "unpaged_count": 251, "page_sizes": [200, 51]})

            text = "a" * 9999 + "\U0001f30aTAIL"
            alpha = docs / "alpha.txt"
            alpha.write_text(text, encoding="utf-8")
            (docs / "beta.txt").write_text("different content", encoding="utf-8")
            (docs / "noise.bin").write_bytes(b"\x00\x01")
            request("/api/scan", {"root": str(docs), "label": "Scan old label", "kind": "folder", "location": "Old shelf", "index_content": True}, expected=202)
            before = completed()
            source = next(s for s in request("/api/sources")["sources"] if s["label"] == "Scan old label")
            params = urllib.parse.urlencode({"q": "TAIL", "source": source["id"], "ext": "txt", "mode": "content", "sort": "modified", "offset": 0, "limit": 60})
            searched = request("/api/search?" + params)
            assert searched["total"] == 1 and searched["results"][0]["name"] == "alpha.txt"
            file_id = searched["results"][0]["id"]
            page = request(f"/api/file?id={file_id}&text_offset=0&max_chars=10000")
            tail = request(f"/api/file?id={file_id}&text_offset=10000&max_chars=4")
            assert len(page["text"]) == 10000 and page["text"].endswith("\U0001f30a") and page["has_more"]
            assert tail["text"] == "TAIL" and not tail["has_more"] and tail["returned_chars"] == 4
            reports.append({"case": "frontend filter fields and Unicode detail paging", "pass": True, "filtered_count": searched["total"], "preview_chars": page["returned_chars"], "tail": tail["text"], "tail_has_more": tail["has_more"]})

            ordered = request("/api/search?sort=size&limit=3")
            sizes = [f["size"] for f in ordered["results"]]
            assert sizes == sorted(sizes, reverse=True)
            reports.append({"case": "UI size-sort enum", "pass": True, "sizes": sizes})

            alpha.write_text("new document body", encoding="utf-8")
            metadata = alpha.stat()
            os.utime(alpha, ns=(metadata.st_atime_ns, metadata.st_mtime_ns + 2_000_000_000))
            request("/api/scan", {"root": str(docs), "source_id": source["id"], "label": "Scan new label", "kind": "folder", "location": "New shelf", "index_content": True}, expected=202)
            after = completed()
            changed = request(f"/api/file?id={file_id}&text_offset=0&max_chars=10000")
            assert before["files"] == after["files"] and before["job"]["id"] != after["job"]["id"]
            assert changed["text"] == "new document body" and changed["file"]["source_label"] == "Scan new label"
            reports.append({"case": "a completed rescan changes data with the same file count", "pass": True, "before_status": before["job"]["status"], "after_status": after["job"]["status"], "job_id_changed": True, "before_files": before["files"], "after_files": after["files"], "stable_file_id": file_id, "new_text": changed["text"], "new_label": changed["file"]["source_label"]})

            old_token = token
            port = urllib.parse.urlparse(base).port
            stop(); start(port)
            assert token != old_token
            rejected = request("/api/cancel", {}, expected=403, override_token=old_token)
            assert "Osvežite stranicu" in rejected["error"]
            request("/api/cancel", {})
            reports.append({"case": "restart changes token and rejects the old session", "pass": True, "old_token_status": 403, "new_token_status": 200})
        finally:
            try:
                stop()
            except Exception:
                if server is not None and server.poll() is None:
                    server.kill(); server.wait(timeout=10)
                raise
    report = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "cases": reports, "passed": sum(r["pass"] for r in reports), "total": len(reports)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=True, indent=2), encoding="utf-8")
    print(json.dumps(report, ensure_ascii=True, indent=2))


if __name__ == "__main__":
    main()
