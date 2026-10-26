package main

import (
	"archive/zip"
	"bytes"
	"context"
	"encoding/binary"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"unicode/utf16"
	"unicode/utf8"

	pdf "github.com/ledongthuc/pdf"
)

const maxTextBytes = 8 << 20
const maxDocumentBytes = 256 << 20
const maxXMLBytes = 32 << 20
const maxExpandedBytes = 64 << 20

var errTextLimit = errors.New("text extraction limit reached")

// IsTextCandidate accepts either a dotted or bare file extension.
func IsTextCandidate(extension string) bool {
	switch strings.TrimPrefix(strings.ToLower(extension), ".") {
	case "pdf", "docx", "pptx", "xlsx", "odt", "ods", "odp", "txt", "md", "csv", "tsv", "log", "json", "xml", "html", "htm":
		return true
	default:
		return false
	}
}

type textSink struct {
	bytes.Buffer
	limit int
}

func (s *textSink) add(text string) error {
	if text == "" {
		return nil
	}
	left := s.limit - s.Len()
	if len(text) > left {
		text = text[:left]
		for len(text) > 0 && !utf8.ValidString(text) {
			text = text[:len(text)-1]
		}
		s.WriteString(text)
		return errTextLimit
	}
	s.WriteString(text)
	return nil
}

// ExtractText does not persist temporary plaintext. Status describes whether
// content is searchable, unavailable, unsupported, or only partially indexed.
func ExtractText(path string) (text, status, detail string) {
	return ExtractTextContext(context.Background(), path)
}

func ExtractTextContext(ctx context.Context, path string) (text, status, detail string) {
	status = "error"
	defer func() {
		if recovered := recover(); recovered != nil {
			text = ""
			status = "error"
			detail = fmt.Sprintf("Document parser rejected malformed content: %v", recovered)
		}
	}()
	ext := strings.ToLower(filepath.Ext(path))
	switch ext {
	case ".pdf", ".docx", ".pptx", ".xlsx", ".odt", ".ods", ".odp", ".txt", ".md", ".csv", ".tsv", ".log", ".json", ".xml", ".html", ".htm", ".rtf":
	case ".doc", ".xls", ".ppt":
		return "", "unsupported", "Legacy binary Office documents require conversion to DOCX/XLSX/PPTX; no content was indexed."
	default:
		return "", "unsupported", "Content extraction is not available for this file type."
	}
	if err := ctx.Err(); err != nil {
		return "", "error", err.Error()
	}
	before, err := os.Lstat(path)
	if err != nil {
		return "", "error", err.Error()
	}
	if !before.Mode().IsRegular() || before.Mode()&os.ModeSymlink != 0 {
		return "", "error", "Content extraction only opens regular files, without following symbolic links."
	}
	if before.Size() > maxDocumentBytes {
		return "", "skipped", "Document exceeds the 256 MiB input limit; content was not extracted."
	}
	sink := &textSink{limit: maxTextBytes}
	switch ext {
	case ".pdf":
		err = extractPDF(ctx, path, sink)
	case ".docx", ".pptx", ".xlsx", ".odt", ".ods", ".odp":
		err = extractOffice(ctx, path, ext, sink)
	case ".rtf":
		return "", "unsupported", "RTF extraction is not yet implemented; the filename remains searchable."
	default:
		err = extractPlain(ctx, path, sink)
	}
	if ctx.Err() != nil {
		return "", "error", ctx.Err().Error()
	}
	after, statErr := os.Lstat(path)
	if statErr != nil {
		return "", "error", statErr.Error()
	}
	if before.Size() != after.Size() || !before.ModTime().Equal(after.ModTime()) {
		return "", "error", "File changed during extraction; scan again to index its current contents."
	}
	text = strings.TrimSpace(strings.ToValidUTF8(sink.String(), " "))
	if errors.Is(err, errTextLimit) {
		if text == "" {
			return "", "skipped", "Document exceeds extraction limits (8 MiB text / 64 MiB expanded XML / 20,000 PDF pages); content was not extracted."
		}
		return text, "truncated", "Only a bounded portion was indexed (8 MiB text / 64 MiB expanded document limit)."
	}
	if err != nil {
		if ext == ".pdf" && (errors.Is(err, pdf.ErrInvalidPassword) || strings.Contains(strings.ToLower(err.Error()), "encrypt") || strings.Contains(strings.ToLower(err.Error()), "password")) {
			return "", "encrypted", "Encrypted PDF could not be opened without a password."
		}
		return "", "error", err.Error()
	}
	if text == "" {
		if ext == ".pdf" {
			return "", "needs_ocr", "No extractable text. This PDF may be image-only or use unsupported fonts; OCR is not included."
		}
		return "", "empty", "Document contains no extractable text."
	}
	return text, "indexed", ""
}

