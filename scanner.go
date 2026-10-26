package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

type ScanOptions struct {
	Root         string `json:"root"`
	Label        string `json:"label"`
	Kind         string `json:"kind"`
	Location     string `json:"location"`
	SourceID     string `json:"sourceId"`
	IndexContent bool   `json:"indexContent"`
}

type Progress struct {
	ID      string `json:"id"`
	Status  string `json:"status"`
	Phase   string `json:"phase"`
	Current string `json:"current"`
	Error   string `json:"error,omitempty"`
	Scanned int64  `json:"scanned"`
	Indexed int64  `json:"indexed"`
}

type ScannedFile struct {
	Relative, Name, Dir, Extension string
	Size, Modified, ModifiedNS     int64
}

type IndexedFile struct {
	ID                int64
	Relative, Version string
	NeedsText         bool
}

func newScanID() string {
	var random [12]byte
	if _, err := rand.Read(random[:]); err == nil {
		return hex.EncodeToString(random[:])
	}
	return fmt.Sprintf("scan-%d", time.Now().UnixNano())
}

func withinPath(root, candidate string) bool {
	rel, err := filepath.Rel(root, candidate)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

// Scan never deletes unseen entries after a failed or cancelled walk. Content
// failures are recorded per file and do not invalidate a complete metadata walk.
func Scan(ctx context.Context, store *Store, opts ScanOptions, report func(Progress)) (err error) {
	opts.Root, err = filepath.Abs(opts.Root)
	if err != nil {
		return err
	}
	opts.Root = filepath.Clean(opts.Root)
	rootInfo, err := os.Lstat(opts.Root)
	if err != nil {
		return fmt.Errorf("cannot access scan folder: %w", err)
	}
	if !rootInfo.IsDir() || rootInfo.Mode()&os.ModeSymlink != 0 {
		return errors.New("scan root must be a real directory, not a symbolic link")
	}
	dataDir, err := filepath.Abs(store.DataDir)
	if err != nil {
		return err
	}
	if withinPath(dataDir, opts.Root) {
		return errors.New("the catalog data directory cannot be scanned")
	}
	if strings.TrimSpace(opts.Label) == "" {
		opts.Label = filepath.Base(opts.Root)
	}
	if opts.Kind == "" {
		opts.Kind = "computer"
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	source, err := store.BeginScan(opts)
	if err != nil {
		return err
	}
	state := Progress{ID: newScanID(), Status: "running", Phase: "scanning"}
	emit := func() {
		if report != nil {
			report(state)
		}
	}
	emit()
	complete := false
	defer func() {
		finishErr := store.FinishScan(source.ID, state.ID, complete)
		if err == nil {
			err = finishErr
		}
		state.Phase = "finished"
		state.Status = "complete"
		if err != nil {
			state.Status = "error"
			state.Error = err.Error()
		}
		if errors.Is(err, context.Canceled) {
			state.Status = "cancelled"
		}
		emit()
	}()
	batch := make([]ScannedFile, 0, 256)
	flush := func() error {
		if len(batch) == 0 {
			return nil
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		indexed, err := store.UpsertFiles(source.ID, state.ID, batch)
		if err != nil {
			return err
		}
		state.Scanned += int64(len(batch))
		batch = batch[:0]
		if opts.IndexContent {
			state.Phase = "extracting"
			for _, item := range indexed {
				if err := ctx.Err(); err != nil {
					return err
				}
				if !item.NeedsText || !IsTextCandidate(filepath.Ext(item.Relative)) {
					continue
				}
				state.Current = item.Relative
				path := filepath.Join(opts.Root, filepath.FromSlash(item.Relative))
				if !withinPath(opts.Root, path) {
					return errors.New("invalid catalog path outside scan root")
				}
				var text, status, detail string
				current, statErr := os.Lstat(path)
				if statErr != nil {
					status, detail = "error", statErr.Error()
				} else if strconv.FormatInt(current.Size(), 10)+":"+strconv.FormatInt(current.ModTime().UnixNano(), 10) != item.Version {
					status, detail = "error", "File changed after metadata scanning; scan again to index its current contents."
				} else {
					text, status, detail = ExtractTextContext(ctx, path)
				}
				if err := ctx.Err(); err != nil {
					return err
				}
				if err := store.RecordContent(item.ID, item.Version, status, text, detail); err != nil {
					return err
				}
				if status == "indexed" || status == "truncated" {
					state.Indexed++
				}
				emit()
			}
		}
		state.Phase = "scanning"
		emit()
		return nil
	}
	var accessErrors []string
	err = filepath.WalkDir(opts.Root, func(path string, entry fs.DirEntry, walkErr error) error {
		if err := ctx.Err(); err != nil {
			return err
		}
		if walkErr != nil {
			if len(accessErrors) < 5 {
				accessErrors = append(accessErrors, walkErr.Error())
			}
			if entry != nil && entry.IsDir() {
				return filepath.SkipDir
			}
			return nil
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return nil
		}
		if entry.IsDir() {
			if withinPath(dataDir, path) {
				return filepath.SkipDir
			}
			return nil
		}
		info, statErr := entry.Info()
		if statErr != nil {
			if len(accessErrors) < 5 {
				accessErrors = append(accessErrors, statErr.Error())
			}
			return nil
		}
		if !info.Mode().IsRegular() {
			return nil
		}
		rel, relErr := filepath.Rel(opts.Root, path)
		if relErr != nil {
			return relErr
		}
		rel = filepath.ToSlash(rel)
		dir := filepath.ToSlash(filepath.Dir(rel))
		if dir == "." {
			dir = ""
		}
		batch = append(batch, ScannedFile{Relative: rel, Name: info.Name(), Dir: dir, Extension: strings.ToLower(strings.TrimPrefix(filepath.Ext(info.Name()), ".")), Size: info.Size(), Modified: info.ModTime().Unix(), ModifiedNS: info.ModTime().UnixNano()})
		state.Current = rel
		if len(batch) == cap(batch) {
			return flush()
		}
		return nil
	})
	if err != nil {
		return err
	}
	if err = flush(); err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	if len(accessErrors) > 0 {
		return fmt.Errorf("scan incomplete; previous entries retained: %s", strings.Join(accessErrors, "; "))
	}
	complete = true
	return nil
}
