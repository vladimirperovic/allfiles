package main

import (
	"bytes"
	"compress/zlib"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/ncruces/go-sqlite3/vfs"
)

type Source struct {
	ID        string `json:"id"`
	Root      string `json:"root"`
	Label     string `json:"label"`
	Kind      string `json:"kind"`
	Location  string `json:"location"`
	LastScan  string `json:"last_scan"`
	Status    string `json:"status"`
	FileCount int64  `json:"file_count"`
	TotalSize int64  `json:"total_size"`
}
type FileResult struct {
	ID           int64  `json:"id"`
	Name         string `json:"name"`
	Path         string `json:"path"`
	RelativePath string `json:"relative_path"`
	SourceID     string `json:"source_id"`
	SourceLabel  string `json:"source_label"`
	Kind         string `json:"kind"`
	Location     string `json:"location"`
	Size         int64  `json:"size"`
	Modified     int64  `json:"modified"`
	Extension    string `json:"extension"`
	TextStatus   string `json:"text_status"`
	TextDetail   string `json:"text_detail"`
	Snippet      string `json:"snippet"`
	root         string
}
type Store struct {
	DataDir    string
	mu         sync.Mutex
	db         *sql.DB
	conn       *sql.Conn
	attached   bool
	catalogID  string
	identity   os.FileInfo
	encryption *encryptionState
	vfsName    string
}

func randomID() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		panic(err)
	}
	return hex.EncodeToString(b)
}
func fold(s string) string {
	return strings.NewReplacer("č", "c", "ć", "c", "š", "s", "ž", "z", "đ", "dj").Replace(strings.ToLower(s))
}
func OpenStore(dir string) (*Store, error) {
	return OpenStoreWithPassword(dir, nil)
}
func OpenStoreWithPassword(dir string, password []byte) (*Store, error) {
	abs, err := filepath.Abs(dir)
	if err != nil {
		return nil, err
	}
	info, err := os.Stat(abs)
	if err != nil || !info.IsDir() {
		return nil, fmt.Errorf("Direktorijum podataka mora već postojati: %s", abs)
	}
	if _, err := os.Stat(filepath.Join(abs, ".allfiles-export-incomplete")); err == nil {
		return nil, errors.New("Ovaj izvoz nije dovršen. Otvorite izvorni katalog i ponovite izvoz u novi folder.")
	} else if !errors.Is(err, os.ErrNotExist) {
		return nil, err
	}
	manifest, err := readEncryptionManifest(abs)
	if err != nil {
		return nil, err
	}
	var encryption *encryptionState
	if manifest != nil {
		if len(password) == 0 {
			return nil, ErrPasswordRequired
		}
		catalog, e := os.Stat(filepath.Join(abs, "catalog.sqlite"))
		if e != nil || catalog.Size() == 0 {
			return nil, errors.New("Enkriptovani katalog nedostaje ili je nepotpun. Vratite odgovarajuću rezervnu kopiju.")
		}
		encryption, err = deriveEncryption(*manifest, password)
		if err != nil {
			return nil, err
		}
	} else if len(password) != 0 {
		return nil, errors.New("Ovaj katalog nije enkriptovan. Napravite novu enkriptovanu kopiju pomoću izvoza.")
	}
	vfsName, err := registerEncryptionVFS(abs, encryption)
	if err != nil {
		encryption.clear()
		return nil, err
	}
	db, err := openSQLite(filepath.Join(abs, "catalog.sqlite"), vfsName)
	if err != nil {
		vfs.Unregister(vfsName)
		encryption.clear()
		return nil, err
	}
	db.SetMaxOpenConns(1)
	conn, err := db.Conn(context.Background())
	if err != nil {
		db.Close()
		vfs.Unregister(vfsName)
		encryption.clear()
		if encryption != nil {
			return nil, ErrInvalidPassword
		}
		return nil, err
	}
	s := &Store{DataDir: abs, db: db, conn: conn, identity: info, encryption: encryption, vfsName: vfsName}
	// Verify the schema can be read before any DDL, journal changes or writes.
	var tableCount int
	if err = conn.QueryRowContext(context.Background(), `SELECT count(*) FROM sqlite_schema`).Scan(&tableCount); err != nil {
		s.Close()
		if encryption != nil {
			return nil, ErrInvalidPassword
		}
		return nil, err
	}
	schema := `PRAGMA busy_timeout=5000; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=ON;
 CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS sources(id TEXT PRIMARY KEY,root TEXT NOT NULL,label TEXT NOT NULL,kind TEXT NOT NULL,location TEXT NOT NULL DEFAULT '',last_scan TEXT NOT NULL DEFAULT '',status TEXT NOT NULL DEFAULT 'new',UNIQUE(root,label));
 CREATE TABLE IF NOT EXISTS directories(id INTEGER PRIMARY KEY,source_id TEXT NOT NULL REFERENCES sources(id),path TEXT NOT NULL,search_path TEXT NOT NULL,UNIQUE(source_id,path));
 CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY AUTOINCREMENT,source_id TEXT NOT NULL REFERENCES sources(id),dir_id INTEGER NOT NULL REFERENCES directories(id),name TEXT NOT NULL,search_name TEXT NOT NULL,extension TEXT NOT NULL,size INTEGER NOT NULL,modified INTEGER NOT NULL,modified_ns INTEGER NOT NULL,version TEXT NOT NULL,seen_scan TEXT NOT NULL,text_status TEXT NOT NULL DEFAULT 'not_indexed',text_detail TEXT NOT NULL DEFAULT '',UNIQUE(dir_id,name));
 CREATE INDEX IF NOT EXISTS files_source ON files(source_id);
 CREATE INDEX IF NOT EXISTS files_name ON files(search_name);
 CREATE INDEX IF NOT EXISTS files_ext ON files(extension);
 CREATE INDEX IF NOT EXISTS files_modified ON files(modified DESC);
 CREATE INDEX IF NOT EXISTS files_size ON files(size DESC);
 CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(search_name,content='files',content_rowid='id',tokenize='trigram case_sensitive 1',detail=none,columnsize=0);
 CREATE VIRTUAL TABLE IF NOT EXISTS paths USING fts5(search_path,content='directories',content_rowid='id',tokenize='trigram case_sensitive 1',detail=none,columnsize=0);
 CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN INSERT INTO names(rowid,search_name) VALUES(new.id,new.search_name); END;
 CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN INSERT INTO names(names,rowid,search_name) VALUES('delete',old.id,old.search_name); END;
 CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE OF search_name ON files WHEN old.search_name != new.search_name BEGIN INSERT INTO names(names,rowid,search_name) VALUES('delete',old.id,old.search_name); INSERT INTO names(rowid,search_name) VALUES(new.id,new.search_name); END;
 CREATE TRIGGER IF NOT EXISTS dirs_ai AFTER INSERT ON directories BEGIN INSERT INTO paths(rowid,search_path) VALUES(new.id,new.search_path); END;
 CREATE TRIGGER IF NOT EXISTS dirs_ad AFTER DELETE ON directories BEGIN INSERT INTO paths(paths,rowid,search_path) VALUES('delete',old.id,old.search_path); END;
 INSERT OR IGNORE INTO meta(key,value) VALUES('schema','1');`
	if _, err = conn.ExecContext(context.Background(), schema); err != nil {
		s.Close()
		return nil, err
	}
	if _, err = conn.ExecContext(context.Background(), `INSERT OR IGNORE INTO meta(key,value) VALUES('catalog_id',?)`, randomID()); err != nil {
		s.Close()
		return nil, err
	}
	err = conn.QueryRowContext(context.Background(), `SELECT value FROM meta WHERE key='catalog_id'`).Scan(&s.catalogID)
	if err != nil {
		s.Close()
		return nil, err
	}
	if err = s.initStats(); err != nil {
		s.Close()
		return nil, err
	}
	conn.ExecContext(context.Background(), `UPDATE sources SET status='interrupted' WHERE status='scanning'`)
	os.Chmod(filepath.Join(abs, "catalog.sqlite"), 0600)
	return s, nil
}

