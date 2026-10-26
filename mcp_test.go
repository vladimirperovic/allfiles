package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
	"unicode/utf8"
)

type mcpTestResponse struct {
	ID     json.RawMessage `json:"id"`
	Result json.RawMessage `json:"result"`
	Error  *mcpRPCError    `json:"error"`
}

type mcpHarness struct {
	t         *testing.T
	input     *io.PipeWriter
	responses chan mcpTestResponse
}

func startMCPHarness(t *testing.T, base string) *mcpHarness {
	t.Helper()
	input, writer := io.Pipe()
	reader, output := io.Pipe()
	done := make(chan error, 1)
	h := &mcpHarness{t: t, input: writer, responses: make(chan mcpTestResponse, 32)}
	go func() { done <- runMCP(context.Background(), base, input, output); output.Close() }()
	go func() {
		scanner := bufio.NewScanner(reader)
		scanner.Buffer(make([]byte, 4096), 2<<20)
		for scanner.Scan() {
			var response mcpTestResponse
			if json.Unmarshal(scanner.Bytes(), &response) == nil {
				h.responses <- response
			}
		}
		close(h.responses)
	}()
	t.Cleanup(func() {
		writer.Close()
		select {
		case err := <-done:
			if err != nil {
				t.Errorf("MCP stdio ended with error: %v", err)
			}
		case <-time.After(5 * time.Second):
			t.Error("MCP stdio did not stop at EOF")
		}
		reader.Close()
	})
	return h
}

