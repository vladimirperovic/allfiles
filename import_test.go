package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf16"
)

func BenchmarkDecodeUTF16Export(b *testing.B) {
	text := strings.Repeat("D:\\Arhiva\\Čaše\\projekat-2026.pdf,12345,BD-01\r\n", 1000)
	var encoded bytes.Buffer
	encoded.Write([]byte{0xff, 0xfe})
	for _, unit := range utf16.Encode([]rune(text)) {
		_ = binary.Write(&encoded, binary.LittleEndian, unit)
	}
	data := encoded.Bytes()
	b.SetBytes(int64(len(data)))
	b.ReportAllocs()
	b.ResetTimer()
	for n := 0; n < b.N; n++ {
		reader, err := decodedExport(bytes.NewReader(data))
		if err != nil {
			b.Fatal(err)
		}
		if _, err := io.Copy(io.Discard, reader); err != nil {
			b.Fatal(err)
		}
	}
}

func TestCSVExportBoundsBeforeRecordAllocation(t *testing.T) {
	cases := []struct{ name, input string }{
		{"header", "Filename," + strings.Repeat("a", 70000)},
		{"columns", "Filename," + strings.Repeat("Size,", 300) + "Disk\n"},
		{"record", "Filename,Size\n\"" + strings.Repeat("a\n", (4<<20)/2+1)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			input := strings.NewReader(tc.input)
			err := parseCSVExport(context.Background(), input, func(importRecord) error { return nil })
			if err == nil || !strings.Contains(err.Error(), "limit") {
				t.Fatalf("missing bounded-record rejection: %v", err)
			}
		})
	}
	input := "Filename;Size;Disk\n\"line\nwith \"\"quote\"\".pdf\";12;BD-01\nnormal.pdf;15;BD-01\n"
	var count int
	if err := parseCSVExport(context.Background(), strings.NewReader(input), func(importRecord) error { count++; return nil }); err != nil || count != 2 {
		t.Fatalf("valid multiline CSV damaged: records=%d err=%v", count, err)
	}
}

func TestImportFlushesInterleavedMediaBeforeReadingWholeExport(t *testing.T) {
	var export strings.Builder
	export.WriteString("Filename,Size,Disk\n")
	for i := 0; i < 4500; i++ {
		fmt.Fprintf(&export, "file-%04d.bin,12,BD-%03d\n", i, i%100)
	}
	export.WriteString("tail.bin,12,Late media\n")
	file := filepath.Join(t.TempDir(), "interleaved.csv")
	if err := os.WriteFile(file, []byte(export.String()), 0600); err != nil {
		t.Fatal(err)
	}
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	err = Import(ctx, store, ImportOptions{Path: file}, func(p Progress) {
		if p.Scanned > 0 {
			cancel()
		}
	})
	if err != context.Canceled {
		t.Fatalf("expected cancellation after first bounded flush: %v", err)
	}
	sources, err := store.Sources()
	if err != nil {
		t.Fatal(err)
	}
	for _, source := range sources {
		if source.Label == "Late media" {
			t.Fatal("import buffered every media group before its first write")
		}
	}
}

func TestXMLExportBoundsBeforeTokenAllocation(t *testing.T) {
	input := "<report><file name=\"" + strings.Repeat("x", (4<<20)+1) + "\"/></report>"
	err := parseXMLExport(context.Background(), strings.NewReader(input), func(importRecord) error { return nil })
	if err == nil || !strings.Contains(err.Error(), "token") || !strings.Contains(err.Error(), "limit") {
		t.Fatalf("unbounded XML token: %v", err)
	}
}

func TestExportLimitsApplyPerRecordOrToken(t *testing.T) {
	longComment := strings.Repeat("x", 50000)
	csv := "Filename,Comment\n" + strings.Repeat("plan.pdf,"+longComment+"\n", 90)
	count := 0
	if err := parseCSVExport(context.Background(), strings.NewReader(csv), func(importRecord) error { count++; return nil }); err != nil || count != 90 {
		t.Fatalf("valid CSV stream over4MiB: count=%d err=%v", count, err)
	}
	xml := "<report>" + strings.Repeat(`<file name="plan.pdf" size="12"/>`, 150000) + "</report>"
	count = 0
	if err := parseXMLExport(context.Background(), strings.NewReader(xml), func(importRecord) error { count++; return nil }); err != nil || count != 150000 {
		t.Fatalf("valid XML stream over4MiB: count=%d err=%v", count, err)
	}
}