func (s *Store) checkDataDir() error {
	if s.conn == nil {
		return errors.New("Katalog je zatvoren.")
	}
	now, e := os.Stat(s.DataDir)
	if e != nil || !os.SameFile(s.identity, now) {
		return errors.New("Direktorijum podataka više nije isti ili nije dostupan. Ponovo montirajte kontejner i pokrenite aplikaciju.")
	}
	return nil
}
func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	var err error
	if s.conn != nil {
		err = s.conn.Close()
		s.conn = nil
	}
	if s.db != nil {
		err = errors.Join(err, s.db.Close())
		s.db = nil
	}
	vfs.Unregister(s.vfsName)
	s.encryption.clear()
	return err
}
func (s *Store) Encrypted() bool { return s.encryption != nil }
func (s *Store) ensureContent(create bool) error {
	if e := s.checkDataDir(); e != nil {
		return e
	}
	if s.attached {
		return nil
	}
	p := filepath.Join(s.DataDir, "content.sqlite")
	if !create {
		if _, e := os.Stat(p); errors.Is(e, os.ErrNotExist) {
			return os.ErrNotExist
		}
	}
	_, err := s.conn.ExecContext(context.Background(), `ATTACH DATABASE ? AS content`, sqliteURI(p, s.vfsName))
	if err != nil {
		return err
	}
	fail := func(err error) error { s.conn.ExecContext(context.Background(), `DETACH DATABASE content`); return err }
	var tables int
	if err = s.conn.QueryRowContext(context.Background(), `SELECT COUNT(*) FROM content.sqlite_master WHERE type='table'`).Scan(&tables); err != nil {
		return fail(err)
	}
	if tables > 0 {
		var owner string
		if err = s.conn.QueryRowContext(context.Background(), `SELECT value FROM content.meta WHERE key='catalog_id'`).Scan(&owner); err != nil || owner != s.catalogID {
			return fail(errors.New("Baza sadržaja pripada drugom katalogu ili nije AllFiles baza."))
		}
	}
	_, err = s.conn.ExecContext(context.Background(), `PRAGMA content.journal_mode=DELETE; PRAGMA content.synchronous=FULL;
 CREATE TABLE IF NOT EXISTS content.meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
 CREATE TABLE IF NOT EXISTS content.documents(id INTEGER PRIMARY KEY AUTOINCREMENT,hash TEXT NOT NULL UNIQUE,payload BLOB NOT NULL,plain_size INTEGER NOT NULL);
 CREATE TABLE IF NOT EXISTS content.links(file_id INTEGER PRIMARY KEY,version TEXT NOT NULL,doc_id INTEGER NOT NULL REFERENCES documents(id));
 CREATE INDEX IF NOT EXISTS content.links_doc ON links(doc_id);
 CREATE VIRTUAL TABLE IF NOT EXISTS content.fts USING fts5(body,content='',contentless_delete=1,tokenize='unicode61 remove_diacritics 2');`)
	if err != nil {
		return fail(err)
	}
	_, err = s.conn.ExecContext(context.Background(), `INSERT OR IGNORE INTO content.meta(key,value) VALUES('catalog_id',?)`, s.catalogID)
	if err != nil {
		return fail(err)
	}
	var id string
	err = s.conn.QueryRowContext(context.Background(), `SELECT value FROM content.meta WHERE key='catalog_id'`).Scan(&id)
	if err != nil {
		return fail(err)
	}
	if id != s.catalogID {
		return fail(errors.New("Baza sadržaja pripada drugom katalogu. Vratite odgovarajući par baza."))
	}
	s.attached = true
	os.Chmod(p, 0600)
	return nil
}
func (s *Store) BeginScan(o ScanOptions) (Source, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.checkDataDir(); e != nil {
		return Source{}, e
	}
	ctx := context.Background()
	v := Source{ID: o.SourceID, Root: o.Root, Label: o.Label, Kind: o.Kind, Location: o.Location, Status: "scanning"}
	if v.Label == "" {
		v.Label = filepath.Base(o.Root)
	}
	if v.Kind == "" {
		v.Kind = "folder"
	}
	if v.ID != "" {
		var old string
		if e := s.conn.QueryRowContext(ctx, `SELECT id FROM sources WHERE id=?`, v.ID).Scan(&old); e != nil {
			return v, e
		}
	} else {
		e := s.conn.QueryRowContext(ctx, `SELECT id FROM sources WHERE root=? AND label=?`, v.Root, v.Label).Scan(&v.ID)
		if e != nil && !errors.Is(e, sql.ErrNoRows) {
			return v, e
		}
		if v.ID == "" {
			v.ID = randomID()
		}
	}
	_, err := s.conn.ExecContext(ctx, `INSERT INTO sources(id,root,label,kind,location,status) VALUES(?,?,?,?,?,'scanning') ON CONFLICT(id) DO UPDATE SET root=excluded.root,label=excluded.label,kind=excluded.kind,location=excluded.location,status='scanning'`, v.ID, v.Root, v.Label, v.Kind, v.Location)
	return v, err
}
func (s *Store) UpsertFiles(source, scan string, entries []ScannedFile) ([]IndexedFile, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.checkDataDir(); e != nil {
		return nil, e
	}
	contentErr := s.ensureContent(false)
	if contentErr != nil && !errors.Is(contentErr, os.ErrNotExist) {
		return nil, contentErr
	}
	hasContent := contentErr == nil
	ctx := context.Background()
	tx, err := s.conn.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	out := make([]IndexedFile, 0, len(entries))
	dirs := map[string]int64{}
	checkContent := []int{}
	for _, f := range entries {
		dir := strings.ReplaceAll(f.Dir, "\\", "/")
		if dir == "." {
			dir = ""
		}
		did, ok := dirs[dir]
		if !ok {
			_, err = tx.Exec(`INSERT OR IGNORE INTO directories(source_id,path,search_path) VALUES(?,?,?)`, source, dir, fold(dir))
			if err != nil {
				return nil, err
			}
			if err = tx.QueryRow(`SELECT id FROM directories WHERE source_id=? AND path=?`, source, dir).Scan(&did); err != nil {
				return nil, err
			}
			dirs[dir] = did
		}
		ver := strconv.FormatInt(f.Size, 10) + ":" + strconv.FormatInt(f.ModifiedNS, 10)
		var id int64
		var old, st string
		err = tx.QueryRow(`SELECT id,version,text_status FROM files WHERE dir_id=? AND name=?`, did, f.Name).Scan(&id, &old, &st)
		fresh := errors.Is(err, sql.ErrNoRows)
		if err != nil && !fresh {
			return nil, err
		}
		if fresh {
			r, e := tx.Exec(`INSERT INTO files(source_id,dir_id,name,search_name,extension,size,modified,modified_ns,version,seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?)`, source, did, f.Name, fold(f.Name), strings.TrimPrefix(strings.ToLower(f.Extension), "."), f.Size, f.Modified, f.ModifiedNS, ver, scan)
			if e != nil {
				return nil, e
			}
			id, _ = r.LastInsertId()
			st = "not_indexed"
		} else {
			_, err = tx.Exec(`UPDATE files SET size=?,modified=?,modified_ns=?,version=?,seen_scan=?,text_status=CASE WHEN version!=? THEN 'not_indexed' ELSE text_status END,text_detail=CASE WHEN version!=? THEN '' ELSE text_detail END WHERE id=?`, f.Size, f.Modified, f.ModifiedNS, ver, scan, ver, ver, id)
			if err != nil {
				return nil, err
			}
		}
		candidate := IsTextCandidate(f.Extension)
		if !candidate && (fresh || old != ver) {
			if _, err = tx.Exec(`UPDATE files SET text_status='unsupported',text_detail='' WHERE id=?`, id); err != nil {
				return nil, err
			}
		}
		expectsText := st == "indexed" || st == "truncated"
		needsText := candidate && (fresh || old != ver || st == "not_indexed" || st == "error" || (!hasContent && expectsText))
		if candidate && hasContent && expectsText && !needsText {
			checkContent = append(checkContent, len(out))
		}
		out = append(out, IndexedFile{ID: id, Relative: f.Relative, Version: ver, NeedsText: needsText})
	}
	// A content database may exist yet lack links for unchanged files, for
	// example while rebuilding a catalog-only copy across multiple scan batches.
	// Check current-version links in groups instead of issuing one query per file.
	validLinks := map[int64]bool{}
	for start := 0; start < len(checkContent); start += 256 {
		end := start + 256
		if end > len(checkContent) {
			end = len(checkContent)
		}
		args := make([]any, 0, end-start)
		for _, index := range checkContent[start:end] {
			args = append(args, out[index].ID)
		}
		placeholders := strings.TrimSuffix(strings.Repeat("?,", len(args)), ",")
		rows, err := tx.QueryContext(ctx, `SELECT l.file_id FROM content.links l JOIN main.files f ON f.id=l.file_id JOIN content.documents d ON d.id=l.doc_id WHERE l.version=f.version AND l.file_id IN (`+placeholders+`)`, args...)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var id int64
			if err := rows.Scan(&id); err != nil {
				rows.Close()
				return nil, err
			}
			validLinks[id] = true
		}
		if err := rows.Err(); err != nil {
			rows.Close()
			return nil, err
		}
		if err := rows.Close(); err != nil {
			return nil, err
		}
	}
	for _, index := range checkContent {
		out[index].NeedsText = !validLinks[out[index].ID]
	}
	return out, tx.Commit()
}
func (s *Store) RecordContent(id int64, ver, status, text, detail string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	ctx := context.Background()
	hasText := text != "" && (status == "indexed" || status == "truncated")
	if hasText || s.attached {
		if e := s.ensureContent(true); e != nil {
			return e
		}
	}
	tx, e := s.conn.BeginTx(ctx, nil)
	if e != nil {
		return e
	}
	defer tx.Rollback()
	var current string
	if e = tx.QueryRow(`SELECT version FROM files WHERE id=?`, id).Scan(&current); e != nil {
		return e
	}
	if ver != current {
		return errors.New("Dokument je izmenjen tokom indeksiranja")
	}
	if s.attached {
		if _, e = tx.Exec(`DELETE FROM content.links WHERE file_id=?`, id); e != nil {
			return e
		}
	}
	if hasText {
		sum := sha256.Sum256([]byte(text))
		hash := hex.EncodeToString(sum[:])
		var doc int64
		e = tx.QueryRow(`SELECT id FROM content.documents WHERE hash=?`, hash).Scan(&doc)
		if errors.Is(e, sql.ErrNoRows) {
			var b bytes.Buffer
			w := zlib.NewWriter(&b)
			w.Write([]byte(text))
			w.Close()
			r, err := tx.Exec(`INSERT INTO content.documents(hash,payload,plain_size) VALUES(?,?,?)`, hash, b.Bytes(), len(text))
			if err != nil {
				return err
			}
			doc, _ = r.LastInsertId()
			if _, err = tx.Exec(`INSERT INTO content.fts(rowid,body) VALUES(?,?)`, doc, text); err != nil {
				return err
			}
		} else if e != nil {
			return e
		}
		if _, e = tx.Exec(`INSERT INTO content.links(file_id,version,doc_id) VALUES(?,?,?)`, id, ver, doc); e != nil {
			return e
		}
	}
	if _, e = tx.Exec(`UPDATE files SET text_status=?,text_detail=? WHERE id=?`, status, detail, id); e != nil {
		return e
	}
	return tx.Commit()
}
func (s *Store) FinishScan(source, scan string, complete bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	ctx := context.Background()
	if _, e := os.Stat(filepath.Join(s.DataDir, "content.sqlite")); e == nil {
		if e = s.ensureContent(false); e != nil {
			return e
		}
	}
	tx, e := s.conn.BeginTx(ctx, nil)
	if e != nil {
		return e
	}
	defer tx.Rollback()
	if complete {
		if s.attached {
			if _, e = tx.Exec(`DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE source_id=? AND seen_scan!=?)`, source, scan); e != nil {
				return e
			}
		}
		if _, e = tx.Exec(`DELETE FROM files WHERE source_id=? AND seen_scan!=?`, source, scan); e != nil {
			return e
		}
		if _, e = tx.Exec(`DELETE FROM directories WHERE source_id=? AND id NOT IN(SELECT dir_id FROM files WHERE source_id=?)`, source, source); e != nil {
			return e
		}
	}
	if s.attached {
		if _, e = tx.Exec(`DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE text_status NOT IN ('indexed','truncated'))`); e != nil {
			return e
		}
		if _, e = tx.Exec(`DELETE FROM content.fts WHERE rowid IN(SELECT id FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links))`); e != nil {
			return e
		}
		if _, e = tx.Exec(`DELETE FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links)`); e != nil {
			return e
		}
	}
	status := "ready"
	if !complete {
		status = "partial"
	}
	_, e = tx.Exec(`UPDATE sources SET status=?,last_scan=? WHERE id=?`, status, time.Now().UTC().Format(time.RFC3339), source)
	if e != nil {
		return e
	}
	return tx.Commit()
}
func (s *Store) Sources() ([]Source, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	rows, e := s.conn.QueryContext(context.Background(), `SELECT s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status,COUNT(f.id),COALESCE(SUM(f.size),0) FROM sources s LEFT JOIN files f ON f.source_id=s.id GROUP BY s.id ORDER BY s.label`)
	if e != nil {
		return nil, e
	}
	defer rows.Close()
	out := []Source{}
	for rows.Next() {
		var v Source
		if e = rows.Scan(&v.ID, &v.Root, &v.Label, &v.Kind, &v.Location, &v.LastScan, &v.Status, &v.FileCount, &v.TotalSize); e != nil {
			return nil, e
		}
		out = append(out, v)
	}
	return out, rows.Err()
}