func extractPlain(ctx context.Context, path string, sink *textSink) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, maxTextBytes*2+4))
	if err != nil {
		return err
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	truncated := len(data) > maxTextBytes*2
	if truncated {
		data = data[:maxTextBytes*2]
	}
	if bytes.HasPrefix(data, []byte{0xff, 0xfe}) || bytes.HasPrefix(data, []byte{0xfe, 0xff}) {
		var order binary.ByteOrder = binary.LittleEndian
		if data[0] == 0xfe {
			order = binary.BigEndian
		}
		data = data[2:]
		units := make([]uint16, len(data)/2)
		for i := range units {
			units[i] = order.Uint16(data[i*2 : i*2+2])
		}
		data = []byte(string(utf16.Decode(units)))
	} else {
		data = bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
		if bytes.IndexByte(data, 0) >= 0 {
			return errors.New("file appears binary or uses unsupported text encoding")
		}
	}
	if err := sink.add(strings.ToValidUTF8(string(data), " ")); err != nil {
		return err
	}
	if truncated {
		return errTextLimit
	}
	return nil
}

func extractPDF(ctx context.Context, path string, sink *textSink) error {
	file, reader, err := pdf.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	pages := reader.NumPage()
	if pages > 20000 {
		return errTextLimit
	}
	for page := 1; page <= pages; page++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		p := reader.Page(page)
		if p.V.IsNull() {
			continue
		}
		text, err := p.GetPlainText(nil)
		if err != nil {
			return fmt.Errorf("PDF page %d: %w", page, err)
		}
		if err := sink.add(text + "\n"); err != nil {
			return err
		}
	}
	return nil
}

func officePart(name, ext string) bool {
	switch ext {
	case ".docx":
		return name == "word/document.xml" || name == "word/footnotes.xml" || name == "word/endnotes.xml" || (strings.HasPrefix(name, "word/header") || strings.HasPrefix(name, "word/footer")) && strings.HasSuffix(name, ".xml")
	case ".pptx":
		return (strings.HasPrefix(name, "ppt/slides/slide") || strings.HasPrefix(name, "ppt/notesSlides/notesSlide")) && strings.HasSuffix(name, ".xml")
	case ".xlsx":
		return name == "xl/sharedStrings.xml" || strings.HasPrefix(name, "xl/worksheets/sheet") && strings.HasSuffix(name, ".xml")
	default:
		return name == "content.xml"
	}
}

func extractOffice(ctx context.Context, path, ext string, sink *textSink) error {
	archive, err := zip.OpenReader(path)
	if err != nil {
		return fmt.Errorf("invalid Office/OpenDocument archive: %w", err)
	}
	defer archive.Close()
	if len(archive.File) > 10000 {
		return fmt.Errorf("%w: excessive ZIP entries", errTextLimit)
	}
	parts := make([]*zip.File, 0)
	var expanded uint64
	for _, part := range archive.File {
		if !officePart(part.Name, ext) {
			continue
		}
		if part.UncompressedSize64 > maxXMLBytes || part.UncompressedSize64 > maxExpandedBytes-expanded {
			return fmt.Errorf("%w: expanded XML exceeds limit", errTextLimit)
		}
		if part.CompressedSize64 > 0 && part.UncompressedSize64/part.CompressedSize64 > 1000 {
			return fmt.Errorf("%w: suspicious compression ratio", errTextLimit)
		}
		expanded += part.UncompressedSize64
		parts = append(parts, part)
	}
	if len(parts) == 0 {
		return errors.New("document archive has no recognized text parts")
	}
	sort.Slice(parts, func(i, j int) bool {
		if parts[i].Name == "xl/sharedStrings.xml" {
			return true
		}
		if parts[j].Name == "xl/sharedStrings.xml" {
			return false
		}
		return parts[i].Name < parts[j].Name
	})
	var shared []string
	for _, part := range parts {
		if err := ctx.Err(); err != nil {
			return err
		}
		reader, err := part.Open()
		if err != nil {
			return err
		}
		limited := io.LimitReader(reader, maxXMLBytes+1)
		if ext == ".xlsx" && part.Name == "xl/sharedStrings.xml" {
			shared, err = readSharedStrings(ctx, limited)
		} else if ext == ".xlsx" {
			err = readSheet(ctx, limited, shared, sink)
		} else {
			err = readDocumentXML(ctx, limited, ext, sink)
		}
		closeErr := reader.Close()
		if err != nil {
			return err
		}
		if closeErr != nil {
			return closeErr
		}
		if err := sink.add("\n"); err != nil {
			return err
		}
	}
	return nil
}