func (h *mcpHarness) send(id any, method string, params any) {
	h.t.Helper()
	message := map[string]any{"jsonrpc": "2.0", "method": method}
	if id != nil {
		message["id"] = id
	}
	if params != nil {
		message["params"] = params
	}
	if err := json.NewEncoder(h.input).Encode(message); err != nil {
		h.t.Fatal(err)
	}
}
func (h *mcpHarness) read() mcpTestResponse {
	h.t.Helper()
	select {
	case response, ok := <-h.responses:
		if !ok {
			h.t.Fatal("MCP output closed before response")
		}
		return response
	case <-time.After(5 * time.Second):
		h.t.Fatal("MCP response timeout")
	}
	return mcpTestResponse{}
}
func (h *mcpHarness) request(id any, method string, params any) mcpTestResponse {
	h.send(id, method, params)
	return h.read()
}
func (h *mcpHarness) initialize(version string) {
	h.t.Helper()
	response := h.request("init", "initialize", map[string]any{"protocolVersion": version, "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "allfiles-test", "version": "1"}})
	if response.Error != nil {
		h.t.Fatalf("initialize: %#v", response.Error)
	}
	h.send(nil, "notifications/initialized", nil)
}

func mcpTestAppServer(t *testing.T) (*app, *httptest.Server) {
	t.Helper()
	a := newAPITestApp(t)
	server := httptest.NewServer(a)
	a.base = server.URL
	t.Cleanup(server.Close)
	return a, server
}
func mcpModernParams(values map[string]any) map[string]any {
	if values == nil {
		values = map[string]any{}
	}
	values["_meta"] = map[string]any{"io.modelcontextprotocol/protocolVersion": mcpModernVersion, "io.modelcontextprotocol/clientCapabilities": map[string]any{}, "io.modelcontextprotocol/clientInfo": map[string]string{"name": "modern-test", "version": "1"}}
	return values
}

func mcpPayload(t *testing.T, response mcpTestResponse) map[string]any {
	t.Helper()
	if response.Error != nil {
		t.Fatalf("unexpected RPC error: %#v", response.Error)
	}
	var result struct {
		IsError bool `json:"isError"`
		Content []struct {
			Text string `json:"text"`
		} `json:"content"`
		Structured map[string]any `json:"structuredContent"`
	}
	if err := json.Unmarshal(response.Result, &result); err != nil {
		t.Fatal(err)
	}
	if result.IsError {
		t.Fatalf("tool failed: %s", response.Result)
	}
	if result.Structured != nil {
		return result.Structured
	}
	var data map[string]any
	if len(result.Content) != 1 || json.Unmarshal([]byte(result.Content[0].Text), &data) != nil {
		t.Fatalf("missing tool payload: %s", response.Result)
	}
	return data
}

func TestMCPLegacyStdioScanSearchAndRescanRoundtrip(t *testing.T) {
	a, server := mcpTestAppServer(t)
	h := startMCPHarness(t, server.URL)
	h.initialize(mcpLegacyVersion)
	list := h.request(1, "tools/list", map[string]any{})
	var listed struct {
		Tools []map[string]any `json:"tools"`
	}
	if list.Error != nil || json.Unmarshal(list.Result, &listed) != nil || len(listed.Tools) != 6 {
		t.Fatalf("tools/list: %s %#v", list.Result, list.Error)
	}
	for _, tool := range listed.Tools {
		if tool["name"] == "allfiles_scan" {
			annotations := tool["annotations"].(map[string]any)
			if annotations["readOnlyHint"] != false || annotations["destructiveHint"] != true {
				t.Fatal("scan annotations hide catalog mutations")
			}
		}
	}
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "mcp-note.txt"), []byte("mcpneedle walnut cabinetry"), 0600); err != nil {
		t.Fatal(err)
	}
	scan := mcpPayload(t, h.request(2, "tools/call", map[string]any{"name": "allfiles_scan", "arguments": map[string]any{"root": root, "label": "MCP archive", "kind": "external", "location": "Office shelf"}}))
	if scan["job"] == nil {
		t.Fatalf("scan did not return asynchronous job: %#v", scan)
	}
	a.wg.Wait()
	status := mcpPayload(t, h.request(3, "tools/call", map[string]any{"name": "allfiles_status", "arguments": map[string]any{}}))
	if status["job"].(map[string]any)["status"] != "completed" {
		t.Fatalf("scan failed: %#v", status)
	}
	sources := mcpPayload(t, h.request(4, "tools/call", map[string]any{"name": "allfiles_sources", "arguments": map[string]any{}}))
	items := sources["sources"].([]any)
	if len(items) != 1 {
		t.Fatalf("sources: %#v", sources)
	}
	source := items[0].(map[string]any)
	search := mcpPayload(t, h.request(5, "tools/call", map[string]any{"name": "allfiles_search", "arguments": map[string]any{"query": "mcpneedle", "mode": "content"}}))
	if search["total"] != float64(1) {
		t.Fatalf("content was not indexed by default: %#v", search)
	}
	if err := os.WriteFile(filepath.Join(root, "new-note.txt"), []byte("new file"), 0600); err != nil {
		t.Fatal(err)
	}
	mcpPayload(t, h.request(6, "tools/call", map[string]any{"name": "allfiles_scan", "arguments": map[string]any{"source_id": source["id"], "index_content": false}}))
	a.wg.Wait()
	updated, err := a.store.Sources()
	if err != nil || len(updated) != 1 || updated[0].FileCount != 2 || updated[0].Kind != "external" || updated[0].Location != "Office shelf" {
		t.Fatalf("rescan did not preserve source settings: %#v %v", updated, err)
	}
	mcpPayload(t, h.request(7, "tools/call", map[string]any{"name": "allfiles_cancel", "arguments": map[string]any{}}))
}

func TestMCPModernDiscoveryAndPerRequestMetadata(t *testing.T) {
	_, server := mcpTestAppServer(t)
	h := startMCPHarness(t, server.URL)
	response := h.request(1, "server/discover", mcpModernParams(nil))
	var discovery map[string]any
	if response.Error != nil || json.Unmarshal(response.Result, &discovery) != nil || discovery["resultType"] != "complete" {
		t.Fatalf("modern discovery failed: %s %#v", response.Result, response.Error)
	}
	versions := discovery["supportedVersions"].([]any)
	if versions[0] != mcpModernVersion {
		t.Fatalf("wrong modern version: %#v", versions)
	}
	response = h.request(2, "tools/list", mcpModernParams(nil))
	if response.Error != nil || !bytes.Contains(response.Result, []byte(`"resultType":"complete"`)) {
		t.Fatalf("modern list failed: %s", response.Result)
	}
	mcpPayload(t, h.request(3, "tools/call", mcpModernParams(map[string]any{"name": "allfiles_status", "arguments": map[string]any{}})))
	missingCaps := map[string]any{"_meta": map[string]any{"io.modelcontextprotocol/protocolVersion": mcpModernVersion}}
	response = h.request(4, "tools/list", missingCaps)
	if response.Error == nil || response.Error.Code != -32602 {
		t.Fatal("missing modern capabilities accepted")
	}
	unknown := map[string]any{"_meta": map[string]any{"io.modelcontextprotocol/protocolVersion": "2099-01-01", "io.modelcontextprotocol/clientCapabilities": map[string]any{}}}
	response = h.request(5, "server/discover", unknown)
	if response.Error == nil || response.Error.Code != -32022 || response.Error.Data == nil {
		t.Fatal("unsupported modern protocol not identified")
	}
	response = h.request(6, "ping", mcpModernParams(nil))
	if response.Error == nil || response.Error.Code != -32601 {
		t.Fatal("modern-era removed ping unexpectedly supported")
	}
}

