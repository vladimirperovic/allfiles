"""Exercise the built application over HTTP and MCP using synthetic data only."""
import argparse
import json
import pathlib
import queue
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request

parser = argparse.ArgumentParser()
parser.add_argument("binary", type=pathlib.Path)
args = parser.parse_args()
binary = str(args.binary.resolve())
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

with tempfile.TemporaryDirectory(prefix="allfiles-smoke-") as directory:
    root = pathlib.Path(directory)
    data, docs = root / "data", root / "docs"
    data.mkdir()
    docs.mkdir()
    (docs / "Unicode čaša.txt").write_text("integrationneedle 🌊 private text", encoding="utf-8")
    (docs / "image.bin").write_bytes(bytes([0, 1, 2]))
    server = subprocess.Popen([binary, "serve", "--data", str(data), "--port", "0", "--no-browser"],
                              stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
    lines = queue.Queue()
    def collect(stream, output):
        for line in stream:
            output.put(line)
    threading.Thread(target=collect, args=(server.stdout, lines), daemon=True).start()
    base, token, bridge = None, None, None
    def request(path, payload=None, headers=None, expected=200, raw=None):
        body = raw if raw is not None else (json.dumps(payload).encode() if payload is not None else None)
        h = {"Origin": base}
        if body is not None:
            h.update({"Content-Type": "application/json", "X-AllFiles-Token": token or ""})
        h.update(headers or {})
        r = urllib.request.Request(base + path, data=body, headers=h)
        try:
            response = opener.open(r, timeout=30)
        except urllib.error.HTTPError as e:
            response = e
        with response:
            body = response.read()
            assert response.status == expected, (path, response.status, body[:300])
            assert response.headers["Cache-Control"] == "no-store"
            return json.loads(body)
    def finish():
        for _ in range(300):
            status = request("/api/status")
            if status["job"]["status"] == "completed":
                return status
            assert status["job"]["status"] not in ["failed", "cancelled"], status
            time.sleep(.05)
        raise AssertionError("Indexing timed out")
    try:
        for _ in range(5):
            line = lines.get(timeout=20)
            if line.startswith("Local: "):
                base = line.strip()[7:]
                break
        assert base
        token = request("/api/session")["token"]
        assert request("/api/status")["backend"] == "rust"
        request("/api/status", headers={"Host": "evil.invalid"}, expected=403)
        request("/api/status", headers={"Origin": "https://evil.invalid"}, expected=403)
        request("/api/status", headers={"Sec-Fetch-Site": "cross-site"}, expected=403)
        request("/api/cancel", {}, headers={"X-AllFiles-Token": "wrong"}, expected=403)
        request("/api/scan", raw=b'{} {}', expected=400)
        request("/api/scan", {"root": str(docs), "unexpected": True}, expected=400)
        request("/api/cancel", raw=b'x' * ((1 << 20) + 1), expected=413)
        request("/api/scan", {"root": str(docs), "label": "Synthetic", "index_content": True}, expected=202)
        status = finish()
        assert (status["files"], status["documents"]) == (2, 1), status
        duplicate = subprocess.run([binary, "export", "--data", str(data), "--out", str(root / "forbidden")], capture_output=True, timeout=20)
        assert duplicate.returncode != 0 and not (root / "forbidden").exists()
        result = request("/api/search?q=integrationneedle&mode=content")
        assert result["total"] == 1
        file_id = result["results"][0]["id"]
        page = request(f"/api/file?id={file_id}&text_offset=18&max_chars=1")
        assert page["text"] == "🌊", page
        copied = request("/api/export", {"destination": str(root / "copy"), "include_content": True})
        assert not copied["encrypted"]
        assert (root / "copy" / "content.sqlite").is_file()
        bridge = subprocess.Popen([binary, "mcp", "--url", base], stdin=subprocess.PIPE,
                                  stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding="utf-8")
        replies = queue.Queue()
        threading.Thread(target=collect, args=(bridge.stdout, replies), daemon=True).start()
        next_id = 0
        def send(message):
            bridge.stdin.write(json.dumps(message) + "\n")
            bridge.stdin.flush()
        def rpc(method, params):
            global next_id
            next_id += 1
            send({"jsonrpc": "2.0", "id": next_id, "method": method, "params": params})
            answer = json.loads(replies.get(timeout=30))
            assert answer["id"] == next_id and "error" not in answer, answer
            return answer["result"]
        initialized = rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "1"}})
        assert initialized["serverInfo"]["version"] == status["version"]
        send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        assert len(rpc("tools/list", {})["tools"]) == 6
        searched = rpc("tools/call", {"name": "allfiles_search", "arguments": {"query": "integrationneedle", "mode": "content"}})
        assert not searched["isError"] and searched["structuredContent"]["total"] == 1
        invalid = rpc("tools/call", {"name": "allfiles_file", "arguments": {"id": file_id, "max_chars": 20001}})
        assert invalid["isError"]
        discovered = rpc("server/discover", {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28", "io.modelcontextprotocol/clientCapabilities": {}}})
        assert discovered["resultType"] == "complete" and "2026-07-28" in discovered["supportedVersions"]
        bridge.stdin.close()
        bridge.wait(timeout=10)
        assert bridge.returncode == 0 and not bridge.stderr.read()
        print(json.dumps({"http_security": True, "scan": True, "content": True, "unicode_page": True,
                          "export": True, "cross_process_lock": True, "legacy_and_modern_mcp": True}))
    finally:
        if bridge and bridge.poll() is None:
            bridge.kill()
            bridge.wait(timeout=10)
        if base and token:
            try:
                request("/api/shutdown", {})
            except Exception:
                pass
        try:
            server.wait(timeout=15)
        except subprocess.TimeoutExpired:
            server.kill()
            server.wait(timeout=10)
        assert server.returncode == 0, server.stderr.read()