// SourcesPage bounds media metadata before it crosses the HTTP/MCP boundary.
// The unpaginated Sources method remains available to the existing web UI.
func (s *Store) SourcesPage(id string, offset, limit int) ([]Source, int64, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if offset < 0 || offset > 1000000 || limit < 1 || limit > 200 || len(id) > 256 {
		return nil, 0, errors.New("Neispravna stranica izvora")
	}
	where := ""
	args := []any{}
	if id != "" {
		where = " WHERE s.id=?"
		args = append(args, id)
	}
	var total int64
	if err := s.conn.QueryRowContext(context.Background(), `SELECT COUNT(*) FROM sources s`+where, args...).Scan(&total); err != nil {
		return nil, 0, err
	}
	args = append(args, limit, offset)
	// Select the page first, then aggregate only the files belonging to it.
	rows, err := s.conn.QueryContext(context.Background(), `SELECT s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status,COUNT(f.id),COALESCE(SUM(f.size),0) FROM (SELECT * FROM sources s`+where+` ORDER BY s.label,s.id LIMIT ? OFFSET ?) s LEFT JOIN files f ON f.source_id=s.id GROUP BY s.id ORDER BY s.label,s.id`, args...)
	if err != nil {
		return nil, 0, err
	}
	defer rows.Close()
	result := []Source{}
	for rows.Next() {
		var source Source
		if err := rows.Scan(&source.ID, &source.Root, &source.Label, &source.Kind, &source.Location, &source.LastScan, &source.Status, &source.FileCount, &source.TotalSize); err != nil {
			return nil, 0, err
		}
		result = append(result, source)
	}
	return result, total, rows.Err()
}

