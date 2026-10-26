package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"
	"time"
)

type app struct {
	store       *Store
	base, token string
	assets      fs.FS
	mu          sync.Mutex
	job         *Progress
	cancel      context.CancelFunc
	wg          sync.WaitGroup
	shutdown    func()
	stopping    bool
}

func newApp(s *Store, base string) *app { return &app{store: s, base: base, token: randomID()} }
func jsonReply(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.WriteHeader(status)
	json.NewEncoder(w).Encode(v)
}
func apiError(w http.ResponseWriter, err error) {
	code := http.StatusBadRequest
	if errors.Is(err, sql.ErrNoRows) || errors.Is(err, os.ErrNotExist) {
		code = 404
	}
	jsonReply(w, code, map[string]string{"error": err.Error()})
}
func decode(w http.ResponseWriter, r *http.Request, v any) error {
	r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
	d := json.NewDecoder(r.Body)
	d.DisallowUnknownFields()
	if e := d.Decode(v); e != nil {
		return errors.New("Neispravni podaci zahteva")
	}
	var extra any
	if e := d.Decode(&extra); e != io.EOF {
		return errors.New("Neočekivani dodatni podaci")
	}
	return nil
}
func (a *app) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("X-Content-Type-Options", "nosniff")
	w.Header().Set("Referrer-Policy", "no-referrer")
	w.Header().Set("Cross-Origin-Resource-Policy", "same-origin")
	w.Header().Set("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'")
	if r.Host != strings.TrimPrefix(a.base, "http://") {
		jsonReply(w, 403, map[string]string{"error": "Pristup dozvoljen samo lokalnoj aplikaciji"})
		return
	}
	if origin := r.Header.Get("Origin"); origin != "" && origin != a.base {
		jsonReply(w, 403, map[string]string{"error": "Nedozvoljeno poreklo zahteva"})
		return
	}
	if r.Header.Get("Sec-Fetch-Site") == "cross-site" {
		jsonReply(w, 403, map[string]string{"error": "Nedozvoljen spoljni zahtev"})
		return
	}
	if strings.HasPrefix(r.URL.Path, "/api/") {
		if r.Method != "GET" && r.Method != "POST" {
			w.WriteHeader(405)
			return
		}
		if r.Method == "POST" && r.Header.Get("X-AllFiles-Token") != a.token {
			jsonReply(w, 403, map[string]string{"error": "Sesija nije važeća. Osvežite stranicu."})
			return
		}
		a.api(w, r)
		return
	}
	if r.Method != "GET" && r.Method != "HEAD" {
		w.WriteHeader(405)
		return
	}
	http.FileServer(http.FS(a.assets)).ServeHTTP(w, r)
}
func (a *app) jobSnapshot() *Progress {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.job == nil {
		return nil
	}
	j := *a.job
	return &j
}
func (a *app) startJob(work func(context.Context, func(Progress)) error) (*Progress, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.stopping {
		return nil, errors.New("Aplikacija se zatvara")
	}
	if a.job != nil && a.job.Status == "running" {
		return nil, errors.New("Indeksiranje je već u toku")
	}
	ctx, cancel := context.WithCancel(context.Background())
	a.cancel = cancel
	id := randomID()
	a.job = &Progress{ID: id, Status: "running", Phase: "Priprema"}
	a.wg.Add(1)
	go func() {
		defer a.wg.Done()
		defer cancel()
		e := work(ctx, func(p Progress) { a.mu.Lock(); p.ID = id; p.Status = "running"; a.job = &p; a.mu.Unlock() })
		a.mu.Lock()
		defer a.mu.Unlock()
		a.job.Current = ""
		if errors.Is(e, context.Canceled) {
			a.job.Status = "cancelled"
			a.job.Phase = "Zaustavljeno"
		} else if e != nil {
			a.job.Status = "failed"
			a.job.Error = e.Error()
			a.job.Phase = "Nije završeno"
		} else {
			a.job.Status = "completed"
			a.job.Phase = "Završeno"
		}
	}()
	j := *a.job
	return &j, nil
}
func (a *app) cancelAndWait() {
	a.mu.Lock()
	a.stopping = true
	if a.cancel != nil {
		a.cancel()
	}
	a.mu.Unlock()
	a.wg.Wait()
}
func (a *app) api(w http.ResponseWriter, r *http.Request) {
	get := func() bool {
		if r.Method != "GET" {
			w.WriteHeader(405)
			return false
		}
		return true
	}
	post := func() bool {
		if r.Method != "POST" {
			w.WriteHeader(405)
			return false
		}
		return true
	}
	switch r.URL.Path {
	case "/api/session":
		if !get() {
			return
		}
		jsonReply(w, 200, map[string]string{"token": a.token})
	case "/api/status":
		if !get() {
			return
		}
		v, e := a.store.Stats()
		if e != nil {
			apiError(w, e)
			return
		}
		j := a.jobSnapshot()
		v["job"] = progressJSON(j)
		jsonReply(w, 200, v)
	case "/api/sources":
		if !get() {
			return
		}
		q := r.URL.Query()
		if q.Has("id") || q.Has("offset") || q.Has("limit") {
			offset, limit := 0, 100
			var err error
			if q.Has("offset") {
				offset, err = strconv.Atoi(q.Get("offset"))
				if err != nil {
					apiError(w, errors.New("Neispravan pomak izvora"))
					return
				}
			}
			if q.Has("limit") {
				limit, err = strconv.Atoi(q.Get("limit"))
				if err != nil {
					apiError(w, errors.New("Neispravan broj izvora"))
					return
				}
			}
			v, total, e := a.store.SourcesPage(q.Get("id"), offset, limit)
			if e != nil {
				apiError(w, e)
				return
			}
			jsonReply(w, 200, map[string]any{"sources": v, "total": total, "offset": offset, "limit": limit, "has_more": int64(offset+len(v)) < total})
			return
		}
		v, e := a.store.Sources()
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, map[string]any{"sources": v})
	case "/api/search":
		if !get() {
			return
		}
		q := r.URL.Query()
		if len(q.Get("q")) > 1000 {
			apiError(w, errors.New("Upit je predugačak"))
			return
		}
		offset, _ := strconv.Atoi(q.Get("offset"))
		limit, _ := strconv.Atoi(q.Get("limit"))
		ctx, cancel := context.WithTimeout(r.Context(), 15*time.Second)
		defer cancel()
		v, e := a.store.Search(ctx, SearchOptions{Query: q.Get("q"), Mode: q.Get("mode"), Source: q.Get("source"), Extension: q.Get("ext"), Sort: q.Get("sort"), Offset: offset, Limit: limit})
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, v)
	case "/api/file":
		if !get() {
			return
		}
		q := r.URL.Query()
		id, _ := strconv.ParseInt(q.Get("id"), 10, 64)
		if q.Has("text_offset") || q.Has("max_chars") {
			offset, limit := 0, 4000
			var err error
			if q.Has("text_offset") {
				offset, err = strconv.Atoi(q.Get("text_offset"))
				if err != nil || offset < 0 || offset > 8388608 {
					apiError(w, errors.New("Invalid document text offset"))
					return
				}
			}
			if q.Has("max_chars") {
				limit, err = strconv.Atoi(q.Get("max_chars"))
				if err != nil || limit < 0 || limit > 20000 {
					apiError(w, errors.New("Invalid document text page size"))
					return
				}
			}
			f, page, e := a.store.FilePage(r.Context(), id, offset, limit)
			if e != nil {
				apiError(w, e)
				return
			}
			jsonReply(w, 200, filePageResponse{File: f, FileTextPage: page, TextStatus: f.TextStatus, TextDetail: f.TextDetail})
			return
		}
		f, t, e := a.store.File(r.Context(), id)
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, map[string]any{"file": f, "text": t, "text_status": f.TextStatus, "text_detail": f.TextDetail})
	case "/api/scan":
		if !post() {
			return
		}
		var v struct {
			Root         string `json:"root"`
			Label        string `json:"label"`
			Kind         string `json:"kind"`
			Location     string `json:"location"`
			SourceID     string `json:"source_id"`
			IndexContent bool   `json:"index_content"`
		}
		if e := decode(w, r, &v); e != nil {
			apiError(w, e)
			return
		}
		if strings.HasPrefix(v.Root, "import://") || strings.HasPrefix(v.Root, "demo://") {
			apiError(w, errors.New("Ovaj izvor se ažurira ponovnim uvozom"))
			return
		}
		if !filepath.IsAbs(v.Root) {
			apiError(w, errors.New("Unesite apsolutnu putanju foldera"))
			return
		}
		o := ScanOptions{Root: v.Root, Label: v.Label, Kind: v.Kind, Location: v.Location, SourceID: v.SourceID, IndexContent: v.IndexContent}
		j, e := a.startJob(func(ctx context.Context, p func(Progress)) error { return Scan(ctx, a.store, o, p) })
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 202, map[string]any{"job": progressJSON(j)})
	case "/api/import":
		if !post() {
			return
		}
		var v struct {
			Path     string `json:"path"`
			Label    string `json:"label"`
			Kind     string `json:"kind"`
			Location string `json:"location"`
		}
		if e := decode(w, r, &v); e != nil {
			apiError(w, e)
			return
		}
		j, e := a.startJob(func(ctx context.Context, p func(Progress)) error {
			return Import(ctx, a.store, ImportOptions{Path: v.Path, Label: v.Label, Kind: v.Kind, Location: v.Location}, p)
		})
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 202, map[string]any{"job": progressJSON(j)})
	case "/api/cancel":
		if !post() {
			return
		}
		a.mu.Lock()
		if a.cancel != nil {
			a.cancel()
		}
		a.mu.Unlock()
		jsonReply(w, 200, map[string]bool{"ok": true})
	case "/api/export":
		if !post() {
			return
		}
		var v struct {
			Destination    string `json:"destination"`
			IncludeContent bool   `json:"include_content"`
			Encrypt        bool   `json:"encrypt"`
			Password       string `json:"password"`
		}
		if e := decode(w, r, &v); e != nil {
			apiError(w, e)
			return
		}
		password := []byte(v.Password)
		v.Password = ""
		defer clear(password)
		if v.Encrypt {
			if e := ValidateNewPassword(password); e != nil {
				apiError(w, e)
				return
			}
		} else if len(password) != 0 {
			apiError(w, errors.New("Za novu lozinku uključite šifrovanje kopije"))
			return
		}
		a.mu.Lock()
		if a.job != nil && a.job.Status == "running" {
			a.mu.Unlock()
			apiError(w, errors.New("Sačekajte da se indeksiranje završi pre izvoza"))
			return
		}
		var p string
		var e error
		if v.Encrypt {
			p, e = a.store.ExportProtected(v.Destination, v.IncludeContent, password)
		} else {
			p, e = a.store.Export(v.Destination, v.IncludeContent)
		}
		a.mu.Unlock()
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, map[string]any{"path": p, "encrypted": v.Encrypt || a.store.Encrypted()})
	case "/api/browse":
		if !get() {
			return
		}
		v, e := browseContext(r.Context(), r.URL.Query().Get("path"))
		if e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, v)
	case "/api/open":
		if !post() {
			return
		}
		var v struct {
			ID     int64 `json:"id"`
			Folder bool  `json:"folder"`
		}
		if e := decode(w, r, &v); e != nil {
			apiError(w, e)
			return
		}
		f, _, e := a.store.FilePage(r.Context(), v.ID, 0, 0)
		if e != nil {
			apiError(w, e)
			return
		}
		abs, e := validateOpenPath(f, v.Folder)
		if e != nil {
			apiError(w, e)
			return
		}
		if e = openLocal(abs); e != nil {
			apiError(w, e)
			return
		}
		jsonReply(w, 200, map[string]bool{"ok": true})
	case "/api/shutdown":
		if !post() {
			return
		}
		jsonReply(w, 200, map[string]bool{"ok": true})
		go func() {
			a.cancelAndWait()
			if a.shutdown != nil {
				a.shutdown()
			}
		}()
	default:
		jsonReply(w, 404, map[string]string{"error": "Nepoznata ruta"})
	}
}

