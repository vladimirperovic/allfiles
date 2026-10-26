package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

type comparisonCase struct {
	Name          string   `json:"name"`
	CountSQL      string   `json:"count_sql"`
	PageSQL       string   `json:"page_sql"`
	Args          []string `json:"args"`
	ExpectedCount int64    `json:"expected_count"`
	ExpectedIDs   []int64  `json:"expected_ids"`
}

// Explicit synthetic fixture generation for the Go/native-SQLite comparison.
// It never reads a user catalog and refuses to reuse a nonempty output folder.
// ALLFILES_COMPARE_DIR=/new/folder go test -run '^TestBuildComparisonFixture$'
func TestBuildComparisonFixture(t *testing.T) {
	dir := os.Getenv("ALLFILES_COMPARE_DIR")
	if dir == "" {
		t.Skip("set ALLFILES_COMPARE_DIR to generate the synthetic comparison fixture")
	}
	abs, err := filepath.Abs(dir)
	if err != nil {
		t.Fatal(err)
	}
	if entries, err := os.ReadDir(abs); err == nil {
		if len(entries) > 0 {
			t.Fatal("comparison output directory must be empty")
		}
	} else if !os.IsNotExist(err) {
		t.Fatal(err)
	} else if err := os.MkdirAll(abs, 0700); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStore(abs)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	populateReviewCatalog(t, store)
	source, err := store.BeginScan(ScanOptions{Root: "import://comparison-content", Label: "Synthetic shared document", Kind: "external"})
	if err != nil {
		t.Fatal(err)
	}
	entries := make([]ScannedFile, 50)
	for i := range entries {
		entries[i] = databaseTestFile(fmt.Sprintf("Documents/document-%03d.txt", i), 1048576, int64(1700000000+i)*1e9)
	}
	indexed, err := store.UpsertFiles(source.ID, "content", entries)
	if err != nil {
		t.Fatal(err)
	}
	text := strings.Repeat("Archive project material timber garden plan. ", 23831) + " matchingneedle"
	for _, entry := range indexed {
		if err := store.RecordContent(entry.ID, entry.Version, "indexed", text, ""); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.FinishScan(source.ID, "content", true); err != nil {
		t.Fatal(err)
	}
	manifest := struct {
		Format int              `json:"format"`
		Cases  []comparisonCase `json:"cases"`
	}{Format: 1, Cases: []comparisonCase{}}
	for _, spec := range []struct {
		name, query, mode string
		offset            int
	}{
		{"rare_name", "unique-needle-78901", "files", 0},
		{"rare_path", "Rare-path-needle", "files", 0},
		{"blank_first", "", "files", 0},
		{"blank_offset90k", "", "files", 90000},
		{"broad_name", "drawing", "files", 0},
		{"broad_offset90k", "drawing", "files", 90000},
		{"short_name", "a", "files", 0},
		{"content_rare", "matchingneedle", "content", 0},
		{"content_common", "garden", "content", 0},
	} {
		entry := comparisonCase{Name: spec.name, Args: []string{}, ExpectedIDs: []int64{}}
		where := "1=1 AND (1=1)"
		if spec.query != "" && spec.mode == "files" {
			where = `1=1 AND (f.id IN(SELECT rowid FROM names WHERE search_name GLOB ? UNION SELECT pf.id FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?)))`
			g := "*" + globLiteral(fold(spec.query)) + "*"
			entry.Args = []string{g, g}
		} else if spec.mode == "content" {
			where = `1=1 AND f.id IN(SELECT l.file_id FROM content.links l JOIN files cf ON cf.id=l.file_id AND cf.version=l.version WHERE l.doc_id IN(SELECT rowid FROM content.fts WHERE fts MATCH ?))`
			entry.Args = []string{textQuery(spec.query)}
		}
		entry.CountSQL = `SELECT COUNT(*) FROM files f WHERE ` + where
		if spec.query == "" && spec.mode == "files" {
			entry.CountSQL = `SELECT files FROM catalog_stats WHERE id=1`
		}
		entry.PageSQL = `SELECT f.id` + resultFrom + ` WHERE ` + where + fmt.Sprintf(` ORDER BY f.search_name,f.id LIMIT 50 OFFSET %d`, spec.offset)
		actual, err := store.Search(context.Background(), SearchOptions{Query: spec.query, Mode: spec.mode, Offset: spec.offset, Limit: 50})
		if err != nil {
			t.Fatal(err)
		}
		entry.ExpectedCount = actual.Total
		for _, file := range actual.Results {
			entry.ExpectedIDs = append(entry.ExpectedIDs, file.ID)
		}
		args := make([]any, len(entry.Args))
		for i, arg := range entry.Args {
			args[i] = arg
		}
		var count int64
		if err := store.conn.QueryRowContext(context.Background(), entry.CountSQL, args...).Scan(&count); err != nil {
			t.Fatal(err)
		}
		rows, err := store.conn.QueryContext(context.Background(), entry.PageSQL, args...)
		if err != nil {
			t.Fatal(err)
		}
		ids := []int64{}
		for rows.Next() {
			var id int64
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				t.Fatal(err)
			}
			ids = append(ids, id)
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			t.Fatal(err)
		}
		rows.Close()
		if count != entry.ExpectedCount || !reflect.DeepEqual(ids, entry.ExpectedIDs) {
			t.Fatalf("%s SQL disagrees with Store.Search: count=%d/%d ids=%v/%v", spec.name, count, entry.ExpectedCount, ids, entry.ExpectedIDs)
		}
		manifest.Cases = append(manifest.Cases, entry)
	}
	if err := store.Close(); err != nil {
		t.Fatal(err)
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(abs, "cases.json"), append(data, '\n'), 0600); err != nil {
		t.Fatal(err)
	}
	t.Logf("Synthetic comparison fixture: %s; %d verified cases; 100050 file locations", abs, len(manifest.Cases))
}