func (s *Store) Stats() (map[string]any, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.checkDataDir(); err != nil {
		return nil, err
	}
	var count, total, docs, src int64
	var demo string
	e := s.conn.QueryRowContext(context.Background(), `SELECT files,bytes,documents,sources FROM catalog_stats WHERE id=1`).Scan(&count, &total, &docs, &src)
	if e != nil {
		return nil, e
	}
	if e = s.conn.QueryRowContext(context.Background(), `SELECT value FROM meta WHERE key='demo'`).Scan(&demo); e != nil && !errors.Is(e, sql.ErrNoRows) {
		return nil, e
	}
	size := func(name string) int64 {
		v, e := os.Stat(filepath.Join(s.DataDir, name))
		if e != nil {
			return 0
		}
		return v.Size()
	}
	return map[string]any{"version": Version, "data_dir": s.DataDir, "demo": demo == "true", "files": count, "bytes": total, "documents": docs, "sources": src, "catalog_bytes": size("catalog.sqlite"), "content_bytes": size("content.sqlite"), "fts5": true, "encrypted": s.Encrypted()}, nil
}

type SearchOptions struct {
	Query, Mode, Source, Extension, Sort string
	Offset, Limit                        int
}
type SearchResult struct {
	Results   []FileResult `json:"results"`
	Total     int64        `json:"total"`
	ElapsedMS float64      `json:"elapsed_ms"`
	Offset    int          `json:"offset"`
	Limit     int          `json:"limit"`
}

