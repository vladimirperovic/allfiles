package main

import (
	"bufio"
	"bytes"
	"compress/zlib"
	"context"
	"database/sql"
	"errors"
	"io"
	"os"
	"strings"
)

type FileTextPage struct {
	Text          string `json:"text"`
	Offset        int    `json:"offset"`
	ReturnedChars int    `json:"returned_chars"`
	TotalChars    *int   `json:"total_chars,omitempty"`
	HasMore       bool   `json:"has_more"`
}

// FilePage keeps document responses bounded for AI clients. A metadata-only
// request never reads the compressed payload. Text requests count runes while
// streaming, retaining only the requested excerpt instead of a full rune copy.
func (s *Store) FilePage(ctx context.Context, id int64, offset, maxChars int) (FileResult, FileTextPage, error) {
	page := FileTextPage{Offset: offset}
	if offset < 0 || offset > maxTextBytes || maxChars < 0 || maxChars > 20000 {
		return FileResult{}, page, errors.New("Invalid document text page")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.checkDataDir(); err != nil {
		return FileResult{}, page, err
	}
	f, err := scanResult(s.conn.QueryRowContext(ctx, `SELECT `+resultColumns+resultFrom+` WHERE f.id=?`, id))
	if err != nil {
		return f, page, err
	}
	total := 0
	if maxChars > 0 {
		page.TotalChars = &total
	}
	if f.TextStatus != "indexed" && f.TextStatus != "truncated" {
		return f, page, nil
	}
	if err = s.ensureContent(false); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			f.TextStatus = "not_indexed"
			f.TextDetail = "Baza sadržaja nije prisutna u ovoj kopiji kataloga."
			return f, page, nil
		}
		return f, page, err
	}
	var docID int64
	err = s.conn.QueryRowContext(ctx, `SELECT l.doc_id FROM content.links l JOIN files f ON f.id=l.file_id WHERE f.id=? AND f.version=l.version`, id).Scan(&docID)
	if errors.Is(err, sql.ErrNoRows) {
		f.TextStatus = "not_indexed"
		f.TextDetail = "Tekst dokumenta nije dostupan u ovoj kopiji baze sadržaja. Ponovo indeksirajte izvor."
		return f, page, nil
	}
	if err != nil {
		return f, page, err
	}
	if maxChars == 0 {
		page.HasMore = true
		return f, page, nil
	}
	var payload []byte
	if err = s.conn.QueryRowContext(ctx, `SELECT payload FROM content.documents WHERE id=?`, docID).Scan(&payload); err != nil {
		return f, page, err
	}
	r, err := zlib.NewReader(bytes.NewReader(payload))
	if err != nil {
		return f, page, err
	}
	defer r.Close()
	reader := bufio.NewReader(io.LimitReader(r, maxTextBytes+1))
	var selected strings.Builder
	for consumed := 0; ; {
		if total%4096 == 0 {
			if err := ctx.Err(); err != nil {
				return f, page, err
			}
		}
		char, size, err := reader.ReadRune()
		if err == io.EOF {
			break
		}
		if err != nil {
			return f, page, err
		}
		consumed += size
		if consumed > maxTextBytes {
			return f, page, errors.New("Stored document exceeds the text limit")
		}
		if total >= offset && total-offset < maxChars {
			selected.WriteRune(char)
			page.ReturnedChars++
		}
		total++
	}
	page.Text = selected.String()
	page.HasMore = offset+page.ReturnedChars < total
	return f, page, nil
}
