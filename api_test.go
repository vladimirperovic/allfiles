package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"testing"
	"testing/fstest"
)

func newAPITestApp(t *testing.T) *app {
	t.Helper()
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { store.Close() })
	a := newApp(store, "http://127.0.0.1:38217")
	a.assets = fstest.MapFS{"index.html": &fstest.MapFile{Data: []byte("<!doctype html><title>AllFiles test</title>")}}
	t.Cleanup(a.cancelAndWait)
	return a
}

func apiTestRequest(a *app, method, path, body string, headers map[string]string) *httptest.ResponseRecorder {
	r := httptest.NewRequest(method, a.base+path, strings.NewReader(body))
	for key, value := range headers {
		if key == "Host" {
			r.Host = value
		} else {
			r.Header.Set(key, value)
		}
	}
	w := httptest.NewRecorder()
	a.ServeHTTP(w, r)
	return w
}

func testAPIFile(t *testing.T, a *app, root, dir, name, body string) (Source, IndexedFile) {
	t.Helper()
	source, err := a.store.BeginScan(ScanOptions{Root: root, Label: "Test archive", Kind: "external", Location: "Test shelf"})
	if err != nil {
		t.Fatal(err)
	}
	files, err := a.store.UpsertFiles(source.ID, "test-scan", []ScannedFile{{Relative: filepath.ToSlash(filepath.Join(dir, name)), Dir: dir, Name: name, Extension: strings.TrimPrefix(filepath.Ext(name), "."), Size: int64(len(body)), Modified: 1720000000, ModifiedNS: 1720000000000000000}})
	if err != nil || len(files) != 1 {
		t.Fatalf("insert file: %v, %#v", err, files)
	}
	if body != "" {
		if err := a.store.RecordContent(files[0].ID, files[0].Version, "indexed", body, ""); err != nil {
			t.Fatal(err)
		}
	}
	if err := a.store.FinishScan(source.ID, "test-scan", true); err != nil {
		t.Fatal(err)
	}
	return source, files[0]
}

func TestAPIRejectsCrossOriginAndInvalidHost(t *testing.T) {
	a := newAPITestApp(t)
	tests := []struct {
		name    string
		path    string
		headers map[string]string
	}{
		{"DNS rebinding host", "/api/session", map[string]string{"Host": "attacker.example"}},
		{"different loopback host", "/api/session", map[string]string{"Host": "localhost:38217"}},
		{"wrong port", "/api/session", map[string]string{"Host": "127.0.0.1:38218"}},
		{"foreign origin", "/api/status", map[string]string{"Origin": "https://attacker.example"}},
		{"opaque file origin", "/api/session", map[string]string{"Origin": "null"}},
		{"different local origin", "/api/session", map[string]string{"Origin": "http://127.0.0.1:38218"}},
		{"cross-site metadata", "/api/sources", map[string]string{"Sec-Fetch-Site": "cross-site"}},
		{"asset origin", "/", map[string]string{"Origin": "https://attacker.example"}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			w := apiTestRequest(a, http.MethodGet, test.path, "", test.headers)
			if w.Code != http.StatusForbidden {
				t.Fatalf("status = %d, want 403: %s", w.Code, w.Body.String())
			}
			if strings.Contains(w.Body.String(), a.token) {
				t.Fatal("rejected request disclosed session token")
			}
		})
	}
	w := apiTestRequest(a, http.MethodGet, "/api/session", "", map[string]string{"Origin": a.base, "Sec-Fetch-Site": "same-origin"})
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), a.token) {
		t.Fatalf("same-origin session failed: %d %s", w.Code, w.Body.String())
	}
	if w.Header().Get("Cache-Control") != "no-store" || w.Header().Get("Referrer-Policy") != "no-referrer" {
		t.Fatal("sensitive API response may be cached or leak referrers")
	}
	if !strings.Contains(w.Header().Get("Content-Security-Policy"), "frame-ancestors 'none'") || w.Header().Get("Access-Control-Allow-Origin") != "" {
		t.Fatal("missing frame protection or unexpected cross-origin access")
	}
}