func TestMCPNegotiatesLegacyVersionsAndIgnoresNotifications(t *testing.T) {
	for _, version := range []string{"2025-03-26", "2025-06-18", mcpLegacyVersion, "unknown"} {
		t.Run(version, func(t *testing.T) {
			h := startMCPHarness(t, "http://127.0.0.1:1")
			response := h.request(1, "initialize", map[string]any{"protocolVersion": version, "capabilities": map[string]any{}, "clientInfo": map[string]string{"name": "test", "version": "1"}})
			var result map[string]any
			if response.Error != nil || json.Unmarshal(response.Result, &result) != nil {
				t.Fatalf("initialize: %#v", response.Error)
			}
			want := version
			if version == "unknown" {
				want = mcpLegacyVersion
			}
			if result["protocolVersion"] != want {
				t.Fatalf("version=%v want=%s", result["protocolVersion"], want)
			}
			h.send(nil, "notifications/initialized", nil)
			h.send(nil, "notifications/unknown", map[string]any{})
			response = h.request(2, "ping", nil)
			if string(response.ID) != "2" || response.Error != nil {
				t.Fatal("notification generated a response or ping failed")
			}
			response = h.request(3, "unknown/method", nil)
			if response.Error == nil || response.Error.Code != -32601 {
				t.Fatal("unknown method did not return JSON-RPC method error")
			}
			response = h.request(4, "tools/call", map[string]any{"name": "allfiles_status", "arguments": map[string]any{}})
			if response.Error != nil || !bytes.Contains(response.Result, []byte(`"isError":true`)) {
				t.Fatalf("offline backend did not return tool error: %s", response.Result)
			}
		})
	}
}

func TestMCPRejectsUnsafeURLsRedirectsAndProxies(t *testing.T) {
	for _, raw := range []string{"https://127.0.0.1:8787", "http://localhost:8787", "http://127.0.0.2:8787", "http://[::1]:8787", "http://example.com:8787", "http://user:pass@127.0.0.1:8787", "http://127.0.0.1:8787/path", "http://127.0.0.1:8787?query=secret", "http://127.0.0.1:8787#part", "http://127.0.0.1:8787?", "http://127.0.0.1:0", "http://127.0.0.1:65536", "http://127.0.0.1", "http://127.0.0.1:08787"} {
		if _, err := newMCPBridge(raw, io.Discard); err == nil {
			t.Errorf("unsafe URL accepted: %s", raw)
		}
	}
	var redirected atomic.Int32
	trap := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { redirected.Add(1); w.Write([]byte(`{}`)) }))
	defer trap.Close()
	t.Setenv("HTTP_PROXY", trap.URL)
	t.Setenv("HTTPS_PROXY", trap.URL)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { http.Redirect(w, r, trap.URL, http.StatusFound) }))
	defer server.Close()
	b, err := newMCPBridge(server.URL, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer b.client.CloseIdleConnections()
	if _, err = b.callTool(context.Background(), "allfiles_status", json.RawMessage(`{}`)); err == nil {
		t.Fatal("redirect was followed")
	}
	if redirected.Load() != 0 {
		t.Fatal("private API request escaped through redirect or environment proxy")
	}
	if b.client.Transport.(*http.Transport).Proxy != nil {
		t.Fatal("environment HTTP proxy is enabled")
	}
}

