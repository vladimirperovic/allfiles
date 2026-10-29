"""Extended HTTP/MCP audit using only temporary synthetic catalogs.

Run: python scripts/audit-server-mcp.py PATH_TO_ALLFILES_EXE
The regression cases cover previously confirmed failures. All cases must pass
for the repaired build, including cancellation, EOF and broken-pipe handling.
"""
import argparse
import http.server
import json
import pathlib
import queue
import sqlite3
import subprocess
import tempfile
import threading
import time
import unittest
import urllib.error
import urllib.request

PARSER = argparse.ArgumentParser(description=__doc__)
PARSER.add_argument("binary", type=pathlib.Path)
ARGS = PARSER.parse_args()
BINARY = str(ARGS.binary.resolve())
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def collect(stream, output):
    for line in stream:
        output.put(line)


class Bridge:
    def __init__(self, base, raw=False):
        self.process = subprocess.Popen(
            [BINARY, "mcp", "--url", base], stdin=subprocess.PIPE,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        self.next_id = 0
        self.lines = queue.Queue()
        self.raw = raw
        if not raw:
            threading.Thread(target=collect, args=(self.process.stdout, self.lines), daemon=True).start()

    def send(self, value):
        self.process.stdin.write((json.dumps(value, ensure_ascii=False) + "\n").encode())
        self.process.stdin.flush()

    def read(self):
        if self.raw:
            result = queue.Queue()
            threading.Thread(target=lambda: result.put(self.process.stdout.readline()), daemon=True).start()
            return json.loads(result.get(timeout=10))
        return json.loads(self.lines.get(timeout=10))

    def rpc(self, method, params):
        self.next_id += 1
        self.send({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params})
        result = self.read()
        assert result["id"] == self.next_id, result
        return result

    def initialize(self):
        result = self.rpc("initialize", {"protocolVersion": "2025-11-25", "capabilities": {},
                                         "clientInfo": {"name": "audit", "version": "1"}})
        assert "result" in result, result
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def close(self):
        if not self.process.stdin.closed:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)
        self.process.stdout.close()
        self.process.stderr.close()


