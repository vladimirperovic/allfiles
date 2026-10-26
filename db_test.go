package main

import (
	"context"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func newDatabaseTestStore(t *testing.T) *Store {
	t.Helper()
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := store.Close(); err != nil {
			t.Error(err)
		}
	})
	return store
}

func databaseTestSource(t *testing.T, store *Store, label string) Source {
	t.Helper()
	source, err := store.BeginScan(ScanOptions{Root: "import://test/" + label, Label: label, Kind: "bluray", Location: "Archive"})
	if err != nil {
		t.Fatal(err)
	}
	return source
}

func databaseTestFile(relative string, size, modified int64) ScannedFile {
	dir := path.Dir(relative)
	if dir == "." {
		dir = ""
	}
	return ScannedFile{Relative: relative, Name: path.Base(relative), Dir: dir, Extension: strings.TrimPrefix(path.Ext(relative), "."), Size: size, Modified: modified / 1e9, ModifiedNS: modified}
}

func databaseTestUpsert(t *testing.T, store *Store, source Source, scan string, entries ...ScannedFile) []IndexedFile {
	t.Helper()
	indexed, err := store.UpsertFiles(source.ID, scan, entries)
	if err != nil {
		t.Fatal(err)
	}
	return indexed
}

func databaseSearch(t *testing.T, store *Store, options SearchOptions, want int64) SearchResult {
	t.Helper()
	result, err := store.Search(context.Background(), options)
	if err != nil {
		t.Fatalf("search %#v: %v", options, err)
	}
	if result.Total != want {
		t.Fatalf("search %#v: got %d, want %d; results %#v", options, result.Total, want, result.Results)
	}
	return result
}

func TestDatabaseNamesPathsUnicodeAndLiteralGlob(t *testing.T) {
	store := newDatabaseTestStore(t)
	source := databaseTestSource(t, store, "BD-01")
	databaseTestUpsert(t, store, source, "initial",
		databaseTestFile("Arhiva/Đorđe/2026/Čaše [final]*?.pdf", 10, 1000000000),
		databaseTestFile("Projects/Kuhinja/case final.pdf", 20, 2000000000),
		databaseTestFile("Радови/План.pdf", 30, 3000000000),
		databaseTestFile("Images/izložba.bin", 40, 4000000000),
	)
	for _, test := range []struct {
		query string
		total int64
	}{
		{"case", 2}, {"ČAŠE", 2}, {"ĐORĐE", 1}, {"djor", 1}, {"case djordje", 1},
		{"[final]*?", 1}, {"*", 1}, {"?", 1}, {"[", 1}, {"план", 1}, {"радови", 1}, {"izlozba", 1}, {"no-such-file", 0},
	} {
		t.Run(test.query, func(t *testing.T) {
			databaseSearch(t, store, SearchOptions{Query: test.query, Mode: "files"}, test.total)
		})
	}
	filtered := databaseSearch(t, store, SearchOptions{Mode: "files", Extension: ".PDF", Sort: "size", Limit: 2}, 3)
	if len(filtered.Results) != 2 || filtered.Results[0].Size != 30 || filtered.Results[1].Size != 20 {
		t.Fatalf("sorting/pagination broken: %#v", filtered.Results)
	}
	secondPage := databaseSearch(t, store, SearchOptions{Mode: "files", Extension: "pdf", Sort: "size", Limit: 2, Offset: 2}, 3)
	if len(secondPage.Results) != 1 || secondPage.Results[0].Size != 10 {
		t.Fatalf("second page broken: %#v", secondPage.Results)
	}
}