const resultColumns = `f.id,f.name,d.path,s.id,s.label,s.kind,s.location,f.size,f.modified,f.extension,f.text_status,f.text_detail,s.root`
const resultFrom = ` FROM files f JOIN directories d ON d.id=f.dir_id JOIN sources s ON s.id=f.source_id `

func scanResult(row interface{ Scan(...any) error }) (FileResult, error) {
	var f FileResult
	var dir string
	e := row.Scan(&f.ID, &f.Name, &dir, &f.SourceID, &f.SourceLabel, &f.Kind, &f.Location, &f.Size, &f.Modified, &f.Extension, &f.TextStatus, &f.TextDetail, &f.root)
	f.RelativePath = strings.TrimSuffix(dir, "/")
	if f.RelativePath != "" {
		f.RelativePath += "/"
	}
	f.RelativePath += f.Name
	f.Path = f.RelativePath
	if !strings.HasPrefix(f.root, "import://") && !strings.HasPrefix(f.root, "demo://") {
		if (len(f.root) > 2 && f.root[1] == ':') || strings.HasPrefix(f.root, `\\`) {
			f.Path = strings.TrimRight(f.root, `\/`) + `\` + strings.ReplaceAll(f.RelativePath, "/", `\`)
		} else {
			f.Path = path.Join(f.root, f.RelativePath)
		}
	}
	return f, e
}
func globLiteral(v string) string {
	return strings.NewReplacer("[", "[[]", "*", "[*]", "?", "[?]").Replace(v)
}
func textQuery(q string) string {
	parts := []string{}
	var b strings.Builder
	quoted := false
	flush := func() {
		if b.Len() > 0 {
			parts = append(parts, `"`+strings.ReplaceAll(b.String(), `"`, `""`)+`"`)
			b.Reset()
		}
	}
	for _, r := range q {
		if r == '"' {
			flush()
			quoted = !quoted
		} else if (r == ' ' || r == '\t' || r == '\n') && !quoted {
			flush()
		} else {
			b.WriteRune(r)
		}
	}
	flush()
	return strings.Join(parts, " AND ")
}
func (s *Store) Search(ctx context.Context, o SearchOptions) (SearchResult, error) {
	start := time.Now()
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.checkDataDir(); err != nil {
		return SearchResult{}, err
	}
	if o.Limit <= 0 || o.Limit > 200 {
		o.Limit = 100
	}
	if o.Offset < 0 {
		o.Offset = 0
	}
	out := SearchResult{Results: []FileResult{}, Offset: o.Offset, Limit: o.Limit}
	where := []string{"1=1"}
	args := []any{}
	if o.Source != "" {
		where = append(where, "f.source_id=?")
		args = append(args, o.Source)
	}
	if o.Extension != "" {
		where = append(where, "f.extension=?")
		args = append(args, strings.TrimPrefix(strings.ToLower(o.Extension), "."))
	}
	q := strings.TrimSpace(o.Query)
	mode := o.Mode
	if mode == "" {
		mode = "files"
	}
	if mode != "files" && mode != "content" && mode != "all" {
		return out, errors.New("Nepoznat režim pretrage")
	}
	names := []string{}
	nameArgs := []any{}
	for _, term := range strings.Fields(strings.ReplaceAll(q, `"`, "")) {
		g := "*" + globLiteral(fold(term)) + "*"
		names = append(names, `f.id IN(SELECT rowid FROM names WHERE search_name GLOB ? UNION SELECT pf.id FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))`)
		nameArgs = append(nameArgs, g, g)
	}
	nameSQL := "1=1"
	if len(names) > 0 {
		nameSQL = strings.Join(names, " AND ")
	}
	contentSQL := "0"
	contentArgs := []any{}
	if mode != "files" {
		e := s.ensureContent(false)
		if e != nil && !errors.Is(e, os.ErrNotExist) {
			return out, e
		}
		if e == nil {
			// Keep the version check inside the candidate query. Correlating it
			// to the outer f row makes SQLite repeat it for every catalog file.
			contentSQL = `f.id IN(SELECT l.file_id FROM content.links l JOIN files cf ON cf.id=l.file_id AND cf.version=l.version)`
			if tq := textQuery(q); tq != "" {
				contentSQL = `f.id IN(SELECT l.file_id FROM content.links l JOIN files cf ON cf.id=l.file_id AND cf.version=l.version WHERE l.doc_id IN(SELECT rowid FROM content.fts WHERE fts MATCH ?))`
				contentArgs = append(contentArgs, tq)
			}
		}
	}
	switch mode {
	case "files":
		where = append(where, "("+nameSQL+")")
		args = append(args, nameArgs...)
	case "content":
		where = append(where, contentSQL)
		args = append(args, contentArgs...)
	case "all":
		where = append(where, "(("+nameSQL+") OR ("+contentSQL+"))")
		args = append(args, nameArgs...)
		args = append(args, contentArgs...)
	}
	condition := " WHERE " + strings.Join(where, " AND ")
	countSQL := `SELECT COUNT(*) FROM files f` + condition
	countArgs := args
	if o.Source == "" && o.Extension == "" && nameSQL == "1=1" && mode != "content" {
		countSQL = `SELECT files FROM catalog_stats WHERE id=1`
		countArgs = nil
	}
	if e := s.conn.QueryRowContext(ctx, countSQL, countArgs...).Scan(&out.Total); e != nil {
		return out, e
	}
	sortSQL := "f.search_name,f.id"
	switch o.Sort {
	case "size":
		sortSQL = "f.size DESC,f.id"
	case "modified":
		sortSQL = "f.modified DESC,f.id"
	}
	paged := append(append([]any{}, args...), o.Limit, o.Offset)
	rows, e := s.conn.QueryContext(ctx, `SELECT `+resultColumns+resultFrom+condition+" ORDER BY "+sortSQL+" LIMIT ? OFFSET ?", paged...)
	if e != nil {
		return out, e
	}
	for rows.Next() {
		f, e := scanResult(rows)
		if e != nil {
			rows.Close()
			return out, e
		}
		out.Results = append(out.Results, f)
	}
	e = rows.Err()
	rows.Close()
	if e != nil {
		return out, e
	}
	if mode != "files" && s.attached {
		// Multiple files can share one compressed document. Build its preview
		// once per result page, then discard the small cache after the request.
		snippets := make(map[int64]string)
		for i := range out.Results {
			if err := ctx.Err(); err != nil {
				return out, err
			}
			if out.Results[i].TextStatus == "indexed" || out.Results[i].TextStatus == "truncated" {
				var docID int64
				err := s.conn.QueryRowContext(ctx, `SELECT l.doc_id FROM content.links l JOIN files f ON f.id=l.file_id WHERE f.id=? AND f.version=l.version`, out.Results[i].ID).Scan(&docID)
				if err != nil {
					continue
				}
				if preview, ok := snippets[docID]; ok {
					out.Results[i].Snippet = preview
					continue
				}
				txt, e := s.textLocked(ctx, out.Results[i].ID)
				if e == nil {
					preview := snippet(txt, q)
					snippets[docID] = preview
					out.Results[i].Snippet = preview
				}
			}
		}
	}
	if err := ctx.Err(); err != nil {
		return out, err
	}
	out.ElapsedMS = float64(time.Since(start).Microseconds()) / 1000
	return out, nil
}
func snippet(text, q string) string {
	idx := 0
	terms := strings.Fields(strings.ReplaceAll(q, `"`, ""))
	if len(terms) > 0 {
		lower := strings.ToLower(text)
		for _, term := range terms {
			if pos := strings.Index(lower, strings.ToLower(term)); pos >= 0 {
				// Case conversion may change UTF-8 byte lengths (e.g. K → k),
				// so locate the excerpt by rune count, not the lowercased offset.
				idx = utf8.RuneCountInString(lower[:pos])
				break
			}
		}
	}
	a := idx - 90
	if a < 0 {
		a = 0
	}
	b := idx + 200
	start, end, n := 0, len(text), 0
	for offset := range text {
		if n == a {
			start = offset
		}
		if n == b {
			end = offset
			break
		}
		n++
	}
	prefix, suffix := "", ""
	if a > 0 {
		prefix = "…"
	}
	if end < len(text) {
		suffix = "…"
	}
	return prefix + strings.Join(strings.Fields(text[start:end]), " ") + suffix
}
func (s *Store) textLocked(ctx context.Context, id int64) (string, error) {
	var data []byte
	e := s.conn.QueryRowContext(ctx, `SELECT d.payload FROM content.documents d JOIN content.links l ON l.doc_id=d.id JOIN main.files f ON f.id=l.file_id WHERE f.id=? AND f.version=l.version`, id).Scan(&data)
	if e != nil {
		return "", e
	}
	r, e := zlib.NewReader(bytes.NewReader(data))
	if e != nil {
		return "", e
	}
	defer r.Close()
	b, e := io.ReadAll(io.LimitReader(r, 9<<20))
	return string(b), e
}
func (s *Store) File(ctx context.Context, id int64) (FileResult, string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	f, e := scanResult(s.conn.QueryRowContext(ctx, `SELECT `+resultColumns+resultFrom+` WHERE f.id=?`, id))
	if e != nil {
		return f, "", e
	}
	txt := ""
	if f.TextStatus == "indexed" || f.TextStatus == "truncated" {
		if e = s.ensureContent(false); e != nil {
			if errors.Is(e, os.ErrNotExist) {
				f.TextStatus = "not_indexed"
				f.TextDetail = "Baza sadržaja nije prisutna u ovoj kopiji kataloga."
				return f, "", nil
			}
			return f, "", e
		}
		txt, e = s.textLocked(ctx, id)
		if errors.Is(e, sql.ErrNoRows) {
			f.TextStatus = "not_indexed"
			f.TextDetail = "Tekst dokumenta nije dostupan u ovoj kopiji baze sadržaja. Ponovo indeksirajte izvor."
			return f, "", nil
		}
	}
	return f, txt, e
}
func (s *Store) Export(destination string, includeContent bool) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	encryption := s.encryption.clone()
	defer encryption.clear()
	return s.exportLocked(destination, includeContent, encryption)
}