func TestAPIMutationsRequireTokenAndCorrectMethod(t *testing.T) {
	a := newAPITestApp(t)
	for _, path := range []string{"/api/scan", "/api/import", "/api/cancel", "/api/export", "/api/open", "/api/shutdown"} {
		t.Run(path, func(t *testing.T) {
			for _, token := range []string{"", "incorrect"} {
				w := apiTestRequest(a, http.MethodPost, path, "{}", map[string]string{"X-AllFiles-Token": token})
				if w.Code != http.StatusForbidden {
					t.Fatalf("invalid token status = %d, want 403", w.Code)
				}
			}
			w := apiTestRequest(a, http.MethodGet, path, "", nil)
			if w.Code != http.StatusMethodNotAllowed {
				t.Fatalf("GET mutation status = %d, want 405", w.Code)
			}
		})
	}
	for _, method := range []string{http.MethodDelete, http.MethodPut, http.MethodOptions, http.MethodHead} {
		w := apiTestRequest(a, method, "/api/status", "", map[string]string{"X-AllFiles-Token": a.token})
		if w.Code != http.StatusMethodNotAllowed {
			t.Fatalf("%s API status = %d, want 405", method, w.Code)
		}
	}
	w := apiTestRequest(a, http.MethodPost, "/api/status", "{}", map[string]string{"X-AllFiles-Token": a.token})
	if w.Code != http.StatusMethodNotAllowed {
		t.Fatalf("POST read-only endpoint = %d, want 405", w.Code)
	}
	w = apiTestRequest(a, http.MethodPost, "/api/cancel", "{}", map[string]string{"X-AllFiles-Token": a.token, "Origin": a.base})
	if w.Code != http.StatusOK {
		t.Fatalf("valid token rejected: %d %s", w.Code, w.Body.String())
	}
}

func TestAPIRejectsMalformedAndOversizedJSON(t *testing.T) {
	a := newAPITestApp(t)
	for _, body := range []string{
		"{",
		`{"unknown":true}`,
		`{"root":123}`,
		`{} {}`,
		`{"root":"relative/folder"}`,
		`{"root":"import://archive"}`,
		`{"root":"` + strings.Repeat("a", (1<<20)+1) + `"}`,
	} {
		w := apiTestRequest(a, http.MethodPost, "/api/scan", body, map[string]string{"X-AllFiles-Token": a.token})
		if w.Code != http.StatusBadRequest {
			t.Fatalf("invalid JSON/path status = %d, want 400 (body length %d)", w.Code, len(body))
		}
		if a.jobSnapshot() != nil {
			t.Fatal("invalid input started a background job")
		}
	}
}

func TestAPISearchFileAndSourceResponses(t *testing.T) {
	a := newAPITestApp(t)
	source, file := testAPIFile(t, a, t.TempDir(), "Projects", "Kuhinja plan.txt", "A distinctive archive phrase about walnut cabinetry.")
	for _, query := range []string{
		"/api/search?q=" + url.QueryEscape("Kuhinja") + "&mode=files",
		"/api/search?q=" + url.QueryEscape("distinctive archive") + "&mode=content",
		"/api/search?q=" + url.QueryEscape(`"walnut cabinetry"`) + "&mode=content&source=" + source.ID + "&ext=txt",
	} {
		w := apiTestRequest(a, http.MethodGet, query, "", nil)
		var result SearchResult
		if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &result) != nil || result.Total != 1 || len(result.Results) != 1 || result.Results[0].ID != file.ID {
			t.Fatalf("search failed: %d %s", w.Code, w.Body.String())
		}
	}
	w := apiTestRequest(a, http.MethodGet, "/api/file?id="+strconv.FormatInt(file.ID, 10), "", nil)
	var detail struct {
		File FileResult `json:"file"`
		Text string     `json:"text"`
	}
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &detail) != nil || detail.File.Name != "Kuhinja plan.txt" || !strings.Contains(detail.Text, "walnut cabinetry") {
		t.Fatalf("file response failed: %d %s", w.Code, w.Body.String())
	}
	w = apiTestRequest(a, http.MethodGet, "/api/sources", "", nil)
	var sources struct {
		Sources []Source `json:"sources"`
	}
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &sources) != nil || len(sources.Sources) != 1 || sources.Sources[0].ID != source.ID || sources.Sources[0].FileCount != 1 {
		t.Fatalf("sources response failed: %d %s", w.Code, w.Body.String())
	}
	for _, path := range []string{"/api/file?id=999999", "/api/file?id=invalid", "/api/does-not-exist"} {
		if w := apiTestRequest(a, http.MethodGet, path, "", nil); w.Code != http.StatusNotFound {
			t.Fatalf("missing record %s status = %d", path, w.Code)
		}
	}
	for _, query := range []string{"/api/search?mode=invalid", "/api/search?q=" + strings.Repeat("x", 1001)} {
		if w := apiTestRequest(a, http.MethodGet, query, "", nil); w.Code != http.StatusBadRequest {
			t.Fatalf("invalid search status = %d", w.Code)
		}
	}
}