func TestMCPToolArgumentValidationAndBoundedDocumentText(t *testing.T) {
	a, server := mcpTestAppServer(t)
	b, err := newMCPBridge(server.URL, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer b.client.CloseIdleConnections()
	_, file := testAPIFile(t, a, t.TempDir(), "", "large.txt", strings.Repeat("ž", 25000))
	raw, _ := json.Marshal(map[string]any{"id": file.ID, "offset": 10, "max_chars": 12})
	result, err := b.callTool(context.Background(), "allfiles_file", raw)
	if err != nil {
		t.Fatal(err)
	}
	if utf8.RuneCountInString(result["text"].(string)) != 12 || result["has_more"] != true || result["total_chars"] != 25000 {
		t.Fatalf("text not bounded by characters: %#v", result)
	}
	for _, test := range []struct{ name, arguments string }{
		{"allfiles_search", `{"limit":51}`}, {"allfiles_search", `{"offset":-1}`}, {"allfiles_search", `{"mode":"anything"}`}, {"allfiles_search", `{"Limit":10}`}, {"allfiles_search", `{"query":null}`}, {"allfiles_search", `{"limit":1.5}`},
		{"allfiles_status", `{"secret":"unknown"}`}, {"allfiles_status", `null`}, {"allfiles_file", `{}`}, {"allfiles_file", `{"id":1,"max_chars":20001}`},
		{"allfiles_scan", `{}`}, {"allfiles_scan", `{"root":"relative","label":"test"}`}, {"allfiles_scan", `{"root":"/tmp","label":"test","index_content":null}`}, {"allfiles_scan", `{"source_id":"missing","root":"/tmp","label":"test"}`},
	} {
		if _, err := b.callTool(context.Background(), test.name, json.RawMessage(test.arguments)); err == nil {
			t.Errorf("invalid arguments accepted for %s: %s", test.name, test.arguments)
		}
	}
	if job := a.jobSnapshot(); job != nil {
		t.Fatalf("invalid scan arguments started a job: %#v", job)
	}
}

func TestMCPCancellationInterruptsInFlightProxyRequest(t *testing.T) {
	started := make(chan struct{}, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { started <- struct{}{}; <-r.Context().Done() }))
	defer server.Close()
	h := startMCPHarness(t, server.URL)
	h.initialize(mcpLegacyVersion)
	h.send("slow", "tools/call", map[string]any{"name": "allfiles_status", "arguments": map[string]any{}})
	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("proxy request did not start")
	}
	h.send(nil, "notifications/cancelled", map[string]any{"requestId": "slow", "reason": "test"})
	response := h.read()
	if string(response.ID) != `"slow"` || response.Error != nil || !bytes.Contains(response.Result, []byte(`"isError":true`)) {
		t.Fatalf("cancel did not end request: %s %#v", response.Result, response.Error)
	}
}

func TestMCPFileRequestsBoundedTextAtServer(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		q := r.URL.Query()
		if r.Method != http.MethodGet || r.URL.Path != "/api/file" || q.Get("id") != "42" || q.Get("text_offset") != "123" || q.Get("max_chars") != "0" {
			http.Error(w, "file text was not bounded at the API", http.StatusBadRequest)
			return
		}
		jsonReply(w, 200, map[string]any{"file": map[string]any{"id": 42}, "text": "", "text_status": "indexed", "offset": 123, "returned_chars": 0, "has_more": true})
	}))
	defer server.Close()
	b, err := newMCPBridge(server.URL, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer b.client.CloseIdleConnections()
	result, err := b.callTool(context.Background(), "allfiles_file", json.RawMessage(`{"id":42,"offset":123,"max_chars":0}`))
	if err != nil || requests.Load() != 1 || result["text"] != "" || result["offset"] != 123 || result["has_more"] != true {
		t.Fatalf("metadata-only request: %#v %v, requests=%d", result, err, requests.Load())
	}
	if _, present := result["total_chars"]; present {
		t.Fatal("metadata-only request invented a total character count")
	}
}

func TestMCPFileRejectsUnboundedLegacyServerResponse(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		jsonReply(w, 200, map[string]any{"file": map[string]any{"id": 42}, "text": strings.Repeat("private document ", 100), "text_status": "indexed"})
	}))
	defer server.Close()
	b, err := newMCPBridge(server.URL, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer b.client.CloseIdleConnections()
	for _, raw := range []string{`{"id":42,"max_chars":10}`, `{"id":42,"max_chars":0}`} {
		result, err := b.callTool(context.Background(), "allfiles_file", json.RawMessage(raw))
		if err == nil || result != nil {
			t.Fatalf("unbounded private text from old server was forwarded: %#v %v", result, err)
		}
	}
}

