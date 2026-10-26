package main

import (
	"bytes"
	"context"
	"encoding/binary"
	"strings"
	"testing"
	"unicode/utf16"
)

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