func TestAPISourcesPageAndDirectIDPreserveLegacyResponse(t *testing.T) {
	a := newAPITestApp(t)
	tx, err := a.store.conn.BeginTx(context.Background(), nil)
	if err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 205; index++ {
		id := fmt.Sprintf("source-%03d", index)
		if _, err := tx.Exec(`INSERT INTO sources(id,root,label,kind) VALUES(?,?,?,'bluray')`, id, "import://"+id, fmt.Sprintf("Disc %03d", index)); err != nil {
			tx.Rollback()
			t.Fatal(err)
		}
	}
	if err := tx.Commit(); err != nil {
		t.Fatal(err)
	}
	if _, err := a.store.UpsertFiles("source-200", "test", []ScannedFile{{Name: "plan.pdf", Relative: "plan.pdf", Extension: "pdf", Size: 123}}); err != nil {
		t.Fatal(err)
	}
	w := apiTestRequest(a, http.MethodGet, "/api/sources?offset=200&limit=2", "", nil)
	var page struct {
		Sources []Source `json:"sources"`
		Total   int      `json:"total"`
		Offset  int      `json:"offset"`
		Limit   int      `json:"limit"`
		HasMore bool     `json:"has_more"`
	}
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &page) != nil || len(page.Sources) != 2 || page.Total != 205 || !page.HasMore || page.Sources[0].ID != "source-200" || page.Sources[0].FileCount != 1 || page.Sources[0].TotalSize != 123 {
		t.Fatalf("source page failed: %d %s", w.Code, w.Body.String())
	}
	w = apiTestRequest(a, http.MethodGet, "/api/sources?id=source-204", "", nil)
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &page) != nil || len(page.Sources) != 1 || page.Total != 1 || page.HasMore || page.Sources[0].ID != "source-204" {
		t.Fatalf("direct source lookup failed: %d %s", w.Code, w.Body.String())
	}
	w = apiTestRequest(a, http.MethodGet, "/api/sources?id=not-found", "", nil)
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &page) != nil || len(page.Sources) != 0 || page.Total != 0 {
		t.Fatalf("missing source lookup failed: %s", w.Body.String())
	}
	w = apiTestRequest(a, http.MethodGet, "/api/sources", "", nil)
	if w.Code != 200 || json.Unmarshal(w.Body.Bytes(), &page) != nil || len(page.Sources) != 205 {
		t.Fatalf("legacy source response changed: %d %s", w.Code, w.Body.String())
	}
	for _, query := range []string{"limit=201", "limit=0", "offset=-1", "offset=invalid", "limit=invalid"} {
		if w := apiTestRequest(a, http.MethodGet, "/api/sources?"+query, "", nil); w.Code != 400 {
			t.Fatalf("invalid source pagination accepted: %s", query)
		}
	}
}

