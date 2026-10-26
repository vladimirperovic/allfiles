package main

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestScanIncrementalExcludesCatalogAndPreservesCancelledEntries(t *testing.T) {
	root := t.TempDir()
	data := filepath.Join(root, "catalog-data")
	if err := os.Mkdir(data, 0700); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStore(data)
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	file := filepath.Join(root, "notes.txt")
	if err = os.WriteFile(file, []byte("original archive contents"), 0600); err != nil {
		t.Fatal(err)
	}
	opts := ScanOptions{Root: root, Label: "Test drive", Kind: "external", IndexContent: true}
	if err = Scan(context.Background(), store, opts, nil); err != nil {
		t.Fatal(err)
	}
	sources, err := store.Sources()
	if err != nil {
		t.Fatal(err)
	}
	if len(sources) != 1 || sources[0].FileCount != 1 {
		t.Fatalf("catalog indexed itself: %#v", sources)
	}
	result, err := store.Search(context.Background(), SearchOptions{Query: "original", Mode: "content"})
	if err != nil || result.Total != 1 {
		t.Fatalf("content missing: %#v %v", result, err)
	}
	var indexed int64
	if err = Scan(context.Background(), store, opts, func(p Progress) { indexed = p.Indexed }); err != nil {
		t.Fatal(err)
	}
	if indexed != 0 {
		t.Fatalf("unchanged content reindexed: %d", indexed)
	}
	if err = os.WriteFile(file, []byte("replacement archive contents"), 0600); err != nil {
		t.Fatal(err)
	}
	later := time.Now().Add(time.Second)
	if err = os.Chtimes(file, later, later); err != nil {
		t.Fatal(err)
	}
	if err = Scan(context.Background(), store, opts, nil); err != nil {
		t.Fatal(err)
	}
	result, err = store.Search(context.Background(), SearchOptions{Query: "original", Mode: "content"})
	if err != nil || result.Total != 0 {
		t.Fatalf("stale text still searchable: %#v %v", result, err)
	}
	result, err = store.Search(context.Background(), SearchOptions{Query: "replacement", Mode: "content"})
	if err != nil || result.Total != 1 {
		t.Fatalf("new text missing: %#v %v", result, err)
	}
	if err = os.Remove(file); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	err = Scan(ctx, store, opts, func(p Progress) {
		if p.Status == "running" {
			cancel()
		}
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("expected cancelled scan, got %v", err)
	}
	sources, err = store.Sources()
	if err != nil || sources[0].FileCount != 1 {
		t.Fatalf("cancelled scan removed prior entries: %#v %v", sources, err)
	}
	newFile := filepath.Join(root, "new.txt")
	if err = os.WriteFile(newFile, []byte("new entry"), 0600); err != nil {
		t.Fatal(err)
	}
	lateCtx, lateCancel := context.WithCancel(context.Background())
	err = Scan(lateCtx, store, opts, func(p Progress) {
		if p.Scanned > 0 {
			lateCancel()
		}
	})
	lateCancel()
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("late cancellation was ignored: %v", err)
	}
	sources, err = store.Sources()
	if err != nil || sources[0].FileCount != 2 {
		t.Fatalf("late cancellation reconciled unseen entries: %#v %v", sources, err)
	}
	if err = os.Remove(newFile); err != nil {
		t.Fatal(err)
	}
	if err = Scan(context.Background(), store, opts, nil); err != nil {
		t.Fatal(err)
	}
	sources, err = store.Sources()
	if err != nil || sources[0].FileCount != 0 {
		t.Fatalf("complete scan did not reconcile deletions: %#v %v", sources, err)
	}
}

func TestImportSeparatesDiscsAndInvalidExportDoesNotChangeCatalog(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	export := filepath.Join(t.TempDir(), "catalog.csv")
	contents := "Filename,File Path,Size,Disk\nplan.pdf,D:\\Projects\\plan.pdf,10,BD-01\nplan.pdf,D:\\Projects\\plan.pdf,20,BD-02\n"
	if err = os.WriteFile(export, []byte(contents), 0600); err != nil {
		t.Fatal(err)
	}
	if err = Import(context.Background(), store, ImportOptions{Path: export}, nil); err != nil {
		t.Fatal(err)
	}
	sources, err := store.Sources()
	if err != nil || len(sources) != 2 || sources[0].FileCount != 1 || sources[1].FileCount != 1 {
		t.Fatalf("media lost: %#v %v", sources, err)
	}
	result, err := store.Search(context.Background(), SearchOptions{Query: "plan", Mode: "files"})
	if err != nil || result.Total != 2 {
		t.Fatalf("imported files missing %#v %v", result, err)
	}
	for _, file := range result.Results {
		if file.TextStatus != "not_indexed" {
			t.Fatalf("metadata import claims extracted text: %s", file.TextStatus)
		}
	}
	if err = os.WriteFile(export, []byte("Filename,Size,Disk\nwrong.pdf,9,BD-01\nbroken.pdf,invalid,BD-01\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = Import(context.Background(), store, ImportOptions{Path: export}, nil); err == nil {
		t.Fatal("invalid import succeeded")
	}
	result, err = store.Search(context.Background(), SearchOptions{Query: "plan", Mode: "files"})
	if err != nil || result.Total != 2 {
		t.Fatalf("invalid import altered previous catalog %#v %v", result, err)
	}
	result, err = store.Search(context.Background(), SearchOptions{Query: "wrong", Mode: "files"})
	if err != nil || result.Total != 0 {
		t.Fatalf("validation wrote partial data %#v %v", result, err)
	}
}