func TestMetadataReimportInvalidatesChangedDocumentText(t *testing.T) {
	store, err := OpenStore(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer store.Close()
	file := filepath.Join(t.TempDir(), "report.csv")
	write := func(size int) {
		t.Helper()
		if err := os.WriteFile(file, []byte(fmt.Sprintf("Filename,File Path,Size,Disk\nplan.pdf,unmounted/plan.pdf,%d,BD-01\n", size)), 0600); err != nil {
			t.Fatal(err)
		}
	}
	write(10)
	if err := Import(context.Background(), store, ImportOptions{Path: file}, nil); err != nil {
		t.Fatal(err)
	}
	result, err := store.Search(context.Background(), SearchOptions{Query: "plan.pdf", Mode: "files"})
	if err != nil || len(result.Results) != 1 {
		t.Fatalf("import: %#v %v", result, err)
	}
	id := result.Results[0].ID
	if err := store.RecordContent(id, "10:0", "indexed", "outdatedarchivetext", ""); err != nil {
		t.Fatal(err)
	}
	write(20)
	if err := Import(context.Background(), store, ImportOptions{Path: file}, nil); err != nil {
		t.Fatal(err)
	}
	result, err = store.Search(context.Background(), SearchOptions{Query: "outdatedarchivetext", Mode: "content"})
	if err != nil || result.Total != 0 {
		t.Fatalf("stale text retained: %#v %v", result, err)
	}
	metadata, text, err := store.File(context.Background(), id)
	if err != nil || metadata.TextStatus != "not_indexed" || text != "" {
		t.Fatalf("metadata import claims current text: %#v %q %v", metadata, text, err)
	}
}

func TestUTF16StreamingSurrogatesAndMalformedInput(t *testing.T) {
	var encoded bytes.Buffer
	encoded.Write([]byte{0xff, 0xfe})
	for _, unit := range utf16.Encode([]rune("Čaše 😀 2026")) {
		_ = binary.Write(&encoded, binary.LittleEndian, unit)
	}
	reader, err := decodedExport(bytes.NewReader(encoded.Bytes()))
	if err != nil {
		t.Fatal(err)
	}
	var decoded strings.Builder
	var one [1]byte
	for {
		n, err := reader.Read(one[:])
		decoded.Write(one[:n])
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
	}
	if decoded.String() != "Čaše 😀 2026" {
		t.Fatalf("split rune lost: %q", decoded.String())
	}
	for _, body := range [][]byte{{0x00, 0xdc}, {0x00, 0xd8}, {0x00, 0xd8, 0x41, 0x00}, {0x41}} {
		input := append([]byte{0xff, 0xfe}, body...)
		reader, err := decodedExport(bytes.NewReader(input))
		if err != nil {
			t.Fatal(err)
		}
		if _, err := io.ReadAll(reader); err == nil {
			t.Fatalf("invalid UTF16 accepted: %x", body)
		}
	}
}

func TestCSVExportMediaAndWindowsPaths(t *testing.T) {
	input := "\ufeffFilename;File Path;Size;Date Modified;Disk;Physical Location\nkuhinja.pdf;D:\\Projekti\\kuhinja.pdf;42;2026-10-20;BD-01;Polica 2\nkuhinja.pdf;D:\\Projekti\\kuhinja.pdf;84;2026-10-21;BD-02;Polica 3\n"
	reader, err := decodedExport(strings.NewReader(input))
	if err != nil {
		t.Fatal(err)
	}
	var records []importRecord
	err = parseCSVExport(context.Background(), reader, func(r importRecord) error { records = append(records, r); return nil })
	if err != nil {
		t.Fatal(err)
	}
	if len(records) != 2 {
		t.Fatalf("got %d records", len(records))
	}
	if records[0].Media == records[1].Media {
		t.Fatal("distinct discs were conflated")
	}
	if records[0].File.Relative != "D:/Projekti/kuhinja.pdf" || records[0].File.Dir != "D:/Projekti" {
		t.Fatalf("wrong path %#v", records[0].File)
	}
	if records[0].Location != "Polica 2" || records[0].File.Size != 42 || records[0].File.Modified == 0 {
		t.Fatalf("metadata lost: %#v", records[0])
	}
}

func TestUTF16EFUAndDirectorySkip(t *testing.T) {
	input := "Filename,Size,Date Modified,Attributes\r\n\"E:\\Čaše\\plan.txt\",10,133000000000000000,32\r\n\"E:\\Čaše\",,133000000000000000,16\r\n"
	var encoded bytes.Buffer
	encoded.Write([]byte{0xff, 0xfe})
	for _, unit := range utf16.Encode([]rune(input)) {
		if err := binary.Write(&encoded, binary.LittleEndian, unit); err != nil {
			t.Fatal(err)
		}
	}
	reader, err := decodedExport(&encoded)
	if err != nil {
		t.Fatal(err)
	}
	var records []importRecord
	err = parseCSVExport(context.Background(), reader, func(r importRecord) error { records = append(records, r); return nil })
	if err != nil {
		t.Fatal(err)
	}
	if len(records) != 1 || records[0].File.Relative != "E:/Čaše/plan.txt" {
		t.Fatalf("bad decoded records %#v", records)
	}
	if records[0].File.Modified != 1655526400 {
		t.Fatalf("bad FILETIME conversion: %d", records[0].File.Modified)
	}
}

func TestImportRejectsAmbiguousAndMalformedRows(t *testing.T) {
	for _, input := range []string{"unknown,thing\na,b\n", "Filename,Size\na.pdf,2 MB\n", "Filename,Size\na.pdf,-1\n", "Filename,Size\na.pdf,42,extra\n"} {
		if err := parseCSVExport(context.Background(), strings.NewReader(input), func(r importRecord) error { return nil }); err == nil {
			t.Fatalf("expected error for %q", input)
		}
	}
	if err := parseXMLExport(context.Background(), strings.NewReader(`<report><unknown>unsupported</unknown></report>`), func(r importRecord) error { return nil }); err == nil {
		t.Fatal("unrecognized XML reported success")
	}
}

func TestXMLExportWithMediaAndFolderContext(t *testing.T) {
	input := `<report><disk name="BD-01" location="Archive"><folder name="Projects"><file name="plan.pdf" size="99"/></folder></disk><disk name="BD-02"><file><Filename>plan.pdf</Filename><FilePath>Other/plan.pdf</FilePath><Size>11</Size></file></disk></report>`
	var records []importRecord
	err := parseXMLExport(context.Background(), strings.NewReader(input), func(r importRecord) error { records = append(records, r); return nil })
	if err != nil {
		t.Fatal(err)
	}
	if len(records) != 2 {
		t.Fatalf("records: %#v", records)
	}
	if records[0].Media != "BD-01" || records[0].File.Relative != "Projects/plan.pdf" || records[0].Location != "Archive" {
		t.Fatalf("missing context: %#v", records[0])
	}
	if records[1].Media != "BD-02" || records[1].File.Relative != "Other/plan.pdf" {
		t.Fatalf("bad second file: %#v", records[1])
	}
}
