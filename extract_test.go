package main

import (
	"archive/zip"
	"bytes"
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func makeOfficeFixture(t *testing.T, extension string, parts map[string]string) string {
	t.Helper()
	file := filepath.Join(t.TempDir(), "document"+extension)
	output, err := os.Create(file)
	if err != nil {
		t.Fatal(err)
	}
	archive := zip.NewWriter(output)
	for name, body := range parts {
		writer, err := archive.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = writer.Write([]byte(body)); err != nil {
			t.Fatal(err)
		}
	}
	if err = archive.Close(); err != nil {
		t.Fatal(err)
	}
	if err = output.Close(); err != nil {
		t.Fatal(err)
	}
	return file
}

func TestOfficeTextExtraction(t *testing.T) {
	cases := []struct {
		name, ext string
		parts     map[string]string
		want      []string
	}{
		{"word", ".docx", map[string]string{"word/document.xml": `<w:document xmlns:w="word"><w:body><w:p><w:r><w:t>Kuhinja čaša</w:t></w:r></w:p></w:body></w:document>`, "word/header1.xml": `<w:hdr xmlns:w="word"><w:p><w:r><w:t>Projekat</w:t></w:r></w:p></w:hdr>`}, []string{"Kuhinja čaša", "Projekat"}},
		{"presentation", ".pptx", map[string]string{"ppt/slides/slide1.xml": `<p:sld xmlns:p="presentation" xmlns:a="drawing"><a:p><a:r><a:t>Plan projekta</a:t></a:r></a:p></p:sld>`}, []string{"Plan projekta"}},
		{"spreadsheet", ".xlsx", map[string]string{"xl/sharedStrings.xml": `<sst><si><t>Troškovi</t></si><si><r><t>unused string</t></r></si></sst>`, "xl/worksheets/sheet1.xml": `<worksheet><sheetData><row><c t="s"><v>0</v></c><c><v>42</v></c><c t="inlineStr"><is><t>Materijal</t></is></c></row></sheetData></worksheet>`}, []string{"Troškovi", "42", "Materijal"}},
		{"open_document", ".odt", map[string]string{"content.xml": `<office:document xmlns:office="office" xmlns:text="text"><office:body><text:p>Stan <text:span>Marković</text:span></text:p></office:body></office:document>`}, []string{"Stan Marković"}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			file := makeOfficeFixture(t, tc.ext, tc.parts)
			text, status, detail := ExtractText(file)
			if status != "indexed" {
				t.Fatalf("status %s: %s", status, detail)
			}
			for _, want := range tc.want {
				if !strings.Contains(text, want) {
					t.Errorf("missing %q in %q", want, text)
				}
			}
			if strings.Contains(text, "unused string") {
				t.Error("unused shared string leaked into spreadsheet text")
			}
		})
	}
}

func makePDFFixture(t *testing.T, text string) string {
	t.Helper()
	var output bytes.Buffer
	output.WriteString("%PDF-1.4\n")
	stream := ""
	if text != "" {
		stream = "BT /F1 12 Tf 72 720 Td (" + text + ") Tj ET"
	}
	objects := []string{`<< /Type /Catalog /Pages 2 0 R >>`, `<< /Type /Pages /Kids [3 0 R] /Count 1 >>`, `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>`, `<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>`, fmt.Sprintf("<< /Length %d >>\nstream\n%s\nendstream", len(stream), stream)}
	offsets := []int{0}
	for index, object := range objects {
		offsets = append(offsets, output.Len())
		fmt.Fprintf(&output, "%d 0 obj\n%s\nendobj\n", index+1, object)
	}
	xref := output.Len()
	fmt.Fprintf(&output, "xref\n0 %d\n0000000000 65535 f \n", len(offsets))
	for _, offset := range offsets[1:] {
		fmt.Fprintf(&output, "%010d 00000 n \n", offset)
	}
	fmt.Fprintf(&output, "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n", len(offsets), xref)
	file := filepath.Join(t.TempDir(), "fixture.pdf")
	if err := os.WriteFile(file, output.Bytes(), 0600); err != nil {
		t.Fatal(err)
	}
	return file
}

func TestPDFTextAndEmptyStatus(t *testing.T) {
	text, status, detail := ExtractText(makePDFFixture(t, "Archive project 2026"))
	if status != "indexed" || !strings.Contains(text, "Archive project 2026") {
		t.Fatalf("%s: %s / %q", status, detail, text)
	}
	_, status, detail = ExtractText(makePDFFixture(t, ""))
	if status != "needs_ocr" {
		t.Fatalf("expected explicit no-text status, got %s: %s", status, detail)
	}
}

func TestExtractionLimitsAndFailures(t *testing.T) {
	dir := t.TempDir()
	plain := filepath.Join(dir, "large.txt")
	if err := os.WriteFile(plain, []byte(strings.Repeat("x", maxTextBytes+123)), 0600); err != nil {
		t.Fatal(err)
	}
	text, status, detail := ExtractText(plain)
	if status != "truncated" || len(text) > maxTextBytes || detail == "" {
		t.Fatalf("limit missing: %s %d %s", status, len(text), detail)
	}
	legacy := filepath.Join(dir, "old.doc")
	_, status, detail = ExtractText(legacy)
	if status != "unsupported" || !strings.Contains(detail, "conversion") {
		t.Fatalf("legacy document mislabeled: %s %s", status, detail)
	}
	corrupt := filepath.Join(dir, "broken.pdf")
	if err := os.WriteFile(corrupt, []byte("this is not a PDF"), 0600); err != nil {
		t.Fatal(err)
	}
	_, status, _ = ExtractText(corrupt)
	if status != "error" {
		t.Fatalf("corrupt PDF status: %s", status)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, status, detail = ExtractTextContext(ctx, plain)
	if status != "error" || !strings.Contains(detail, "canceled") {
		t.Fatalf("cancellation ignored: %s %s", status, detail)
	}
	bomb := makeOfficeFixture(t, ".docx", map[string]string{"word/document.xml": "<document>" + strings.Repeat("a", 2<<20) + "</document>"})
	_, status, detail = ExtractText(bomb)
	if status != "skipped" {
		t.Fatalf("ZIP expansion guard: %s %s", status, detail)
	}
	oversize := filepath.Join(dir, "oversize.pdf")
	large, err := os.Create(oversize)
	if err != nil {
		t.Fatal(err)
	}
	if err := large.Truncate(maxDocumentBytes + 1); err != nil {
		large.Close()
		t.Fatal(err)
	}
	large.Close()
	text, status, detail = ExtractText(oversize)
	if status != "skipped" || text != "" {
		t.Fatalf("unread content reported searchable: %s %q %s", status, text, detail)
	}
}