func TestAPICatalogOnlyCopyRetainsFileDetailsAndCanReindex(t *testing.T) {
	a := newAPITestApp(t)
	root := t.TempDir()
	if err := os.WriteFile(filepath.Join(root, "notes.txt"), []byte("recoverable content search"), 0600); err != nil {
		t.Fatal(err)
	}
	options := ScanOptions{Root: root, Label: "Portable", IndexContent: true}
	if err := Scan(context.Background(), a.store, options, nil); err != nil {
		t.Fatal(err)
	}
	result, err := a.store.Search(context.Background(), SearchOptions{Query: "notes", Mode: "files"})
	if err != nil || len(result.Results) != 1 {
		t.Fatalf("original file missing: %v %#v", err, result)
	}
	destination := filepath.Join(t.TempDir(), "catalog-only")
	if _, err := a.store.Export(destination, false); err != nil {
		t.Fatal(err)
	}
	copyStore, err := OpenStore(destination)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { copyStore.Close() })
	copyApp := newApp(copyStore, a.base)
	w := apiTestRequest(copyApp, http.MethodGet, "/api/file?id="+strconv.FormatInt(result.Results[0].ID, 10), "", nil)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "notes.txt") {
		t.Fatalf("catalog-only metadata unavailable: %d %s", w.Code, w.Body.String())
	}
	if _, err := os.Stat(filepath.Join(destination, "content.sqlite")); !os.IsNotExist(err) {
		t.Fatalf("metadata lookup created content database: %v", err)
	}
	if err := Scan(context.Background(), copyStore, options, nil); err != nil {
		t.Fatal(err)
	}
	rebuilt, err := copyStore.Search(context.Background(), SearchOptions{Query: "recoverable", Mode: "content"})
	if err != nil || rebuilt.Total != 1 {
		t.Fatalf("catalog-only copy did not rebuild unchanged document content: %v %#v", err, rebuilt)
	}
}

func TestAPIFileDetailsSurviveSeparatelyMissingContentDatabase(t *testing.T) {
	a := newAPITestApp(t)
	_, file := testAPIFile(t, a, t.TempDir(), "", "portable.txt", "content stored separately")
	// Simulate someone copying only the catalog, without the export command's
	// status adjustment. Metadata must remain useful when content is unavailable.
	catalog, err := os.ReadFile(filepath.Join(a.store.DataDir, "catalog.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	destination := t.TempDir()
	if err := os.WriteFile(filepath.Join(destination, "catalog.sqlite"), catalog, 0600); err != nil {
		t.Fatal(err)
	}
	copyStore, err := OpenStore(destination)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { copyStore.Close() })
	copyApp := newApp(copyStore, a.base)
	w := apiTestRequest(copyApp, http.MethodGet, "/api/file?id="+strconv.FormatInt(file.ID, 10), "", nil)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), "portable.txt") || strings.Contains(w.Body.String(), "content stored separately") {
		t.Fatalf("metadata-only file detail failed: %d %s", w.Code, w.Body.String())
	}
	w = apiTestRequest(copyApp, http.MethodGet, "/api/search?q=content&mode=content", "", nil)
	var result SearchResult
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &result) != nil || result.Total != 0 {
		t.Fatalf("missing content search failed: %d %s", w.Code, w.Body.String())
	}
}

