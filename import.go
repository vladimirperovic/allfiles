package main

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/csv"
	"encoding/hex"
	"encoding/xml"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf16"
	"unicode/utf8"
)

const maxImportPending = 4096
const maxCSVRecordBytes = 4 << 20
const maxCSVHeaderBytes = 64 << 10
const maxImportColumns = 256
const maxImportXMLTokenBytes = 4 << 20

type ImportOptions struct {
	Path     string `json:"path"`
	Label    string `json:"label"`
	Kind     string `json:"kind"`
	Location string `json:"location"`
}

type importRecord struct {
	File            ScannedFile
	Media, Location string
}
type importBatch struct {
	Source  Source
	Entries []ScannedFile
}

// Import validates the export before changing the catalog. Unknown XML schemas
// and ambiguous CSV columns fail explicitly, leaving previous sources intact.
func Import(ctx context.Context, store *Store, opts ImportOptions, report func(Progress)) (err error) {
	abs, err := filepath.Abs(opts.Path)
	if err != nil {
		return err
	}
	info, err := os.Stat(abs)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() {
		return errors.New("import requires a regular export file")
	}
	if info.Size() > 2<<30 {
		return errors.New("export exceeds the 2 GiB import limit; split it into smaller exports")
	}
	state := Progress{ID: newScanID(), Status: "running", Phase: "validating"}
	emit := func() {
		if report != nil {
			report(state)
		}
	}
	emit()
	parse := func(emitRecord func(importRecord) error) error {
		file, err := os.Open(abs)
		if err != nil {
			return err
		}
		defer file.Close()
		reader, err := decodedExport(file)
		if err != nil {
			return err
		}
		switch strings.ToLower(filepath.Ext(abs)) {
		case ".xml":
			return parseXMLExport(ctx, reader, emitRecord)
		case ".csv", ".tsv", ".efu":
			return parseCSVExport(ctx, reader, emitRecord)
		default:
			return errors.New("use a CSV, TSV, EFU, or supported XML report; a native WinCatalog database must be exported first")
		}
	}
	count := int64(0)
	err = parse(func(record importRecord) error {
		count++
		if count%4096 == 0 {
			state.Current = fmt.Sprintf("%d entries checked", count)
			emit()
		}
		return nil
	})
	if err != nil {
		state.Status = "error"
		state.Error = err.Error()
		emit()
		return err
	}
	if count == 0 {
		return errors.New("no file records found; include filename and file path columns in the export")
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	current, err := os.Stat(abs)
	if err != nil {
		return err
	}
	if current.Size() != info.Size() || !current.ModTime().Equal(info.ModTime()) {
		return errors.New("export changed during validation; retry after saving it")
	}
	if opts.Label == "" {
		opts.Label = strings.TrimSuffix(filepath.Base(abs), filepath.Ext(abs))
	}
	if opts.Kind == "" {
		opts.Kind = "bluray"
	}
	digest := sha256.Sum256([]byte(filepath.Clean(abs)))
	root := "import://" + hex.EncodeToString(digest[:16])
	groups := map[string]*importBatch{}
	pending := 0
	complete := false
	defer func() {
		for _, group := range groups {
			if finishErr := store.FinishScan(group.Source.ID, state.ID, complete); err == nil {
				err = finishErr
			}
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
	flush := func(group *importBatch) error {
		if len(group.Entries) == 0 {
			return nil
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		// UpsertFiles already marks fresh/changed documents as not_indexed.
		// Repeating that status in one durable transaction per file makes a
		// metadata-only document export much slower than an ordinary file list.
		_, err := store.UpsertFiles(group.Source.ID, state.ID, group.Entries)
		if err != nil {
			return err
		}
		state.Scanned += int64(len(group.Entries))
		pending -= len(group.Entries)
		// Release path strings and backing slices for sparse/interleaved media.
		group.Entries = nil
		emit()
		return nil
	}
	state.Phase = "importing"
	emit()
	err = parse(func(record importRecord) error {
		groupKey := record.Media
		group := groups[groupKey]
		if group == nil {
			if len(groups) >= 10000 {
				return errors.New("export contains more than 10,000 media groups; split the export")
			}
			label := record.Media
			if label == "" {
				label = opts.Label
			}
			location := record.Location
			if opts.Location != "" {
				location = opts.Location
			}
			mediaHash := sha256.Sum256([]byte(groupKey))
			source, err := store.BeginScan(ScanOptions{Root: root + "/" + hex.EncodeToString(mediaHash[:8]), Label: label, Kind: opts.Kind, Location: location})
			if err != nil {
				return err
			}
			group = &importBatch{Source: source}
			groups[groupKey] = group
		}
		group.Entries = append(group.Entries, record.File)
		pending++
		state.Current = record.File.Relative
		if len(group.Entries) >= 256 {
			return flush(group)
		}
		if pending >= maxImportPending {
			for _, buffered := range groups {
				if err := flush(buffered); err != nil {
					return err
				}
			}
		}
		return nil
	})
	if err != nil {
		return err
	}
	for _, group := range groups {
		if err = flush(group); err != nil {
			return err
		}
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	current, err = os.Stat(abs)
	if err != nil {
		return err
	}
	if current.Size() != info.Size() || !current.ModTime().Equal(info.ModTime()) {
		return errors.New("export changed during import; previous entries were retained, retry after saving it")
	}
	complete = true
	return nil
}

// decodedExport supports the UTF-16 reports commonly exported by Windows tools
// without loading the whole report into memory.
func decodedExport(input io.Reader) (io.Reader, error) {
	reader := bufio.NewReader(input)
	prefix, _ := reader.Peek(3)
	if len(prefix) >= 3 && string(prefix[:3]) == "\xef\xbb\xbf" {
		_, _ = reader.Discard(3)
		return reader, nil
	}
	if len(prefix) >= 2 && (string(prefix[:2]) == "\xff\xfe" || string(prefix[:2]) == "\xfe\xff") {
		var order binary.ByteOrder = binary.LittleEndian
		if prefix[0] == 0xfe {
			order = binary.BigEndian
		}
		_, _ = reader.Discard(2)
		return &utf16Reader{reader: reader, order: order}, nil
	}
	return reader, nil
}

type utf16Reader struct {
	reader     io.Reader
	order      binary.ByteOrder
	pair       [2]byte
	pending    [utf8.UTFMax]byte
	start, end int
	err        error
}

func (r *utf16Reader) Read(out []byte) (int, error) {
	if len(out) == 0 {
		return 0, nil
	}
	for r.start == r.end && r.err == nil {
		_, err := io.ReadFull(r.reader, r.pair[:])
		if err != nil {
			r.err = err
			break
		}
		unit := r.order.Uint16(r.pair[:])
		value := rune(unit)
		if unit >= 0xd800 && unit <= 0xdbff {
			_, err = io.ReadFull(r.reader, r.pair[:])
			if err != nil {
				r.err = errors.New("truncated UTF-16 surrogate pair")
				break
			}
			low := r.order.Uint16(r.pair[:])
			if low < 0xdc00 || low > 0xdfff {
				r.err = errors.New("invalid UTF-16 surrogate pair")
				break
			}
			value = utf16.DecodeRune(rune(unit), rune(low))
		} else if unit >= 0xdc00 && unit <= 0xdfff {
			r.err = errors.New("invalid UTF-16 surrogate pair")
			break
		}
		r.start = 0
		r.end = utf8.EncodeRune(r.pending[:], value)
	}
	if r.start < r.end {
		n := copy(out, r.pending[r.start:r.end])
		r.start += n
		return n, nil
	}
	return 0, r.err
}

func normalizeHeader(header string) string {
	return strings.Map(func(r rune) rune {
		if unicode.IsLetter(r) || unicode.IsDigit(r) {
			return unicode.ToLower(r)
		}
		return -1
	}, strings.TrimSpace(strings.TrimPrefix(header, "\ufeff")))
}
func firstField(fields map[string]string, keys ...string) string {
	for _, key := range keys {
		if value := strings.TrimSpace(fields[key]); value != "" {
			return value
		}
	}
	return ""
}

// encoding/csv otherwise allocates an entire quoted record before callers can
// inspect its fields. Bound bytes and columns while respecting quoted newlines
// and doubled quotes, without duplicating the CSV parser's syntax validation.
type csvRecordReader struct {
	reader                io.Reader
	ctx                   context.Context
	delimiter             byte
	limit, bytes, columns int
	quoted                bool
}

func (r *csvRecordReader) Read(out []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	n, err := r.reader.Read(out)
	for i, value := range out[:n] {
		r.bytes++
		if r.bytes > r.limit {
			return i, errors.New("CSV record exceeds byte limit")
		}
		if value == '"' {
			r.quoted = !r.quoted
		}
		if !r.quoted {
			if value == '\n' {
				r.bytes = 0
				r.columns = 0
			}
			if value == r.delimiter {
				r.columns++
				if r.columns >= maxImportColumns {
					return i, errors.New("CSV record exceeds column limit")
				}
			}
		}
	}
	return n, err
}

// XML tokens can also grow before Decoder.Token returns. Its InputOffset lets
// us cap bytes ahead of the last completed token, including parser read-ahead.
type xmlTokenReader struct {
	reader      io.Reader
	ctx         context.Context
	read, limit int64
}

func (r *xmlTokenReader) Read(out []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	remaining := r.limit - r.read
	if remaining <= 0 {
		return 0, errors.New("XML token exceeds 4 MiB limit")
	}
	if int64(len(out)) > remaining {
		out = out[:remaining]
	}
	n, err := r.reader.Read(out)
	r.read += int64(n)
	return n, err
}

func parseCSVExport(ctx context.Context, input io.Reader, emit func(importRecord) error) error {
	reader := bufio.NewReader(input)
	prefix, _ := reader.Peek(8192)
	line := strings.SplitN(string(prefix), "\n", 2)[0]
	if strings.HasPrefix(strings.ToLower(strings.TrimSpace(line)), "sep=") {
		if len(strings.TrimSpace(line)) != 5 {
			return errors.New("invalid CSV separator directive; limit is one separator")
		}
		_, _ = reader.ReadString('\n')
		prefix, _ = reader.Peek(8192)
		line = strings.SplitN(string(prefix), "\n", 2)[0]
	}
	comma := ','
	best := 0
	for _, delimiter := range []rune{',', ';', '\t'} {
		probe := csv.NewReader(strings.NewReader(line))
		probe.Comma = delimiter
		row, err := probe.Read()
		if err == nil && len(row) > best {
			best = len(row)
			comma = delimiter
		}
	}
	bounded := &csvRecordReader{reader: reader, ctx: ctx, delimiter: byte(comma), limit: maxCSVHeaderBytes}
	parser := csv.NewReader(bounded)
	parser.Comma = comma
	parser.FieldsPerRecord = -1
	parser.TrimLeadingSpace = true
	headers, err := parser.Read()
	if err != nil {
		return fmt.Errorf("read CSV header: %w", err)
	}
	bounded.limit = maxCSVRecordBytes
	known := map[string]bool{}
	for i := range headers {
		headers[i] = normalizeHeader(headers[i])
		known[headers[i]] = true
	}
	if !known["filename"] && !known["name"] && !known["fullpath"] && !known["filepath"] && !known["path"] {
		return errors.New("unrecognized CSV headers; include Filename and File Path (plus Disk or Volume Label for multiple media)")
	}
	for rowNumber := 2; ; rowNumber++ {
		if err := ctx.Err(); err != nil {
			return err
		}
		row, err := parser.Read()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return fmt.Errorf("CSV row %d: %w", rowNumber, err)
		}
		if len(row) != len(headers) {
			return fmt.Errorf("CSV row %d has %d columns; expected %d", rowNumber, len(row), len(headers))
		}
		fields := map[string]string{}
		for i, value := range row {
			if len(value) > 1<<20 {
				return fmt.Errorf("CSV row %d has an excessively long field", rowNumber)
			}
			fields[headers[i]] = value
		}
		record, skip, err := recordFromFields(fields)
		if err != nil {
			return fmt.Errorf("CSV row %d: %w", rowNumber, err)
		}
		if skip {
			continue
		}
		if err = emit(record); err != nil {
			return err
		}
	}
}

func recordFromFields(fields map[string]string) (importRecord, bool, error) {
	typeName := strings.ToLower(firstField(fields, "type", "itemtype", "kind"))
	if typeName == "folder" || typeName == "directory" || typeName == "disk" || typeName == "volume" || strings.EqualFold(firstField(fields, "isfolder", "isdirectory"), "true") {
		return importRecord{}, true, nil
	}
	if attrs := firstField(fields, "attributes"); attrs != "" {
		if value, err := strconv.ParseInt(attrs, 10, 64); err == nil && value&16 != 0 {
			return importRecord{}, true, nil
		}
	}
	name := firstField(fields, "filename", "name")
	full := firstField(fields, "fullpath", "filepath", "path", "catalogpath")
	name = strings.ReplaceAll(name, "\\", "/")
	full = strings.ReplaceAll(full, "\\", "/")
	if name == "" && full == "" {
		return importRecord{}, true, nil
	}
	if strings.HasSuffix(name, "/") || strings.HasSuffix(full, "/") && name == "" {
		return importRecord{}, true, nil
	}
	if full == "" {
		full = name
	} else if name != "" && path.Base(full) != path.Base(name) {
		full = strings.TrimRight(full, "/") + "/" + name
	}
	if strings.ContainsRune(full, 0) {
		return importRecord{}, false, errors.New("file path contains a null byte")
	}
	if len(full) > 32768 {
		return importRecord{}, false, errors.New("file path is longer than 32768 bytes")
	}
	// CSV fields share one backing string for the whole record. Keep only the
	// path instead of retaining unrelated large description/comment columns.
	full = strings.Clone(path.Clean(full))
	name = path.Base(full)
	if full == "." || name == ".." || name == "/" {
		return importRecord{}, true, nil
	}
	sizeText := firstField(fields, "sizebytes", "filesizebytes", "size", "filesize", "length")
	var size int64
	if sizeText != "" {
		clean := strings.ReplaceAll(strings.ReplaceAll(sizeText, " ", ""), "\u00a0", "")
		var err error
		size, err = strconv.ParseInt(clean, 10, 64)
		if err != nil || size < 0 {
			return importRecord{}, false, fmt.Errorf("invalid file size %q; export exact byte sizes without separators or units", sizeText)
		}
	}
	modified, modifiedNS := parseExportDate(firstField(fields, "datemodified", "modified", "modificationdate", "lastmodified", "modifiedtime"))
	dir := path.Dir(full)
	if dir == "." {
		dir = ""
	}
	media := firstField(fields, "disk", "disc", "medianame", "medium", "volumelabel", "volume", "diskname", "source")
	if serial := firstField(fields, "serialnumber", "disknumber"); serial != "" {
		if media != "" {
			media += " · "
		}
		media += serial
	}
	location := firstField(fields, "physicallocation", "location")
	if len(media) > 4096 || len(location) > 4096 {
		return importRecord{}, false, errors.New("media label or location exceeds 4096-byte limit")
	}
	return importRecord{File: ScannedFile{Relative: full, Name: name, Dir: dir, Extension: strings.ToLower(strings.TrimPrefix(path.Ext(name), ".")), Size: size, Modified: modified, ModifiedNS: modifiedNS}, Media: strings.Clone(media), Location: strings.Clone(location)}, false, nil
}

func parseExportDate(value string) (int64, int64) {
	if value == "" {
		return 0, 0
	}
	if number, err := strconv.ParseInt(value, 10, 64); err == nil {
		// Everything EFU uses Windows FILETIME (100 ns since 1601).
		if number > 100000000000000000 {
			unix100ns := number - 116444736000000000
			return unix100ns / 10000000, unix100ns * 100
		}
		if number > 1000000000000 {
			return number / 1000, number * 1000000
		}
		return number, number * 1000000000
	}
	for _, format := range []string{time.RFC3339Nano, "2006-01-02 15:04:05", "2006-01-02", "02.01.2006 15:04:05", "02.01.2006", "01/02/2006 15:04:05", "1/2/2006 3:04:05 PM"} {
		if date, err := time.Parse(format, value); err == nil {
			return date.Unix(), date.UnixNano()
		}
	}
	return 0, 0
}

type xmlImportFrame struct {
	tag                     string
	fields                  map[string]string
	text                    strings.Builder
	media, location, folder string
	children                bool
}

func parseXMLExport(ctx context.Context, input io.Reader, emit func(importRecord) error) error {
	bounded := &xmlTokenReader{reader: input, ctx: ctx, limit: maxImportXMLTokenBytes}
	decoder := xml.NewDecoder(bounded)
	// The input has already been transcoded according to its BOM.
	decoder.CharsetReader = func(charset string, reader io.Reader) (io.Reader, error) {
		if strings.EqualFold(charset, "utf-16") || strings.EqualFold(charset, "utf-8") {
			return reader, nil
		}
		return nil, fmt.Errorf("unsupported XML encoding %s", charset)
	}
	stack := []*xmlImportFrame{}
	count := 0
	for {
		if err := ctx.Err(); err != nil {
			return err
		}
		bounded.limit = decoder.InputOffset() + maxImportXMLTokenBytes
		token, err := decoder.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return fmt.Errorf("invalid XML report: %w", err)
		}
		switch element := token.(type) {
		case xml.StartElement:
			if len(element.Attr) > maxImportColumns {
				return errors.New("XML attribute count exceeds limit")
			}
			if len(stack) > 256 {
				return errors.New("XML nesting exceeds limit")
			}
			frame := &xmlImportFrame{tag: normalizeHeader(element.Name.Local), fields: map[string]string{}}
			if len(stack) > 0 {
				parent := stack[len(stack)-1]
				parent.children = true
				frame.media = parent.media
				frame.location = parent.location
				frame.folder = parent.folder
			}
			for _, attr := range element.Attr {
				frame.fields[normalizeHeader(attr.Name.Local)] = attr.Value
			}
			if frame.tag == "disk" || frame.tag == "disc" || frame.tag == "volume" || frame.tag == "medium" {
				if value := firstField(frame.fields, "name", "label", "volumelabel"); value != "" {
					frame.media = value
				}
				frame.location = firstField(frame.fields, "physicallocation", "location")
			}
			if frame.tag == "folder" || frame.tag == "directory" {
				folder := firstField(frame.fields, "path", "name")
				frame.folder = path.Join(frame.folder, strings.ReplaceAll(folder, "\\", "/"))
				frame.fields["type"] = "folder"
			}
			stack = append(stack, frame)
		case xml.CharData:
			if len(stack) > 0 {
				frame := stack[len(stack)-1]
				if frame.text.Len()+len(element) > 1<<20 {
					return errors.New("XML field exceeds 1 MiB limit")
				}
				frame.text.Write(element)
			}
		case xml.EndElement:
			if len(stack) == 0 {
				return errors.New("invalid XML stack")
			}
			frame := stack[len(stack)-1]
			stack = stack[:len(stack)-1]
			if frame.tag == "file" || frame.tag == "item" || frame.tag == "record" || frame.tag == "entry" {
				if firstField(frame.fields, "disk", "disc", "volume", "volumelabel") == "" {
					frame.fields["disk"] = frame.media
				}
				if firstField(frame.fields, "location", "physicallocation") == "" {
					frame.fields["location"] = frame.location
				}
				if firstField(frame.fields, "path", "filepath", "fullpath") == "" && frame.folder != "" {
					frame.fields["path"] = frame.folder
				}
				record, skip, err := recordFromFields(frame.fields)
				if err != nil {
					return err
				}
				if !skip {
					if err := emit(record); err != nil {
						return err
					}
					count++
				}
			} else if len(stack) > 0 && !frame.children {
				parent := stack[len(stack)-1]
				value := strings.TrimSpace(frame.text.String())
				if value == "" {
					value = firstField(frame.fields, "value")
				}
				key := frame.tag
				if key == "column" || key == "field" {
					key = normalizeHeader(firstField(frame.fields, "name", "key"))
				}
				if key != "" {
					if _, exists := parent.fields[key]; !exists && len(parent.fields) >= maxImportColumns {
						return errors.New("XML field count exceeds limit")
					}
					parent.fields[key] = value
				}
				if (parent.tag == "disk" || parent.tag == "volume") && (key == "name" || key == "label") {
					parent.media = value
				}
			}
		}
	}
	if count == 0 {
		return errors.New("unsupported XML report layout: no named file records found; export a CSV report with Filename, File Path, Size and Disk columns")
	}
	return nil
}