// ExportProtected creates a separate encrypted copy. The source is never
// converted or removed, and no plaintext intermediate copy is written.
func (s *Store) ExportProtected(destination string, includeContent bool, password []byte) (string, error) {
	encryption, err := newEncryption(password)
	if err != nil {
		return "", err
	}
	defer encryption.clear()
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.exportLocked(destination, includeContent, encryption)
}

func (s *Store) exportLocked(destination string, includeContent bool, encryption *encryptionState) (result string, resultErr error) {
	if e := s.checkDataDir(); e != nil {
		return "", e
	}
	if strings.TrimSpace(destination) == "" {
		return "", errors.New("Unesite novi direktorijum za izvoz")
	}
	if includeContent {
		if e := s.ensureContent(false); e != nil && !errors.Is(e, os.ErrNotExist) {
			return "", e
		}
	}
	abs, e := filepath.Abs(destination)
	if e != nil {
		return "", e
	}
	if _, e = os.Stat(abs); e == nil {
		return "", errors.New("Odredišni folder već postoji. Izaberite novo ime za izvoz.")
	}
	if !errors.Is(e, os.ErrNotExist) {
		return "", e
	}
	if e = os.Mkdir(abs, 0700); e != nil {
		return "", e
	}
	directoryInfo, e := os.Stat(abs)
	if e != nil {
		return "", e
	}
	owned := make(map[string]os.FileInfo)
	create := func(name string, data []byte) error {
		p := filepath.Join(abs, name)
		f, err := os.OpenFile(p, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if err != nil {
			return err
		}
		if info, err := f.Stat(); err == nil {
			owned[name] = info
		}
		_, writeErr := f.Write(data)
		return errors.Join(writeErr, f.Sync(), f.Close())
	}
	const incomplete = ".allfiles-export-incomplete"
	defer func() {
		if resultErr == nil {
			return
		}
		now, err := os.Stat(abs)
		if err != nil || !os.SameFile(directoryInfo, now) {
			return
		}
		// Only remove exact files we created, after checking their identity.
		// An incomplete marker remains if an unexpected file prevents cleanup.
		for name, info := range owned {
			if name == incomplete {
				continue
			}
			p := filepath.Join(abs, name)
			if now, err := os.Stat(p); err == nil && os.SameFile(info, now) {
				_ = os.Remove(p)
			}
		}
		entries, err := os.ReadDir(abs)
		if err == nil && len(entries) == 1 && entries[0].Name() == incomplete {
			if now, err := os.Stat(filepath.Join(abs, incomplete)); err == nil && os.SameFile(owned[incomplete], now) {
				_ = os.Remove(filepath.Join(abs, incomplete))
				_ = os.Remove(abs)
			}
		}
	}()
	if e = create(incomplete, []byte("AllFiles: izvoz nije dovršen.\n")); e != nil {
		return "", e
	}
	if encryption != nil {
		b, err := encryption.manifestBytes()
		if err != nil {
			return "", err
		}
		if e = create(encryptionManifestName, b); e != nil {
			return "", e
		}
	}
	targetVFS, e := registerEncryptionVFS(abs, encryption)
	if e != nil {
		return "", e
	}
	defer vfs.Unregister(targetVFS)
	if e = create("catalog.sqlite", nil); e != nil {
		return "", e
	}
	if _, e = s.conn.ExecContext(context.Background(), `VACUUM main INTO ?`, sqliteURI(filepath.Join(abs, "catalog.sqlite"), targetVFS)); e != nil {
		return "", e
	}
	if !includeContent {
		copyDB, err := openSQLite(filepath.Join(abs, "catalog.sqlite"), targetVFS)
		if err != nil {
			return "", err
		}
		_, err = copyDB.Exec(`UPDATE files SET text_status='not_indexed',text_detail='Izvezen je samo katalog; tekst dokumenata nije uključen.' WHERE text_status IN ('indexed','truncated')`)
		closeErr := copyDB.Close()
		if err != nil {
			return "", err
		}
		if closeErr != nil {
			return "", closeErr
		}
	}
	if includeContent {
		e = s.ensureContent(false)
		if e != nil && !errors.Is(e, os.ErrNotExist) {
			return "", e
		}
		if e == nil {
			if e = create("content.sqlite", nil); e != nil {
				return "", e
			}
			if _, e = s.conn.ExecContext(context.Background(), `VACUUM content INTO ?`, sqliteURI(filepath.Join(abs, "content.sqlite"), targetVFS)); e != nil {
				return "", e
			}
		}
	}
	for _, name := range []string{"catalog.sqlite", "content.sqlite"} {
		p := filepath.Join(abs, name)
		if _, err := os.Stat(p); errors.Is(err, os.ErrNotExist) {
			continue
		}
		os.Chmod(p, 0600)
		check, err := openSQLite(p, targetVFS)
		if err != nil {
			return "", err
		}
		var integrity, owner string
		err = check.QueryRow(`PRAGMA quick_check`).Scan(&integrity)
		if err == nil {
			err = check.QueryRow(`SELECT value FROM meta WHERE key='catalog_id'`).Scan(&owner)
		}
		err = errors.Join(err, check.Close())
		if err != nil {
			return "", err
		}
		if integrity != "ok" || owner != s.catalogID {
			return "", errors.New("Provera izvezene baze nije uspela. Ponovite izvoz u novi folder.")
		}
		// Explicitly flush the finished file rather than relying on VACUUM INTO
		// or a read-only verification connection to persist the output.
		output, err := os.OpenFile(p, os.O_RDWR, 0)
		if err != nil {
			return "", err
		}
		if err = errors.Join(output.Sync(), output.Close()); err != nil {
			return "", err
		}
	}
	if e = os.Remove(filepath.Join(abs, incomplete)); e != nil {
		return "", e
	}
	// Directory fsync is supported on Unix filesystems; Windows and some
	// removable filesystems reject it, so file flushes above are authoritative.
	if directory, err := os.Open(abs); err == nil {
		_ = directory.Sync()
		_ = directory.Close()
	}
	return abs, nil
}