func readDocumentXML(ctx context.Context, reader io.Reader, ext string, sink *textSink) error {
	decoder := xml.NewDecoder(reader)
	textDepth, paragraphDepth := 0, 0
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		token, err := decoder.Token()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return fmt.Errorf("invalid document XML: %w", err)
		}
		switch value := token.(type) {
		case xml.StartElement:
			name := value.Name.Local
			if ext == ".odt" || ext == ".ods" || ext == ".odp" {
				if name == "p" || name == "h" {
					paragraphDepth++
				}
				if name == "s" && paragraphDepth > 0 {
					if err := sink.add(" "); err != nil {
						return err
					}
				}
			} else if name == "t" {
				textDepth++
			}
			if name == "tab" || name == "br" || name == "line-break" {
				if err := sink.add(" "); err != nil {
					return err
				}
			}
		case xml.CharData:
			if textDepth > 0 || paragraphDepth > 0 {
				if err := sink.add(string(value)); err != nil {
					return err
				}
			}
		case xml.EndElement:
			name := value.Name.Local
			if name == "t" && textDepth > 0 {
				textDepth--
			}
			if name == "p" || name == "h" {
				if paragraphDepth > 0 {
					paragraphDepth--
				}
				if err := sink.add("\n"); err != nil {
					return err
				}
			}
		}
	}
}

func readSharedStrings(ctx context.Context, reader io.Reader) ([]string, error) {
	decoder := xml.NewDecoder(reader)
	var shared []string
	var current strings.Builder
	inText, inItem, total := false, false, 0
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		token, err := decoder.Token()
		if err == io.EOF {
			return shared, nil
		}
		if err != nil {
			return nil, err
		}
		switch value := token.(type) {
		case xml.StartElement:
			if value.Name.Local == "si" {
				inItem = true
				current.Reset()
			}
			if value.Name.Local == "t" && inItem {
				inText = true
			}
		case xml.CharData:
			if inText {
				total += len(value)
				if total > maxTextBytes {
					return nil, errTextLimit
				}
				current.Write(value)
			}
		case xml.EndElement:
			if value.Name.Local == "t" {
				inText = false
			}
			if value.Name.Local == "si" {
				shared = append(shared, current.String())
				inItem = false
				if len(shared) > 500000 {
					return nil, errTextLimit
				}
			}
		}
	}
}

func readSheet(ctx context.Context, reader io.Reader, shared []string, sink *textSink) error {
	decoder := xml.NewDecoder(reader)
	kind := ""
	inValue, inText, inCell := false, false, false
	var value, inline strings.Builder
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		token, err := decoder.Token()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return err
		}
		switch element := token.(type) {
		case xml.StartElement:
			switch element.Name.Local {
			case "c":
				inCell = true
				kind = ""
				value.Reset()
				inline.Reset()
				for _, attr := range element.Attr {
					if attr.Name.Local == "t" {
						kind = attr.Value
					}
				}
			case "v":
				inValue = inCell
			case "t":
				inText = inCell
			}
		case xml.CharData:
			if inValue {
				if value.Len()+len(element) > maxTextBytes {
					return errTextLimit
				}
				value.Write(element)
			}
			if inText {
				if inline.Len()+len(element) > maxTextBytes {
					return errTextLimit
				}
				inline.Write(element)
			}
		case xml.EndElement:
			switch element.Name.Local {
			case "v":
				inValue = false
			case "t":
				inText = false
			case "c":
				text := value.String()
				if kind == "s" {
					index, err := strconv.Atoi(strings.TrimSpace(text))
					if err != nil || index < 0 || index >= len(shared) {
						return errors.New("invalid XLSX shared string reference")
					}
					text = shared[index]
				} else if kind == "inlineStr" {
					text = inline.String()
				}
				if err := sink.add(text + "\t"); err != nil {
					return err
				}
				inCell = false
			case "row":
				if err := sink.add("\n"); err != nil {
					return err
				}
			}
		}
	}
}