class ServerMCPAudit(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory(prefix="allfiles-server-audit-")
        cls.root = pathlib.Path(cls.temporary.name)
        data, docs = cls.root / "data", cls.root / "docs"
        data.mkdir()
        docs.mkdir()
        (docs / "Unicode čaša.txt").write_text("needle 🌊 čaša " * 2000, encoding="utf-8")
        seed = subprocess.run([BINARY, "scan", "--data", str(data), "--root", str(docs),
                               "--label", "seed", "--content"], capture_output=True, timeout=30)
        if seed.returncode:
            raise RuntimeError(seed.stderr.decode(errors="replace"))
        # Offline fixture only: no live database files are modified. A 6000-char
        # label is accepted by both the public HTTP API and MCP's 8192-byte cap.
        db = sqlite3.connect(data / "catalog.sqlite")
        try:
            db.executemany("INSERT INTO sources(id,root,label,kind) VALUES(?,?,?,'folder')",
                           [(f"audit-source-{i}", str(docs / str(i)), str(i) + "x" * 6000)
                            for i in range(200)])
            db.commit()
        finally:
            db.close()
        cls.server = subprocess.Popen([BINARY, "serve", "--data", str(data), "--no-browser"],
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        lines = queue.Queue()
        threading.Thread(target=collect, args=(cls.server.stdout, lines), daemon=True).start()
        for _ in range(5):
            line = lines.get(timeout=20).decode("utf-8")
            if line.startswith("Local: "):
                cls.base = line.strip()[7:]
                break
        else:
            raise RuntimeError("Server did not announce its address")
        cls.token = json.load(OPENER.open(cls.base + "/api/session", timeout=5))["token"]
        cls.file_id = json.load(OPENER.open(cls.base + "/api/search?q=Unicode", timeout=5))["results"][0]["id"]

    @classmethod
    def tearDownClass(cls):
        try:
            request = urllib.request.Request(cls.base + "/api/shutdown", data=b"{}",
                                             headers={"X-AllFiles-Token": cls.token})
            OPENER.open(request, timeout=10).close()
            cls.server.wait(timeout=10)
        finally:
            if cls.server.poll() is None:
                cls.server.kill()
                cls.server.wait(timeout=5)
            cls.server.stdout.close()
            cls.server.stderr.close()
            cls.temporary.cleanup()

    def request(self, path, method="GET", raw=None, headers=None):
        h = {"Origin": self.base, "X-AllFiles-Token": self.token}
        h.update(headers or {})
        request = urllib.request.Request(self.base + path, data=raw, method=method, headers=h)
        try:
            response = OPENER.open(request, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, response.headers, response.read()

    def bridge(self, raw=False):
        bridge = Bridge(self.base, raw)
        self.addCleanup(bridge.close)
        bridge.initialize()
        return bridge

    def test_http_security_and_validation_matrix(self):
        for headers in [{"Host": "evil.invalid"}, {"Host": "localhost:1234"},
                        {"Origin": "https://evil.invalid"}, {"Origin": "null"},
                        {"Sec-Fetch-Site": "cross-site"}]:
            with self.subTest(headers=headers):
                self.assertEqual(self.request("/api/status", headers=headers)[0], 403)
        for method in ["HEAD", "PUT", "DELETE", "OPTIONS", "PATCH"]:
            with self.subTest(method=method):
                self.assertEqual(self.request("/api/status", method=method)[0], 405)
        self.assertEqual(self.request("/api/cancel", method="POST", raw=b"{}",
                                      headers={"X-AllFiles-Token": "wrong"})[0], 403)
        self.assertEqual(self.request("/api/cancel")[0], 405)
        for path in ["/api/file?id=1&text_offset=-1", "/api/file?id=1&max_chars=20001",
                     "/api/file?id=1&max_chars=-1", "/api/sources?limit=201",
                     "/api/sources?offset=-1", "/api/search?mode=unknown"]:
            with self.subTest(path=path):
                self.assertEqual(self.request(path)[0], 400)
        for raw in [b"{} {}", b'{"unexpected":1}', b'{"root":null}']:
            with self.subTest(raw=raw):
                self.assertEqual(self.request("/api/scan", "POST", raw)[0], 400)
        code, headers, _ = self.request("/api/status")
        self.assertEqual(code, 200)
        for key, value in [("Cache-Control", "no-store"), ("X-Content-Type-Options", "nosniff"),
                           ("Cross-Origin-Resource-Policy", "same-origin")]:
            self.assertEqual(headers[key], value)

    def test_protocol_rejects_invalid_frames(self):
        b = self.bridge()
        for value in [[], {}, {"jsonrpc": "1.0", "id": 8, "method": "ping"},
                      {"jsonrpc": "2.0", "id": None, "method": "ping"},
                      {"jsonrpc": "2.0", "id": 1.5, "method": "ping"},
                      {"jsonrpc": "2.0", "id": {}, "method": "ping"}]:
            with self.subTest(value=value):
                b.send(value)
                self.assertEqual(b.read()["error"]["code"], -32600)
        b.process.stdin.write(b"{broken\n")
        b.process.stdin.flush()
        self.assertEqual(b.read()["error"]["code"], -32700)
        for params in [None, [], "x", {"_meta": []}]:
            with self.subTest(params=params):
                self.assertEqual(b.rpc("tools/list", params)["error"]["code"], -32602)
        self.assertIn("result", b.rpc("ping", {}))

    def test_tool_argument_and_text_bounds(self):
        b = self.bridge()
        for name, args in [("allfiles_status", {"extra": 1}), ("allfiles_sources", {"limit": 201}),
                           ("allfiles_sources", {"offset": -1}), ("allfiles_search", {"limit": 51}),
                           ("allfiles_search", {"query": None}), ("allfiles_search", {"limit": 1.5}),
                           ("allfiles_search", {"mode": "invalid"}), ("allfiles_file", {}),
                           ("allfiles_file", {"id": self.file_id, "max_chars": 20001}),
                           ("allfiles_scan", {}), ("allfiles_scan", {"root": "relative", "label": "x"})]:
            with self.subTest(name=name, arguments=args):
                self.assertTrue(b.rpc("tools/call", {"name": name, "arguments": args})["result"]["isError"])
        for offset, limit in [(0, 0), (7, 1), (7, 20000), (8388608, 10)]:
            with self.subTest(offset=offset, limit=limit):
                result = b.rpc("tools/call", {"name": "allfiles_file", "arguments": {
                    "id": self.file_id, "offset": offset, "max_chars": limit}})["result"]
                self.assertFalse(result["isError"])
                data = result["structuredContent"]
                self.assertLessEqual(len(data["text"]), limit)
                self.assertEqual(data["returned_chars"], len(data["text"]))
        self.assertIn("2026-07-28", b.rpc("server/discover", {"_meta": {
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {}}})["result"]["supportedVersions"])

    def test_request_limit_and_cancellation_release_connections(self):
        started, disconnected = queue.Queue(), queue.Queue()

        class SlowStatus(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                started.put(self.path)
                self.connection.settimeout(5)
                try:
                    disconnected.put(self.connection.recv(1) == b"")
                except OSError:
                    disconnected.put(False)

            def log_message(self, *_):
                pass

        api = http.server.ThreadingHTTPServer(("127.0.0.1", 0), SlowStatus)
        threading.Thread(target=api.serve_forever, daemon=True).start()
        b = Bridge(f"http://127.0.0.1:{api.server_port}")
        try:
            b.initialize()
            for index in range(8):
                b.send({"jsonrpc": "2.0", "id": f"slow-{index}", "method": "tools/call",
                        "params": {"name": "allfiles_status", "arguments": {}}})
            for _ in range(8):
                self.assertEqual(started.get(timeout=5), "/api/status")
            rejected = b.rpc("tools/call", {"name": "allfiles_status", "arguments": {}})
            self.assertEqual(rejected["error"]["code"], -32600)
            b.send({"jsonrpc": "2.0", "method": "notifications/cancelled",
                    "params": {"requestId": "slow-0"}})
            self.assertTrue(disconnected.get(timeout=5), "cancel must close pending HTTP request")
            b.send({"jsonrpc": "2.0", "id": "replacement", "method": "tools/call",
                    "params": {"name": "allfiles_status", "arguments": {}}})
            self.assertEqual(started.get(timeout=5), "/api/status", "cancel must free an admission slot")
            for key in [f"slow-{i}" for i in range(1, 8)] + ["replacement"]:
                b.send({"jsonrpc": "2.0", "method": "notifications/cancelled",
                        "params": {"requestId": key}})
            for _ in range(8):
                self.assertTrue(disconnected.get(timeout=5))
            self.assertIn("result", b.rpc("ping", {}))
        finally:
            b.close()
            api.shutdown()
            api.server_close()

    def test_oversized_input_is_bounded(self):
        result = subprocess.run([BINARY, "mcp", "--url", self.base],
                                input=b"x" * ((1 << 20) + 1) + b"\n", capture_output=True, timeout=5)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, b"")
        self.assertIn(b"1 MiB", result.stderr)

    def test_regression_valid_unicode_query_matches_advertised_schema(self):
        b = self.bridge()
        # maxLength=1000 is a Unicode character count, as mcp::validate confirms.
        result = b.rpc("tools/call", {"name": "allfiles_search", "arguments": {
            "query": "ž" * 501}})["result"]
        self.assertFalse(result["isError"], result)

    def test_regression_explicit_port_80_is_accepted(self):
        # EOF exits without network traffic, so no service on port 80 is needed.
        result = subprocess.run([BINARY, "mcp", "--url", "http://127.0.0.1:80"],
                                input=b"", capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr.decode(errors="replace"))

    def test_eof_drains_accepted_tool_responses(self):
        b = self.bridge()
        cases = [("eof-status", "allfiles_status", {}),
                 ("eof-search", "allfiles_search", {"query": "Unicode"}),
                 ("eof-large", "allfiles_sources", {"limit": 200})]
        for key, name, args in cases:
            b.send({"jsonrpc": "2.0", "id": key, "method": "tools/call",
                    "params": {"name": name, "arguments": args}})
        b.process.stdin.close()
        received = {}
        for _ in cases:
            response = b.read()
            self.assertFalse(response["result"]["isError"], response)
            received[response["id"]] = response["result"]["structuredContent"]
        self.assertEqual(set(received), {key for key, _, _ in cases})
        self.assertEqual(len(received["eof-large"]["sources"]), 200)
        self.assertEqual(b.process.wait(timeout=5), 0)

    def test_broken_stdout_exits_with_stdin_still_open(self):
        b = Bridge(self.base, raw=True)
        self.addCleanup(b.close)
        b.process.stdout.close()
        b.send({"jsonrpc": "2.0", "id": "broken", "method": "ping", "params": {}})
        self.assertNotEqual(b.process.wait(timeout=5), 0)
        self.assertFalse(b.process.stdin.closed, "test must not supply EOF to unblock exit")

    def test_regression_cancellation_cannot_corrupt_stdout_frames(self):
        code, _, api_bytes = self.request("/api/sources?limit=200")
        self.assertEqual(code, 200)
        self.assertLess(len(api_bytes), 2 << 20, "fixture must be a valid bounded API reply")
        b = self.bridge(raw=True)
        b.send({"jsonrpc": "2.0", "id": "large", "method": "tools/call",
                "params": {"name": "allfiles_sources", "arguments": {"limit": 200}}})
        prefix_queue = queue.Queue()
        threading.Thread(target=lambda: prefix_queue.put(b.process.stdout.read(1024)), daemon=True).start()
        prefix = prefix_queue.get(timeout=10)
        self.assertEqual(len(prefix), 1024)
        # Receiving the prefix proves writing has begun. Stop draining stdout,
        # cancel while the pipe is full, then send an independently valid frame.
        b.send({"jsonrpc": "2.0", "method": "notifications/cancelled",
                "params": {"requestId": "large"}})
        time.sleep(.15)
        b.send({"jsonrpc": "2.0", "id": "after-cancel", "method": "ping", "params": {}})
        remaining_queue = queue.Queue()
        threading.Thread(target=lambda: remaining_queue.put(b.process.stdout.readline()), daemon=True).start()
        line = prefix + remaining_queue.get(timeout=10)
        try:
            completed = json.loads(line)
        except json.JSONDecodeError as error:
            self.fail(f"Cancellation corrupted a JSON-RPC line ({len(line)} bytes); "
                      f"API reply was {len(api_bytes)} bytes. {error}")
        self.assertEqual(completed["id"], "large")
        self.assertEqual(len(completed["result"]["structuredContent"]["sources"]), 200)
        self.assertEqual(b.read()["id"], "after-cancel", "subsequent responses must survive cancellation")
        self.assertIn("result", b.rpc("ping", {}))


if __name__ == "__main__":
    unittest.main(argv=[__file__], verbosity=2)