type filePageResponse struct {
	File FileResult `json:"file"`
	FileTextPage
	TextStatus string `json:"text_status"`
	TextDetail string `json:"text_detail"`
}

func progressJSON(p *Progress) any {
	if p == nil {
		return nil
	}
	return map[string]any{"id": p.ID, "status": p.Status, "phase": p.Phase, "current": p.Current, "error": p.Error, "scanned": p.Scanned, "indexed": p.Indexed}
}
func browse(path string) (map[string]any, error) {
	return browseContext(context.Background(), path)
}

func browseContext(ctx context.Context, path string) (map[string]any, error) {
	type entry struct {
		Name  string `json:"name"`
		Path  string `json:"path"`
		IsDir bool   `json:"is_dir"`
	}
	entries := []entry{}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if path == "" {
		if runtime.GOOS == "windows" {
			for c := 'A'; c <= 'Z'; c++ {
				if err := ctx.Err(); err != nil {
					return nil, err
				}
				p := fmt.Sprintf("%c:\\", c)
				if st, e := os.Stat(p); e == nil && st.IsDir() {
					entries = append(entries, entry{p, p, true})
				}
			}
			return map[string]any{"path": "", "parent": "", "entries": entries, "truncated": false}, nil
		}
		path = "/"
	}
	if !filepath.IsAbs(path) {
		return nil, errors.New("Putanja mora biti apsolutna")
	}
	directory, e := os.Open(path)
	if e != nil {
		return nil, e
	}
	defer directory.Close()
	truncated := false
readDirectories:
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		items, readErr := directory.ReadDir(256)
		for _, it := range items {
			if it.Type()&os.ModeSymlink != 0 || !it.IsDir() {
				continue
			}
			if len(entries) == 2000 {
				truncated = true
				break readDirectories
			}
			entries = append(entries, entry{it.Name(), filepath.Join(path, it.Name()), true})
		}
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			return nil, readErr
		}
	}
	sort.Slice(entries, func(i, j int) bool { return entries[i].Name < entries[j].Name })
	parent := filepath.Dir(path)
	if parent == path {
		parent = ""
	}
	return map[string]any{"path": path, "parent": parent, "entries": entries, "truncated": truncated}, nil
}
func openLocal(path string) error {
	var c *exec.Cmd
	switch runtime.GOOS {
	case "windows":
		c = exec.Command("rundll32.exe", "url.dll,FileProtocolHandler", path)
	case "darwin":
		c = exec.Command("open", path)
	default:
		c = exec.Command("xdg-open", path)
	}
	if e := c.Start(); e != nil {
		return e
	}
	go c.Wait()
	return nil
}
