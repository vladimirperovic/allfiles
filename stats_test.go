package main

import (
	"context"
	"path/filepath"
	"testing"
)

func checkCatalogStats(t *testing.T, s *Store, files, bytes, documents, sources int64) {
	t.Helper()
	stats, err := s.Stats()
	if err != nil {
		t.Fatal(err)
	}
	for key, want := range map[string]int64{"files": files, "bytes": bytes, "documents": documents, "sources": sources} {
		if stats[key] != want {
			t.Errorf("%s: got %v, want %d", key, stats[key], want)
		}
	}
}

func TestStatsStayAccurateAcrossScansAndExport(t *testing.T) {
	s := newDatabaseTestStore(t)
	checkCatalogStats(t, s, 0, 0, 0, 0)
	source := databaseTestSource(t, s, "stats")
	files := databaseTestUpsert(t, s, source, "first", databaseTestFile("a.txt", 10, 1e9), databaseTestFile("b.txt", 20, 1e9))
	checkCatalogStats(t, s, 2, 30, 0, 1)
	for i, f := range files {
		status := []string{"indexed", "truncated"}[i]
		if err := s.RecordContent(f.ID, f.Version, status, "content", ""); err != nil {
			t.Fatal(err)
		}
	}
	checkCatalogStats(t, s, 2, 30, 2, 1)
	// Unchanged rescans and source upserts must not double-count.
	databaseTestSource(t, s, "stats")
	databaseTestUpsert(t, s, source, "second", databaseTestFile("a.txt", 10, 1e9))
	if err := s.FinishScan(source.ID, "second", false); err != nil {
		t.Fatal(err)
	}
	checkCatalogStats(t, s, 2, 30, 2, 1)
	if err := s.FinishScan(source.ID, "second", true); err != nil {
		t.Fatal(err)
	}
	checkCatalogStats(t, s, 1, 10, 1, 1)
	copyDir := filepath.Join(t.TempDir(), "catalog-only")
	if _, err := s.Export(copyDir, false); err != nil {
		t.Fatal(err)
	}
	copy, err := OpenStore(copyDir)
	if err != nil {
		t.Fatal(err)
	}
	defer copy.Close()
	checkCatalogStats(t, copy, 1, 10, 0, 1)
	databaseTestUpsert(t, s, source, "third", databaseTestFile("a.txt", 40, 2e9))
	checkCatalogStats(t, s, 1, 40, 0, 1)
	// Counters must roll back together with data mutations.
	tx, err := s.conn.BeginTx(context.Background(), nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := tx.Exec(`UPDATE files SET size=999,text_status='indexed'`); err != nil {
		t.Fatal(err)
	}
	if err := tx.Rollback(); err != nil {
		t.Fatal(err)
	}
	checkCatalogStats(t, s, 1, 40, 0, 1)
}

func TestStatsUpgradeExistingCatalogAndReopen(t *testing.T) {
	dir := t.TempDir()
	s, err := OpenStore(dir)
	if err != nil {
		t.Fatal(err)
	}
	source := databaseTestSource(t, s, "legacy")
	files := databaseTestUpsert(t, s, source, "first", databaseTestFile("legacy.txt", 100, 1e9))
	if err := s.RecordContent(files[0].ID, files[0].Version, "indexed", "old catalog", ""); err != nil {
		t.Fatal(err)
	}
	// Model a v0.1.0 catalog which predates the summary table.
	_, err = s.conn.ExecContext(context.Background(), `DROP TRIGGER stats_files_ai; DROP TRIGGER stats_files_ad; DROP TRIGGER stats_files_au; DROP TRIGGER stats_sources_ai; DROP TRIGGER stats_sources_ad; DROP TABLE catalog_stats`)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		s, err = OpenStore(dir)
		if err != nil {
			t.Fatal(err)
		}
		checkCatalogStats(t, s, 1, 100, 1, 1)
		if err := s.Close(); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := s.Stats(); err == nil {
		t.Fatal("closed catalog must return an error")
	}
}

func TestSearchCountsMatchFilteredAndCombinedPages(t *testing.T) {
	s := newDatabaseTestStore(t)
	first := databaseTestSource(t, s, "first")
	second := databaseTestSource(t, s, "second")
	items := databaseTestUpsert(t, s, first, "scan", databaseTestFile("match/a.txt", 10, 1e9), databaseTestFile("match/b.pdf", 20, 1e9))
	databaseTestUpsert(t, s, second, "scan", databaseTestFile("c.bin", 30, 1e9))
	if err := s.RecordContent(items[1].ID, items[1].Version, "indexed", "match content", ""); err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		options SearchOptions
		want    int64
	}{
		{SearchOptions{Mode: "files"}, 3},
		{SearchOptions{Mode: "all"}, 3},
		{SearchOptions{Mode: "all", Query: `" "`}, 3},
		{SearchOptions{Source: first.ID}, 2},
		{SearchOptions{Extension: "pdf"}, 1},
		{SearchOptions{Mode: "all", Query: "match"}, 2},
		{SearchOptions{Mode: "content", Query: "match"}, 1},
		{SearchOptions{Mode: "all", Query: "match", Source: second.ID}, 0},
	} {
		result := databaseSearch(t, s, test.options, test.want)
		if int64(len(result.Results)) != test.want {
			t.Fatalf("count/page mismatch for %#v: %#v", test.options, result)
		}
	}
}
