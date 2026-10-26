package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// These are explicitly invoked benchmarks, not part of the normal test run.
// Example: go test -run '^$' -bench '^BenchmarkCatalogWorkloads$' -benchtime=5x
// The fixture has 100,000 metadata entries and 50 copies of one 1 MiB document.
func BenchmarkCatalogWorkloads(b *testing.B) {
	b.StopTimer()
	store, err := OpenStore(b.TempDir())
	if err != nil {
		b.Fatal(err)
	}
	defer store.Close()
	started := time.Now()
	populateReviewCatalog(b, store)
	b.Logf("100k metadata population: %s", time.Since(started).Round(time.Millisecond))
	source, err := store.BeginScan(ScanOptions{Root: "import://benchmark-content", Label: "Shared document", Kind: "external"})
	if err != nil {
		b.Fatal(err)
	}
	entries := make([]ScannedFile, 50)
	for i := range entries {
		entries[i] = databaseTestFile(fmt.Sprintf("Documents/document-%03d.txt", i), 1048576, int64(1700000000+i)*1e9)
	}
	indexed, err := store.UpsertFiles(source.ID, "content", entries)
	if err != nil {
		b.Fatal(err)
	}
	text := strings.Repeat("Archive project material timber garden plan. ", 23831) + " matchingneedle"
	for _, entry := range indexed {
		if err := store.RecordContent(entry.ID, entry.Version, "indexed", text, ""); err != nil {
			b.Fatal(err)
		}
	}
	if err := store.FinishScan(source.ID, "content", true); err != nil {
		b.Fatal(err)
	}
	b.Run("stats", func(b *testing.B) {
		b.ReportAllocs()
		for i := 0; i < b.N; i++ {
			if _, err := store.Stats(); err != nil {
				b.Fatal(err)
			}
		}
	})
	b.Run("sources", func(b *testing.B) {
		b.ReportAllocs()
		for i := 0; i < b.N; i++ {
			if _, err := store.Sources(); err != nil {
				b.Fatal(err)
			}
		}
	})
	for _, query := range []struct {
		name    string
		options SearchOptions
	}{
		{"rare_name", SearchOptions{Query: "unique-needle-78901", Mode: "files", Limit: 50}},
		{"blank_first", SearchOptions{Mode: "files", Limit: 50}},
		{"blank_offset_90000", SearchOptions{Mode: "files", Limit: 50, Offset: 90000}},
		{"broad_name", SearchOptions{Query: "drawing", Mode: "files", Limit: 50}},
		{"broad_offset_90000", SearchOptions{Query: "drawing", Mode: "files", Limit: 50, Offset: 90000}},
		{"short_term", SearchOptions{Query: "a", Mode: "files", Limit: 50}},
		{"content_shared_1MiB_tail", SearchOptions{Query: "matchingneedle", Mode: "content", Limit: 50}},
		{"content_shared_1MiB_early", SearchOptions{Query: "garden", Mode: "content", Limit: 50}},
	} {
		b.Run(query.name, func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				if _, err := store.Search(context.Background(), query.options); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// Use -benchtime=1x to compare metadata-write overhead, including SQLite
// durability and aggregate triggers, without repeating a large fixture.
func BenchmarkCatalogPopulation100K(b *testing.B) {
	for i := 0; i < b.N; i++ {
		b.StopTimer()
		store, err := OpenStore(b.TempDir())
		if err != nil {
			b.Fatal(err)
		}
		b.StartTimer()
		populateReviewCatalog(b, store)
		b.StopTimer()
		if err := store.Close(); err != nil {
			b.Fatal(err)
		}
	}
}

func populateReviewCatalog(b testing.TB, store *Store) {
	b.Helper()
	source, err := store.BeginScan(ScanOptions{Root: "import://workload-benchmark", Label: "Synthetic 100k", Kind: "external"})
	if err != nil {
		b.Fatal(err)
	}
	for offset := 0; offset < 100000; offset += 256 {
		entries := make([]ScannedFile, 0, 256)
		for i := offset; i < offset+256 && i < 100000; i++ {
			ext := "bin"
			if i%2 == 0 {
				ext = "pdf"
			}
			dir := fmt.Sprintf("Archive/Project-%04d/Revision-%02d", i/100, i%7)
			name := fmt.Sprintf("drawing-%06d.%s", i, ext)
			if i == 78901 {
				name = "unique-needle-78901.docx"
				dir = "Archive/Project-0789/Rare-path-needle"
			}
			entries = append(entries, databaseTestFile(dir+"/"+name, int64(1024+i*53), int64(1700000000+i)*1e9))
		}
		if _, err := store.UpsertFiles(source.ID, "initial", entries); err != nil {
			b.Fatal(err)
		}
	}
	if err := store.FinishScan(source.ID, "initial", true); err != nil {
		b.Fatal(err)
	}
}

// Real metadata import includes CSV validation and media grouping, unlike a
// direct UpsertFiles benchmark. No document files are opened or extracted.
func BenchmarkMetadataImport10K(b *testing.B) {
	for _, extension := range []string{"bin", "pdf"} {
		b.Run(extension, func(b *testing.B) {
			b.StopTimer()
			var csv strings.Builder
			csv.WriteString("Filename,File Path,Size,Disk\n")
			for i := 0; i < 10000; i++ {
				fmt.Fprintf(&csv, "document-%05d.%s,Archive/Project-%03d,12345,BD-01\n", i, extension, i/100)
			}
			file := filepath.Join(b.TempDir(), "export.csv")
			if err := os.WriteFile(file, []byte(csv.String()), 0600); err != nil {
				b.Fatal(err)
			}
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				store, err := OpenStore(b.TempDir())
				if err != nil {
					b.Fatal(err)
				}
				b.StartTimer()
				if err := Import(context.Background(), store, ImportOptions{Path: file}, nil); err != nil {
					b.Fatal(err)
				}
				b.StopTimer()
				if err := store.Close(); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
