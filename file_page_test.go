package main

import (
	"context"
	"strings"
	"testing"
	"unicode/utf8"
)

func TestFilePageUnicodeAndMetadataOnly(t *testing.T) {
	s := newDatabaseTestStore(t)
	source := databaseTestSource(t, s, "pages")
	f := databaseTestUpsert(t, s, source, "scan", databaseTestFile("long.txt", 100, 1e9))[0]
	text := strings.Repeat("Ađ界🌊", 1000)
	if err := s.RecordContent(f.ID, f.Version, "indexed", text, ""); err != nil {
		t.Fatal(err)
	}
	for _, offset := range []int{0, 3, 3997, 4000, 5000} {
		_, page, err := s.FilePage(context.Background(), f.ID, offset, 6)
		if err != nil {
			t.Fatal(err)
		}
		runes := []rune(text)
		start := min(offset, len(runes))
		end := min(start+6, len(runes))
		if page.Text != string(runes[start:end]) || page.TotalChars == nil || *page.TotalChars != 4000 || page.HasMore != (end < 4000) || page.ReturnedChars != end-start || page.Offset != offset {
			t.Fatalf("page at %d: %#v", offset, page)
		}
	}
	// An invalid payload proves metadata-only requests do not decompress it.
	if _, err := s.conn.ExecContext(context.Background(), `UPDATE content.documents SET payload=x'00'`); err != nil {
		t.Fatal(err)
	}
	_, page, err := s.FilePage(context.Background(), f.ID, 0, 0)
	if err != nil || page.Text != "" || page.TotalChars != nil || !page.HasMore {
		t.Fatalf("metadata-only request: %#v %v", page, err)
	}
	if _, _, err := s.FilePage(context.Background(), f.ID, 0, 1); err == nil {
		t.Fatal("text request must report the corrupt payload")
	}
}

func TestSnippetUnicodeCaseAndWindow(t *testing.T) {
	// K lowercases to a shorter UTF-8 sequence; a byte-based offset would cut
	// the wrong excerpt. Emojis must never be split between UTF-8 bytes.
	text := strings.Repeat("K🌊", 150) + "Needle" + strings.Repeat("界", 300)
	got := snippet(text, "needle")
	if !utf8.ValidString(got) || !strings.Contains(got, "Needle") || !strings.HasPrefix(got, "…") || !strings.HasSuffix(got, "…") || utf8.RuneCountInString(got) != 292 {
		t.Fatalf("invalid Unicode excerpt: %q", got)
	}
	if got := snippet("  A\n B   C  ", ""); got != "A B C" {
		t.Fatalf("whitespace: %q", got)
	}
	if got := snippet("", "query"); got != "" {
		t.Fatalf("empty text: %q", got)
	}
	if got := snippet(strings.Repeat("🌊", 500), ""); utf8.RuneCountInString(got) != 201 {
		t.Fatalf("initial excerpt size: %d", utf8.RuneCountInString(got))
	}
}