func TestDatabaseContentPhrasesFiltersAndDeduplication(t *testing.T) {
	store := newDatabaseTestStore(t)
	first := databaseTestSource(t, store, "BD-01")
	second := databaseTestSource(t, store, "BD-02")
	firstFiles := databaseTestUpsert(t, store, first, "initial", databaseTestFile("Projects/plan.pdf", 10, 1000000000), databaseTestFile("Projects/separate.pdf", 20, 2000000000))
	secondFiles := databaseTestUpsert(t, store, second, "initial", databaseTestFile("Backup/plan.docx", 10, 1000000000))
	shared := "The red blue kitchen has čaše and project drawings."
	for _, item := range []IndexedFile{firstFiles[0], secondFiles[0]} {
		if err := store.RecordContent(item.ID, item.Version, "indexed", shared, ""); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.RecordContent(firstFiles[1].ID, firstFiles[1].Version, "indexed", "The red green blue room has separate words.", ""); err != nil {
		t.Fatal(err)
	}
	var documents, links int
	if err := store.conn.QueryRowContext(context.Background(), `SELECT COUNT(*) FROM content.documents`).Scan(&documents); err != nil {
		t.Fatal(err)
	}
	if err := store.conn.QueryRowContext(context.Background(), `SELECT COUNT(*) FROM content.links`).Scan(&links); err != nil {
		t.Fatal(err)
	}
	if documents != 2 || links != 3 {
		t.Fatalf("dedup failed: documents=%d links=%d", documents, links)
	}
	databaseSearch(t, store, SearchOptions{Query: "red blue", Mode: "content"}, 3)
	phrase := databaseSearch(t, store, SearchOptions{Query: `"red blue"`, Mode: "content"}, 2)
	if phrase.Results[0].SourceID == phrase.Results[1].SourceID {
		t.Fatal("duplicate content lost one physical location")
	}
	for _, file := range phrase.Results {
		if !strings.Contains(file.Snippet, "red blue") {
			t.Fatalf("missing searchable text preview: %#v", file)
		}
	}
	databaseSearch(t, store, SearchOptions{Query: "case", Mode: "content"}, 2)
	databaseSearch(t, store, SearchOptions{Query: `"red blue"`, Mode: "content", Source: first.ID, Extension: "pdf"}, 1)
	databaseSearch(t, store, SearchOptions{Query: `"red blue"`, Mode: "content", Source: first.ID, Extension: "docx"}, 0)
	databaseSearch(t, store, SearchOptions{Query: "separate", Mode: "all", Source: first.ID}, 1)

	// A version change must hide stale content before the replacement is ready.
	changed := databaseTestUpsert(t, store, first, "next", databaseTestFile("Projects/plan.pdf", 35, 4000000000))[0]
	if changed.ID != firstFiles[0].ID || !changed.NeedsText {
		t.Fatalf("stable file identity/change detection failed: %#v", changed)
	}
	databaseSearch(t, store, SearchOptions{Query: `"red blue"`, Mode: "content", Source: first.ID}, 0)
	if err := store.RecordContent(changed.ID, changed.Version, "indexed", "Replacement text has a violet room.", ""); err != nil {
		t.Fatal(err)
	}
	databaseSearch(t, store, SearchOptions{Query: "violet", Mode: "content", Source: first.ID}, 1)
	databaseSearch(t, store, SearchOptions{Query: `"red blue"`, Mode: "content"}, 1)
	_, text, err := store.File(context.Background(), changed.ID)
	if err != nil || !strings.Contains(text, "violet") || strings.Contains(text, "red blue") {
		t.Fatalf("document preview is stale: %q %v", text, err)
	}
}

func TestDatabaseRejectsMismatchedContentPair(t *testing.T) {
	makeCatalog := func(label string) string {
		dir := t.TempDir()
		store, err := OpenStore(dir)
		if err != nil {
			t.Fatal(err)
		}
		source := databaseTestSource(t, store, label)
		item := databaseTestUpsert(t, store, source, "initial", databaseTestFile("report.txt", 10, 1000000000))[0]
		if err = store.RecordContent(item.ID, item.Version, "indexed", label+" private contents", ""); err != nil {
			store.Close()
			t.Fatal(err)
		}
		if err = store.Close(); err != nil {
			t.Fatal(err)
		}
		return dir
	}
	first, second := makeCatalog("first"), makeCatalog("second")
	other, err := os.ReadFile(filepath.Join(second, "content.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(first, "content.sqlite"), other, 0600); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStore(first)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	// Metadata remains useful; content queries must refuse the foreign database.
	databaseSearch(t, store, SearchOptions{Query: "report", Mode: "files"}, 1)
	if _, err = store.Search(context.Background(), SearchOptions{Query: "private", Mode: "content"}); err == nil {
		t.Fatal("mismatched content was silently accepted")
	}
}

func TestDatabaseScanReconciliationIsSourceScoped(t *testing.T) {
	store := newDatabaseTestStore(t)
	first, second := databaseTestSource(t, store, "first"), databaseTestSource(t, store, "second")
	files := databaseTestUpsert(t, store, first, "initial", databaseTestFile("keep.txt", 10, 1000000000), databaseTestFile("gone.txt", 10, 1000000000))
	other := databaseTestUpsert(t, store, second, "initial", databaseTestFile("other.txt", 10, 1000000000))[0]
	for _, item := range append(files, other) {
		if err := store.RecordContent(item.ID, item.Version, "indexed", "Shared archival content", ""); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.FinishScan(first.ID, "initial", true); err != nil {
		t.Fatal(err)
	}
	if err := store.FinishScan(second.ID, "initial", true); err != nil {
		t.Fatal(err)
	}
	databaseTestUpsert(t, store, first, "partial", databaseTestFile("keep.txt", 10, 1000000000))
	if err := store.FinishScan(first.ID, "partial", false); err != nil {
		t.Fatal(err)
	}
	databaseSearch(t, store, SearchOptions{Query: "archival", Mode: "content"}, 3)
	databaseTestUpsert(t, store, first, "complete", databaseTestFile("keep.txt", 10, 1000000000))
	if err := store.FinishScan(first.ID, "complete", true); err != nil {
		t.Fatal(err)
	}
	databaseSearch(t, store, SearchOptions{Mode: "files", Source: first.ID}, 1)
	databaseSearch(t, store, SearchOptions{Mode: "files", Source: second.ID}, 1)
	databaseSearch(t, store, SearchOptions{Query: "archival", Mode: "content"}, 2)
	var documents int
	if err := store.conn.QueryRowContext(context.Background(), `SELECT COUNT(*) FROM content.documents`).Scan(&documents); err != nil {
		t.Fatal(err)
	}
	if documents != 1 {
		t.Fatalf("cleanup removed shared text still used elsewhere: %d", documents)
	}
}

// BenchmarkCatalog100K measures real SQLite query latency after a 100,000-file
// synthetic import. Run explicitly with -run '^$' -bench BenchmarkCatalog100K.
func BenchmarkCatalog100K(b *testing.B) {
	benchmarkCatalog100K(b, false)
}

func BenchmarkEncryptedCatalog100K(b *testing.B) {
	benchmarkCatalog100K(b, true)
}

func benchmarkCatalog100K(b *testing.B, encrypted bool) {
	b.StopTimer()
	store, err := OpenStore(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	defer store.Close()
	source, err := store.BeginScan(ScanOptions{Root: "import://benchmark", Label: "Benchmark 100K", Kind: "external"})
	if err != nil {
		b.Fatal(err)
	}
	started := time.Now()
	const count = 100000
	for offset := 0; offset < count; offset += 256 {
		entries := make([]ScannedFile, 0, 256)
		for index := offset; index < offset+256 && index < count; index++ {
			ext := "bin"
			if index%4 == 0 {
				ext = "pdf"
			}
			if index%4 == 1 {
				ext = "docx"
			}
			dir := fmt.Sprintf("Archive/Project-%04d/Revision-%02d", index/100, index%7)
			name := fmt.Sprintf("drawing-%06d.%s", index, ext)
			if index == 78901 {
				name = "unique-needle-78901.docx"
				dir = "Archive/Project-0789/Rare-path-needle"
			}
			entries = append(entries, databaseTestFile(dir+"/"+name, int64(1024+index*53), int64(1700000000+index)*1e9))
		}
		if _, err = store.UpsertFiles(source.ID, "initial", entries); err != nil {
			b.Fatal(err)
		}
	}
	if err = store.FinishScan(source.ID, "initial", true); err != nil {
		b.Fatal(err)
	}
	metadata, err := os.Stat(filepath.Join(store.DataDir, "catalog.sqlite"))
	if err != nil {
		b.Fatal(err)
	}
	b.Logf("100k import: %s; catalog.sqlite=%d bytes (%.2f MiB), %.1f bytes/file; content.sqlite absent (metadata only)", time.Since(started).Round(time.Millisecond), metadata.Size(), float64(metadata.Size())/(1<<20), float64(metadata.Size())/count)
	if encrypted {
		destination := filepath.Join(b.TempDir(), "protected")
		password := []byte("synthetic benchmark passphrase")
		defer clear(password)
		if _, err := store.ExportProtected(destination, false, password); err != nil {
			b.Fatal(err)
		}
		store, err = OpenStoreWithPassword(destination, password)
		if err != nil {
			b.Fatal(err)
		}
		defer store.Close()
		metadata, err = os.Stat(filepath.Join(destination, "catalog.sqlite"))
		if err != nil {
			b.Fatal(err)
		}
		b.Logf("Encrypted compacted copy: catalog.sqlite=%d bytes (%.2f MiB), plus encryption.json", metadata.Size(), float64(metadata.Size())/(1<<20))
	}
	queries := []struct{ name, query string }{{"rare_name", "unique-needle-78901"}, {"rare_path", "Rare-path-needle"}}
	for _, query := range queries {
		b.Run(query.name, func(b *testing.B) {
			b.ReportAllocs()
			for n := 0; n < b.N; n++ {
				result, err := store.Search(context.Background(), SearchOptions{Query: query.query, Mode: "files"})
				if err != nil || result.Total != 1 {
					b.Fatalf("query result=%d error=%v", result.Total, err)
				}
			}
		})
	}
}
