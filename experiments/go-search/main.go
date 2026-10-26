// Command go-search measures only SQLite count and ID-page queries against a
// synthetic fixture. It is separate from the AllFiles application binary.
package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"slices"
	"strings"
	"time"
)

type queryCase struct {
	Name          string   `json:"name"`
	CountSQL      string   `json:"count_sql"`
	PageSQL       string   `json:"page_sql"`
	Args          []string `json:"args"`
	ExpectedCount *int64   `json:"expected_count"`
	ExpectedIDs   []int64  `json:"expected_ids"`
}

type caseManifest struct {
	Format int         `json:"format"`
	Cases  []queryCase `json:"cases"`
}

type caseResult struct {
	Name     string  `json:"name"`
	MedianMS float64 `json:"median_ms"`
	P95MS    float64 `json:"p95_ms"`
	MinMS    float64 `json:"min_ms"`
	Count    int64   `json:"count"`
	IDs      []int64 `json:"ids"`
}

type benchmarkResult struct {
	Implementation string       `json:"implementation"`
	SQLiteVersion  string       `json:"sqlite_version"`
	SQLiteSourceID string       `json:"sqlite_source_id"`
	CompileOptions []string     `json:"sqlite_compile_options"`
	Iterations     int          `json:"iterations"`
	Warmup         int          `json:"warmup"`
	Results        []caseResult `json:"results"`
}

func main() {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	if err := run(ctx, os.Args[1:], os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "go-search:", err)
		os.Exit(1)
	}
}

func run(ctx context.Context, args []string, output, diagnostics io.Writer) error {
	flags := flag.NewFlagSet("go-search", flag.ContinueOnError)
	flags.SetOutput(diagnostics)
	data := flags.String("data", "", "Existing unencrypted synthetic fixture directory")
	cases := flags.String("cases", "", "Shared format-1 JSON query manifest")
	iterations := flags.Int("iterations", 20, "Measured samples per query case")
	warmup := flags.Int("warmup", 3, "Unmeasured verified samples per query case")
	if err := flags.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	if flags.NArg() != 0 || *data == "" || *cases == "" {
		return errors.New("--data and --cases are required; positional arguments are not accepted")
	}
	if *iterations < 1 || *iterations > 10000 || *warmup < 0 || *warmup > 1000 {
		return errors.New("iterations must be 1..10000 and warmup must be 0..1000")
	}
	manifest, err := readCases(*cases)
	if err != nil {
		return err
	}
	db, conn, err := openFixture(ctx, *data)
	if err != nil {
		return err
	}
	defer db.Close()
	defer conn.Close()
	result := benchmarkResult{Implementation: implementation, Iterations: *iterations, Warmup: *warmup, Results: make([]caseResult, 0, len(manifest.Cases))}
	if err := conn.QueryRowContext(ctx, `SELECT sqlite_version()`).Scan(&result.SQLiteVersion); err != nil {
		return err
	}
	if result.SQLiteVersion != "3.53.4" {
		return fmt.Errorf("comparison requires SQLite 3.53.4, got %s", result.SQLiteVersion)
	}
	if err := conn.QueryRowContext(ctx, `SELECT sqlite_source_id()`).Scan(&result.SQLiteSourceID); err != nil {
		return err
	}
	options, err := conn.QueryContext(ctx, `PRAGMA compile_options`)
	if err != nil {
		return err
	}
	for options.Next() {
		var option string
		if err = options.Scan(&option); err != nil {
			break
		}
		result.CompileOptions = append(result.CompileOptions, option)
	}
	if err = errors.Join(err, options.Err(), options.Close()); err != nil {
		return err
	}
	slices.Sort(result.CompileOptions)
	for _, test := range manifest.Cases {
		args := make([]any, len(test.Args))
		for i := range test.Args {
			args[i] = test.Args[i]
		}
		samples := make([]float64, 0, *iterations)
		var count int64
		var ids []int64
		for index := 0; index < *warmup+*iterations; index++ {
			if err := ctx.Err(); err != nil {
				return err
			}
			started := time.Now()
			count, ids, err = query(ctx, conn, test, args)
			elapsed := float64(time.Since(started).Nanoseconds()) / 1e6
			if err != nil {
				return fmt.Errorf("case %q sample %d: %w", test.Name, index+1, err)
			}
			// Verification is deliberately outside the timed region and also
			// covers every warmup sample, so wrong results never look faster.
			if count != *test.ExpectedCount || !slices.Equal(ids, test.ExpectedIDs) {
				return fmt.Errorf("case %q sample %d: results differ from the shared expected count/IDs", test.Name, index+1)
			}
			if index >= *warmup {
				samples = append(samples, elapsed)
			}
		}
		median, p95, minimum := summarize(samples)
		result.Results = append(result.Results, caseResult{Name: test.Name, MedianMS: median, P95MS: p95, MinMS: minimum, Count: count, IDs: ids})
	}
	encoder := json.NewEncoder(output)
	encoder.SetIndent("", "  ")
	return encoder.Encode(result)
}

