package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func testFixture(t *testing.T) (string, caseManifest) {
	t.Helper()
	dir := t.TempDir()
	for name, schema := range map[string]string{
		"catalog.sqlite": `CREATE TABLE files(id INTEGER PRIMARY KEY,name TEXT); INSERT INTO files VALUES(1,'alpha.txt'),(2,'beta.txt'),(3,'alphabet.txt'); CREATE VIRTUAL TABLE names USING fts5(name,tokenize='trigram'); INSERT INTO names(rowid,name) SELECT id,name FROM files;`,
		"content.sqlite": `CREATE VIRTUAL TABLE fts USING fts5(text); INSERT INTO fts(rowid,text) VALUES(7,'walnut cabinetry'),(8,'oak shelving');`,
	} {
		uri := strings.TrimSuffix(readOnlyURI(filepath.Join(dir, name)), "?mode=ro") + "?mode=rwc"
		db, err := openDatabase(uri)
		if err != nil {
			t.Fatal(err)
		}
		_, err = db.Exec(schema)
		closeErr := db.Close()
		if err != nil || closeErr != nil {
			t.Fatalf("create fixture: %v %v", err, closeErr)
		}
	}
	nameCount, contentCount := int64(2), int64(1)
	return dir, caseManifest{Format: 1, Cases: []queryCase{
		{Name: "name-trigram", CountSQL: `SELECT COUNT(*) FROM files WHERE id IN(SELECT rowid FROM names WHERE name GLOB ?)`, PageSQL: `SELECT id FROM files WHERE id IN(SELECT rowid FROM names WHERE name GLOB ?) ORDER BY id LIMIT 50`, Args: []string{"*alpha*"}, ExpectedCount: &nameCount, ExpectedIDs: []int64{1, 3}},
		{Name: "content-word", CountSQL: `SELECT COUNT(*) FROM content.fts WHERE fts MATCH ?`, PageSQL: `SELECT rowid FROM content.fts WHERE fts MATCH ? ORDER BY rowid LIMIT 50`, Args: []string{"walnut"}, ExpectedCount: &contentCount, ExpectedIDs: []int64{7}},
	}}
}

func writeManifest(t *testing.T, dir string, manifest caseManifest) string {
	t.Helper()
	data, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "cases.json")
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	return path
}

func TestBenchmarkVerifiesSharedResultsAndReadsAttachedFTS(t *testing.T) {
	dir, manifest := testFixture(t)
	cases := writeManifest(t, dir, manifest)
	var output bytes.Buffer
	if err := run(context.Background(), []string{"--data", dir, "--cases", cases, "--iterations", "2", "--warmup", "1"}, &output, io.Discard); err != nil {
		t.Fatal(err)
	}
	var report benchmarkResult
	if err := json.Unmarshal(output.Bytes(), &report); err != nil {
		t.Fatal(err)
	}
	if report.Implementation != implementation || report.SQLiteVersion != "3.53.4" || len(report.Results) != 2 || report.Iterations != 2 || report.Warmup != 1 {
		t.Fatalf("invalid report: %#v", report)
	}
	for _, result := range report.Results {
		if result.MedianMS < 0 || result.MinMS > result.MedianMS || result.MedianMS > result.P95MS {
			t.Fatalf("invalid timing summary: %#v", result)
		}
	}
	manifest.Cases[0].ExpectedIDs = []int64{1, 999}
	cases = writeManifest(t, dir, manifest)
	output.Reset()
	if err := run(context.Background(), []string{"--data", dir, "--cases", cases, "--iterations", "1", "--warmup", "0"}, &output, io.Discard); err == nil || !strings.Contains(err.Error(), "results differ") || output.Len() != 0 {
		t.Fatalf("incorrect result accepted or published: %v, %s", err, output.String())
	}
}

func TestFixtureConnectionCannotWriteEitherDatabase(t *testing.T) {
	dir, _ := testFixture(t)
	db, conn, err := openFixture(context.Background(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	defer conn.Close()
	for _, statement := range []string{`INSERT INTO files VALUES(99,'changed')`, `INSERT INTO content.fts(rowid,text) VALUES(99,'changed')`} {
		if _, err := conn.ExecContext(context.Background(), statement); err == nil {
			t.Fatalf("fixture connection accepted mutation: %s", statement)
		}
	}
}

func TestTimingSummaryMatchesNearestRankConvention(t *testing.T) {
	median, p95, minimum := summarize([]float64{4, 2, 1, 3})
	if median != 2.5 || p95 != 4 || minimum != 1 {
		t.Fatalf("unexpected summary: %g %g %g", median, p95, minimum)
	}
	samples := make([]float64, 20)
	for i := range samples {
		samples[i] = float64(i + 1)
	}
	median, p95, minimum = summarize(samples)
	if median != 10.5 || p95 != 19 || minimum != 1 {
		t.Fatalf("unexpected 20-sample summary: %g %g %g", median, p95, minimum)
	}
}