func TestAPIMissingContentRebuildSpansMultipleScanBatches(t *testing.T) {
	a := newAPITestApp(t)
	root := t.TempDir()
	const total = 300 // The scanner flushes batches of 256 files.
	for index := 0; index < total; index++ {
		name := fmt.Sprintf("document-%03d.txt", index)
		if err := os.WriteFile(filepath.Join(root, name), []byte(fmt.Sprintf("recoverable archive document %d", index)), 0600); err != nil {
			t.Fatal(err)
		}
	}
	options := ScanOptions{Root: root, Label: "Large archive", IndexContent: true}
	if err := Scan(context.Background(), a.store, options, nil); err != nil {
		t.Fatal(err)
	}
	catalog, err := os.ReadFile(filepath.Join(a.store.DataDir, "catalog.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	destination := t.TempDir()
	if err := os.WriteFile(filepath.Join(destination, "catalog.sqlite"), catalog, 0600); err != nil {
		t.Fatal(err)
	}
	copyStore, err := OpenStore(destination)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { copyStore.Close() })
	if err := Scan(context.Background(), copyStore, options, nil); err != nil {
		t.Fatal(err)
	}
	result, err := copyStore.Search(context.Background(), SearchOptions{Query: "recoverable", Mode: "content"})
	if err != nil || result.Total != total {
		t.Fatalf("only part of the missing content database was rebuilt: total=%d want=%d error=%v", result.Total, total, err)
	}
	// Also handle an existing content database with one lost link: file details
	// stay available and the next incremental scan repairs the missing text.
	fileID := result.Results[0].ID
	if _, err := copyStore.conn.ExecContext(context.Background(), `DELETE FROM content.links WHERE file_id=?`, fileID); err != nil {
		t.Fatal(err)
	}
	copyApp := newApp(copyStore, a.base)
	w := apiTestRequest(copyApp, http.MethodGet, "/api/file?id="+strconv.FormatInt(fileID, 10), "", nil)
	if w.Code != http.StatusOK || !strings.Contains(w.Body.String(), `"text_status":"not_indexed"`) {
		t.Fatalf("missing-link file metadata unavailable: %d %s", w.Code, w.Body.String())
	}
	if err := Scan(context.Background(), copyStore, options, nil); err != nil {
		t.Fatal(err)
	}
	result, err = copyStore.Search(context.Background(), SearchOptions{Query: "recoverable", Mode: "content"})
	if err != nil || result.Total != total {
		t.Fatalf("missing link was not repaired: total=%d want=%d error=%v", result.Total, total, err)
	}
}

func TestAPIContentRejectsDatabaseFromAnotherCatalog(t *testing.T) {
	a := newAPITestApp(t)
	testAPIFile(t, a, t.TempDir(), "", "local.txt", "")
	other := newAPITestApp(t)
	testAPIFile(t, other, t.TempDir(), "", "foreign.txt", "foreign confidential phrase")
	content, err := os.ReadFile(filepath.Join(other.store.DataDir, "content.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(a.store.DataDir, "content.sqlite"), content, 0600); err != nil {
		t.Fatal(err)
	}
	w := apiTestRequest(a, http.MethodGet, "/api/search?q=confidential&mode=content", "", nil)
	if w.Code != http.StatusBadRequest || strings.Contains(w.Body.String(), "foreign confidential phrase") {
		t.Fatalf("mismatched content accepted or leaked: %d %s", w.Code, w.Body.String())
	}
	w = apiTestRequest(a, http.MethodGet, "/api/search?q=local&mode=files", "", nil)
	var result SearchResult
	if w.Code != http.StatusOK || json.Unmarshal(w.Body.Bytes(), &result) != nil || result.Total != 1 {
		t.Fatalf("wrong content database broke metadata search: %d %s", w.Code, w.Body.String())
	}
}

func TestAPIOpenRejectsTraversalAndOfflineSourcesWithoutLaunching(t *testing.T) {
	for _, offlineRoot := range []string{"import://archived", "demo://synthetic"} {
		t.Run(offlineRoot, func(t *testing.T) {
			a := newAPITestApp(t)
			_, file := testAPIFile(t, a, offlineRoot, "", "offline.txt", "")
			body := `{"id":` + strconv.FormatInt(file.ID, 10) + `}`
			w := apiTestRequest(a, http.MethodPost, "/api/open", body, map[string]string{"X-AllFiles-Token": a.token})
			if w.Code != http.StatusBadRequest {
				t.Fatalf("offline source open status = %d", w.Code)
			}
		})
	}
	a := newAPITestApp(t)
	parent := t.TempDir()
	root := filepath.Join(parent, "source")
	if err := os.Mkdir(root, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(parent, "outside.txt"), []byte("outside source"), 0600); err != nil {
		t.Fatal(err)
	}
	_, file := testAPIFile(t, a, root, "..", "outside.txt", "")
	for _, folder := range []string{"false", "true"} {
		body := `{"id":` + strconv.FormatInt(file.ID, 10) + `,"folder":` + folder + `}`
		w := apiTestRequest(a, http.MethodPost, "/api/open", body, map[string]string{"X-AllFiles-Token": a.token})
		if w.Code != http.StatusBadRequest {
			t.Fatalf("path traversal open status = %d, response %s", w.Code, w.Body.String())
		}
	}
}

func TestAPIBrowseListsOnlyRequestedDirectory(t *testing.T) {
	a := newAPITestApp(t)
	root := t.TempDir()
	if err := os.Mkdir(filepath.Join(root, "Documents"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "example.txt"), []byte("contents must not be returned by browse"), 0600); err != nil {
		t.Fatal(err)
	}
	w := apiTestRequest(a, http.MethodGet, "/api/browse?path="+url.QueryEscape(root), "", nil)
	data, _ := io.ReadAll(w.Result().Body)
	if w.Code != http.StatusOK || !strings.Contains(string(data), "Documents") || !strings.Contains(string(data), "example.txt") || strings.Contains(string(data), "contents must not") {
		t.Fatalf("browse failed: %d %s", w.Code, data)
	}
	w = apiTestRequest(a, http.MethodGet, "/api/browse?path=relative/path", "", nil)
	if w.Code != http.StatusBadRequest {
		t.Fatalf("relative browse status = %d", w.Code)
	}
}

func TestValidateOpenPathResolvesOnlyAvailableSourceFiles(t *testing.T) {
	root := t.TempDir()
	directory := filepath.Join(root, "Documents")
	if err := os.Mkdir(directory, 0700); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(directory, "notes.txt")
	if err := os.WriteFile(file, []byte("test"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, folder := range []bool{false, true} {
		got, err := validateOpenPath(FileResult{root: root, Path: file}, folder)
		want := file
		if folder {
			want = directory
		}
		want, _ = filepath.EvalSymlinks(want)
		if err != nil || got != want {
			t.Fatalf("folder=%v: got %q, %v; want %q", folder, got, err, want)
		}
	}
	for _, test := range []FileResult{
		{root: "import://offline", Path: file},
		{root: "demo://sample", Path: file},
		{root: "relative", Path: file},
		{root: root, Path: "relative/notes.txt"},
		{root: root, Path: filepath.Join(root, "missing.txt")},
		{root: filepath.Join(root, "missing"), Path: file},
		{root: root, Path: filepath.Join(root, "..", "outside.txt")},
	} {
		if got, err := validateOpenPath(test, false); err == nil || got != "" {
			t.Fatalf("invalid path accepted: %#v => %q, %v", test, got, err)
		}
	}
}

func TestValidateOpenPathRejectsSymlinkEscape(t *testing.T) {
	root := t.TempDir()
	outside := t.TempDir()
	if err := os.WriteFile(filepath.Join(outside, "private.txt"), []byte("outside"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "changed-directory")
	if err := os.Symlink(outside, link); err != nil {
		if runtime.GOOS == "windows" {
			t.Skipf("Windows symlink privilege unavailable: %v", err)
		}
		t.Fatal(err)
	}
	for _, folder := range []bool{false, true} {
		got, err := validateOpenPath(FileResult{root: root, Path: filepath.Join(link, "private.txt")}, folder)
		if err == nil || got != "" {
			t.Fatalf("symlink escape allowed (folder=%v): %q %v", folder, got, err)
		}
	}
	// A source itself may be reached through a mounted-directory symlink; its
	// resolved root remains the containment boundary.
	inside := filepath.Join(root, "inside.txt")
	if err := os.WriteFile(inside, []byte("inside"), 0600); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(t.TempDir(), "source-alias")
	if err := os.Symlink(root, alias); err != nil {
		t.Fatal(err)
	}
	got, err := validateOpenPath(FileResult{root: alias, Path: filepath.Join(alias, "inside.txt")}, false)
	want, _ := filepath.EvalSymlinks(inside)
	if err != nil || got != want {
		t.Fatalf("legitimate source alias rejected: %q %v", got, err)
	}
}