func readCases(path string) (caseManifest, error) {
	f, err := os.Open(path)
	if err != nil {
		return caseManifest{}, err
	}
	defer f.Close()
	b, err := io.ReadAll(io.LimitReader(f, (4<<20)+1))
	if err != nil || len(b) > 4<<20 {
		return caseManifest{}, errors.New("query manifest could not be read or exceeds 4 MiB")
	}
	var manifest caseManifest
	decoder := json.NewDecoder(strings.NewReader(string(b)))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return manifest, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return manifest, errors.New("query manifest has trailing data")
	}
	if manifest.Format != 1 || len(manifest.Cases) == 0 || len(manifest.Cases) > 128 {
		return manifest, errors.New("query manifest must use format 1 and contain 1..128 cases")
	}
	names := make(map[string]bool)
	for _, test := range manifest.Cases {
		if strings.TrimSpace(test.Name) == "" || names[test.Name] || test.ExpectedCount == nil || *test.ExpectedCount < 0 {
			return manifest, errors.New("each query needs a unique name and a nonnegative expected_count")
		}
		names[test.Name] = true
		if len(test.Args) > 128 || len(test.ExpectedIDs) > 10000 {
			return manifest, errors.New("query arguments or expected page exceed the experiment limits")
		}
		for _, statement := range []string{test.CountSQL, test.PageSQL} {
			words := strings.Fields(statement)
			if len(statement) > 128<<10 || len(words) == 0 || (!strings.EqualFold(words[0], "SELECT") && !strings.EqualFold(words[0], "WITH")) {
				return manifest, errors.New("benchmark queries must be SELECT/WITH statements of at most 128 KiB")
			}
		}
	}
	return manifest, nil
}

func readOnlyURI(path string) string {
	u := url.URL{Path: filepath.ToSlash(path)}
	return "file:" + u.EscapedPath() + "?mode=ro"
}

func openFixture(ctx context.Context, data string) (*sql.DB, *sql.Conn, error) {
	abs, err := filepath.Abs(data)
	if err != nil {
		return nil, nil, err
	}
	if _, err := os.Stat(filepath.Join(abs, "encryption.json")); !errors.Is(err, os.ErrNotExist) {
		return nil, nil, errors.New("this experiment requires an unencrypted synthetic fixture")
	}
	for _, name := range []string{"catalog.sqlite", "content.sqlite"} {
		info, err := os.Stat(filepath.Join(abs, name))
		if err != nil || !info.Mode().IsRegular() {
			return nil, nil, fmt.Errorf("fixture requires existing regular %s", name)
		}
	}
	db, err := openDatabase(readOnlyURI(filepath.Join(abs, "catalog.sqlite")))
	if err != nil {
		return nil, nil, err
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	conn, err := db.Conn(ctx)
	if err != nil {
		db.Close()
		return nil, nil, err
	}
	if _, err = conn.ExecContext(ctx, `ATTACH DATABASE ? AS content`, readOnlyURI(filepath.Join(abs, "content.sqlite"))); err == nil {
		_, err = conn.ExecContext(ctx, `PRAGMA query_only=ON;`)
	}
	if err != nil {
		conn.Close()
		db.Close()
		return nil, nil, err
	}
	return db, conn, nil
}

func query(ctx context.Context, conn *sql.Conn, test queryCase, args []any) (int64, []int64, error) {
	// Prepare/execute/close on each sample matches the application's uncached
	// QueryRowContext/QueryContext behavior and the paired Rust comparator.
	countStatement, err := conn.PrepareContext(ctx, test.CountSQL)
	if err != nil {
		return 0, nil, err
	}
	var count int64
	err = countStatement.QueryRowContext(ctx, args...).Scan(&count)
	err = errors.Join(err, countStatement.Close())
	if err != nil {
		return 0, nil, err
	}
	pageStatement, err := conn.PrepareContext(ctx, test.PageSQL)
	if err != nil {
		return 0, nil, err
	}
	rows, err := pageStatement.QueryContext(ctx, args...)
	if err != nil {
		pageStatement.Close()
		return 0, nil, err
	}
	ids := make([]int64, 0, len(test.ExpectedIDs))
	for rows.Next() {
		var id int64
		if err = rows.Scan(&id); err != nil {
			break
		}
		ids = append(ids, id)
	}
	err = errors.Join(err, rows.Err(), rows.Close(), pageStatement.Close())
	return count, ids, err
}

func summarize(samples []float64) (median, p95, minimum float64) {
	sorted := slices.Clone(samples)
	slices.Sort(sorted)
	middle := len(sorted) / 2
	median = sorted[middle]
	if len(sorted)%2 == 0 {
		median = (sorted[middle-1] + sorted[middle]) / 2
	}
	return median, sorted[int(math.Ceil(float64(len(sorted))*0.95))-1], sorted[0]
}