func TestMCPRejectsMalformedProtocolAndBoundsInput(t *testing.T) {
	h := startMCPHarness(t, "http://127.0.0.1:1")
	if _, err := io.WriteString(h.input, "{broken\n"); err != nil {
		t.Fatal(err)
	}
	if response := h.read(); response.Error == nil || response.Error.Code != -32700 {
		t.Fatal("invalid JSON not rejected")
	}
	for _, line := range []string{`[]`, `{"jsonrpc":"2.0","id":null,"method":"ping"}`, `{"jsonrpc":"2.0","id":1.5,"method":"ping"}`, `{"jsonrpc":"1.0","id":1,"method":"ping"}`} {
		if _, err := io.WriteString(h.input, line+"\n"); err != nil {
			t.Fatal(err)
		}
		if response := h.read(); response.Error == nil || response.Error.Code != -32600 {
			t.Fatalf("invalid request not rejected: %s", line)
		}
	}
	var output bytes.Buffer
	if err := runMCP(context.Background(), "http://127.0.0.1:1", strings.NewReader(strings.Repeat("x", mcpMaxMessage+1)+"\n"), &output); err == nil {
		t.Fatal("oversized stdio message accepted")
	}
	if output.Len() != 0 {
		t.Fatal("oversized message produced non-protocol stdout")
	}
}

func TestMCPContextCancellationUnblocksStdin(t *testing.T) {
	input, writer := io.Pipe()
	defer writer.Close()
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- runMCP(ctx, "http://127.0.0.1:1", input, io.Discard) }()
	cancel()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("MCP ignored process cancellation while waiting for stdin")
	}
}

func TestMCPSourcesArePagedAtAPIAndRescanUsesDirectID(t *testing.T) {
	root := t.TempDir()
	var paged, direct, scanned atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/api/sources":
			if r.URL.Query().Get("id") == "disc-9000" {
				direct.Add(1)
				jsonReply(w, 200, map[string]any{"sources": []Source{{ID: "disc-9000", Root: root, Label: "Disc 9000", Kind: "bluray"}}, "total": 1})
				return
			}
			if r.URL.Query().Get("offset") == "9000" && r.URL.Query().Get("limit") == "1" {
				paged.Add(1)
				jsonReply(w, 200, map[string]any{"sources": []Source{{ID: "disc-9000", Root: root, Label: "Disc 9000", Kind: "bluray"}}, "total": 10000, "offset": 9000, "limit": 1, "has_more": true})
				return
			}
			http.Error(w, "unbounded source request is forbidden in this test", 400)
		case "/api/session":
			jsonReply(w, 200, map[string]string{"token": "test-token"})
		case "/api/scan":
			var payload struct {
				SourceID string `json:"source_id"`
				Root     string `json:"root"`
			}
			if json.NewDecoder(r.Body).Decode(&payload) != nil || payload.SourceID != "disc-9000" || payload.Root != root || r.Header.Get("X-AllFiles-Token") != "test-token" {
				http.Error(w, "invalid scan", 400)
				return
			}
			scanned.Add(1)
			jsonReply(w, 202, map[string]any{"job": map[string]string{"status": "running"}})
		default:
			http.NotFound(w, r)
		}
	}))
	defer server.Close()
	b, err := newMCPBridge(server.URL, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	defer b.client.CloseIdleConnections()
	page, err := b.callTool(context.Background(), "allfiles_sources", json.RawMessage(`{"offset":9000,"limit":1}`))
	if err != nil || page["total"] != float64(10000) || page["has_more"] != true {
		t.Fatalf("paged sources failed: %#v %v", page, err)
	}
	if _, err := b.callTool(context.Background(), "allfiles_scan", json.RawMessage(`{"source_id":"disc-9000"}`)); err != nil {
		t.Fatal(err)
	}
	if paged.Load() != 1 || direct.Load() != 1 || scanned.Load() != 1 {
		t.Fatalf("unexpected requests: paged=%d direct=%d scan=%d", paged.Load(), direct.Load(), scanned.Load())
	}
}
