use crate::{
    crypto,
    scanner::{self, Progress, SharedStore},
    store::{self, ScanOptions, ScannedFile, Source},
};
use anyhow::{Result, bail};
use quick_xml::{Reader, events::Event};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    collections::HashMap,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read},
    path::Path,
    rc::Rc,
    sync::atomic::AtomicBool,
};
#[derive(Default, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ImportOptions {
    pub path: String,
    pub label: String,
    pub kind: String,
    pub location: String,
}
struct Record {
    file: ScannedFile,
    media: String,
    location: String,
}
struct Batch {
    source: Source,
    entries: Vec<ScannedFile>,
}
pub fn import(
    store: &SharedStore,
    mut options: ImportOptions,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    let path = store::absolute(Path::new(&options.path))?;
    let before = fs::metadata(&path)?;
    if !before.is_file() || before.len() > 2 << 30 {
        bail!("Import requires a regular export file smaller than 2 GiB");
    }
    let mut state = Progress {
        id: crypto::random_id(),
        status: "running".into(),
        phase: "validating".into(),
        ..Default::default()
    };
    report(state.clone());
    let mut count = 0;
    let mut total_bytes = 0i64;
    parse(&path, cancel, &mut |record| {
        total_bytes = total_bytes.checked_add(record.file.size).ok_or_else(|| {
            anyhow::anyhow!(
                "Export total size exceeds the supported 64-bit limit (9223372036854775807 bytes)"
            )
        })?;
        count += 1;
        if count % 4096 == 0 {
            state.current = format!("{count} entries checked");
            report(state.clone());
        }
        Ok(())
    })?;
    if count == 0 {
        bail!("No file records found; include filename and file path columns in the export");
    }
    unchanged(&path, &before)?;
    scanner::check(cancel)?;
    if options.label.is_empty() {
        options.label = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    if options.kind.is_empty() {
        options.kind = "bluray".into();
    }
    let root = format!(
        "import://{}",
        hex::encode(&Sha256::digest(path.to_string_lossy().as_bytes())[..16])
    );
    let mut groups: HashMap<String, Batch> = HashMap::new();
    let mut pending = 0usize;
    state.phase = "importing".into();
    report(state.clone());
    let result = (|| -> Result<()> {
        parse(&path, cancel, &mut |record| {
            if !groups.contains_key(&record.media) {
                if groups.len() >= 10_000 {
                    bail!("Export contains more than 10,000 media groups; split the export");
                }
                let label = if record.media.is_empty() {
                    options.label.clone()
                } else {
                    record.media.clone()
                };
                let location = if options.location.is_empty() {
                    record.location.clone()
                } else {
                    options.location.clone()
                };
                let media_root = format!(
                    "{root}/{}",
                    hex::encode(&Sha256::digest(record.media.as_bytes())[..8])
                );
                let source = store.lock().unwrap().begin_scan(&ScanOptions {
                    root: media_root,
                    label,
                    kind: options.kind.clone(),
                    location,
                    ..Default::default()
                })?;
                groups.insert(
                    record.media.clone(),
                    Batch {
                        source,
                        entries: Vec::new(),
                    },
                );
            }
            let group = groups.get_mut(&record.media).unwrap();
            state.current = record.file.relative.clone();
            group.entries.push(record.file);
            pending += 1;
            if group.entries.len() >= scanner::BATCH || pending >= 4096 {
                flush(store, &mut groups, &mut pending, &mut state, cancel, report)?;
            }
            Ok(())
        })?;
        flush(store, &mut groups, &mut pending, &mut state, cancel, report)?;
        scanner::check(cancel)?;
        unchanged(&path, &before)?;
        Ok(())
    })();
    let sources = groups
        .values()
        .map(|g| g.source.id.as_str())
        .collect::<Vec<_>>();
    let finish = store
        .lock()
        .unwrap()
        .finish_scans(&sources, &state.id, result.is_ok());
    result.and(finish)
}
fn unchanged(path: &Path, before: &fs::Metadata) -> Result<()> {
    let now = fs::metadata(path)?;
    if now.len() != before.len() || now.modified()? != before.modified()? {
        bail!("Export changed during import; previous entries retained, retry after saving it");
    }
    Ok(())
}
fn flush(
    store: &SharedStore,
    groups: &mut HashMap<String, Batch>,
    pending: &mut usize,
    state: &mut Progress,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    if *pending == 0 {
        return Ok(());
    }
    scanner::check(cancel)?;
    // Interleaved media share one bounded transaction. Committing each small
    // group separately makes import time depend heavily on input row order.
    let batch = groups
        .values()
        .filter(|group| !group.entries.is_empty())
        .map(|group| (group.source.id.as_str(), group.entries.as_slice()))
        .collect::<Vec<_>>();
    store.lock().unwrap().upsert_groups(&state.id, &batch)?;
    state.scanned += *pending as i64;
    *pending = 0;
    for group in groups.values_mut() {
        // Release per-media capacity so a large number of media cannot retain
        // an unbounded multiple of the global batch limit.
        group.entries = Vec::new();
    }
    report(state.clone());
    Ok(())
}
fn parse(
    path: &Path,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Record) -> Result<()>,
) -> Result<()> {
    let decoded = encoding_rs_io::DecodeReaderBytesBuilder::new()
        .utf8_passthru(true)
        .build(File::open(path)?);
    match path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
        .as_str()
    {
        "csv" | "tsv" | "efu" => csv(decoded, cancel, emit),
        "xml" => xml(decoded, cancel, emit),
        _ => bail!(
            "Use a CSV, TSV, EFU or supported XML report; export a native WinCatalog database first"
        ),
    }
}
fn normalize(s: &str) -> String {
    s.trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn first<'a>(f: &'a HashMap<String, String>, keys: &[&str]) -> &'a str {
    keys.iter()
        .find_map(|k| f.get(*k).map(|s| s.trim()).filter(|s| !s.is_empty()))
        .unwrap_or("")
}
#[derive(Clone, Copy)]
enum CsvState {
    Start,
    Unquoted,
    Quoted,
    AfterQuote,
}
// csv::Trim acts after parsing, so it cannot recognize a quote preceded by
// whitespace. Strip leading Unicode whitespace before handing bytes to csv,
// while counting the original bytes so discarded padding cannot evade bounds.
struct CsvInput<R> {
    reader: BufReader<R>,
    state: CsvState,
    size: usize,
    columns: usize,
    header: bool,
    separator: u8,
    pending: [u8; 4],
    pending_start: usize,
    pending_end: usize,
}
impl<R: Read> CsvInput<R> {
    fn new(reader: R, separator: u8) -> Self {
        Self {
            reader: BufReader::new(reader),
            state: CsvState::Start,
            size: 0,
            columns: 1,
            header: true,
            separator,
            pending: [0; 4],
            pending_start: 0,
            pending_end: 0,
        }
    }
    fn byte(&mut self) -> io::Result<Option<u8>> {
        let mut byte = [0];
        if self.reader.read(&mut byte)? == 0 {
            return Ok(None);
        }
        self.size += 1;
        if self.size > if self.header { 65536 } else { 4 << 20 } {
            return Err(io::Error::other("CSV record or column limit exceeded"));
        }
        Ok(Some(byte[0]))
    }
    fn outside_quote(&mut self, byte: u8) -> io::Result<()> {
        if byte == self.separator {
            self.columns += 1;
            if self.columns > 256 {
                return Err(io::Error::other("CSV record or column limit exceeded"));
            }
            self.state = CsvState::Start;
        } else if matches!(byte, b'\n' | b'\r') {
            self.size = 0;
            self.columns = 1;
            self.header = false;
            self.state = CsvState::Start;
        } else {
            self.state = CsvState::Unquoted;
        }
        Ok(())
    }
}
impl<R: Read> Read for CsvInput<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < out.len().min(4096) {
            if self.pending_start < self.pending_end {
                out[written] = self.pending[self.pending_start];
                self.pending_start += 1;
                written += 1;
                continue;
            }
            let Some(byte) = self.byte()? else {
                break;
            };
            match self.state {
                CsvState::Start => {
                    if byte != self.separator && !matches!(byte, b'\n' | b'\r') {
                        if byte.is_ascii_whitespace() {
                            continue;
                        }
                        if byte >= 0x80 {
                            let width = match byte {
                                0xc2..=0xdf => 2,
                                0xe0..=0xef => 3,
                                0xf0..=0xf4 => 4,
                                _ => return Err(io::Error::other("Invalid UTF-8 in CSV")),
                            };
                            self.pending[0] = byte;
                            for i in 1..width {
                                self.pending[i] = self
                                    .byte()?
                                    .ok_or_else(|| io::Error::other("Incomplete UTF-8 in CSV"))?;
                            }
                            let text = std::str::from_utf8(&self.pending[..width])
                                .map_err(io::Error::other)?;
                            if text.chars().next().is_some_and(char::is_whitespace) {
                                continue;
                            }
                            self.pending_start = 0;
                            self.pending_end = width;
                            self.state = CsvState::Unquoted;
                            continue;
                        }
                    }
                    if byte == b'"' {
                        self.state = CsvState::Quoted;
                    } else {
                        self.outside_quote(byte)?;
                    }
                }
                CsvState::Quoted if byte == b'"' => self.state = CsvState::AfterQuote,
                CsvState::Quoted => {}
                CsvState::AfterQuote if byte == b'"' => self.state = CsvState::Quoted,
                CsvState::AfterQuote | CsvState::Unquoted => self.outside_quote(byte)?,
            }
            out[written] = byte;
            written += 1;
        }
        Ok(written)
    }
}
fn csv(
    reader: impl Read,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Record) -> Result<()>,
) -> Result<()> {
    let mut reader = BufReader::with_capacity(8192, reader);
    let mut prefix = Vec::new();
    (&mut reader).take(65537).read_until(b'\n', &mut prefix)?;
    if prefix.len() > 65536 {
        bail!("CSV header exceeds 64 KiB");
    }
    let first_line = String::from_utf8_lossy(&prefix)
        .split('\n')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned();
    let explicit = if first_line.to_lowercase().starts_with("sep=") {
        if first_line.len() != 5 {
            bail!("Invalid CSV separator directive");
        }
        let sep = first_line.as_bytes()[4];
        prefix.clear();
        (&mut reader).take(65537).read_until(b'\n', &mut prefix)?;
        if prefix.len() > 65536 {
            bail!("CSV header exceeds 64 KiB");
        }
        Some(sep)
    } else {
        None
    };
    let line = prefix.split(|b| *b == b'\n').next().unwrap_or(&[]);
    let separator = explicit.unwrap_or_else(|| {
        b",;\t"
            .iter()
            .copied()
            .max_by_key(|sep| {
                let mut quoted = false;
                let mut n = 0;
                for &b in line {
                    if b == b'"' {
                        quoted = !quoted;
                    }
                    if b == *sep && !quoted {
                        n += 1;
                    }
                }
                n
            })
            .unwrap()
    });
    if !b",;\t".contains(&separator) {
        bail!("Unsupported CSV separator");
    }
    let bounded = CsvInput::new(std::io::Cursor::new(prefix).chain(reader), separator);
    let mut csv = csv::ReaderBuilder::new()
        .delimiter(separator)
        .trim(csv::Trim::Headers)
        .from_reader(bounded);
    let headers = csv.headers()?.iter().map(normalize).collect::<Vec<_>>();
    if headers.len() > 256
        || !headers.iter().any(|s| {
            matches!(
                s.as_str(),
                "filename" | "name" | "fullpath" | "filepath" | "path"
            )
        })
    {
        bail!("Unrecognized CSV headers; include Filename and File Path");
    }
    for row in csv.records() {
        scanner::check(cancel)?;
        let row = row?;
        let mut fields = HashMap::new();
        for (header, value) in headers.iter().zip(row.iter()) {
            if value.len() > 1 << 20 {
                bail!("CSV field exceeds 1 MiB limit");
            }
            fields.insert(header.clone(), value.to_owned());
        }
        if let Some(record) = record(fields)? {
            emit(record)?;
        }
    }
    Ok(())
}
fn clean_path(value: &str) -> String {
    let absolute = value.starts_with('/');
    let unc = value.starts_with("//") && !value.starts_with("///");
    let drive = value.as_bytes().get(1) == Some(&b':') && value.as_bytes().get(2) == Some(&b'/');
    let root_parts = if unc { 2 } else { usize::from(drive) };
    let mut parts = Vec::new();
    for p in value.split('/') {
        match p {
            "" | "." => {}
            ".." if parts.len() > root_parts && parts.last().is_some_and(|s| *s != "..") => {
                parts.pop();
            }
            ".." if absolute || drive => {}
            _ => parts.push(p),
        }
    }
    let s = parts.join("/");
    if unc {
        format!("//{s}")
    } else if absolute {
        format!("/{s}")
    } else if s.is_empty() {
        ".".into()
    } else {
        s
    }
}
fn record(fields: HashMap<String, String>) -> Result<Option<Record>> {
    let kind = first(&fields, &["type", "itemtype", "kind"]).to_lowercase();
    if matches!(kind.as_str(), "folder" | "directory" | "disk" | "volume")
        || first(&fields, &["isfolder", "isdirectory"]).eq_ignore_ascii_case("true")
        || first(&fields, &["attributes"])
            .parse::<i64>()
            .is_ok_and(|v| v & 16 != 0)
    {
        return Ok(None);
    }
    let name = first(&fields, &["filename", "name"]).replace('\\', "/");
    let mut full =
        first(&fields, &["fullpath", "filepath", "path", "catalogpath"]).replace('\\', "/");
    if (name.is_empty() && full.is_empty())
        || name.ends_with('/')
        || (full.ends_with('/') && name.is_empty())
    {
        return Ok(None);
    }
    if full.is_empty() {
        full = name.clone();
    } else if !name.is_empty() && full.rsplit('/').next() != name.rsplit('/').next() {
        full = format!("{}/{}", full.trim_end_matches('/'), name);
    }
    if full.contains('\0') || full.len() > 32768 {
        bail!("Invalid file path in export");
    }
    let full = clean_path(&full);
    let name = full.rsplit('/').next().unwrap_or("").to_owned();
    if full == "." || name == ".." || name.is_empty() {
        return Ok(None);
    }
    let size = first(
        &fields,
        &["sizebytes", "filesizebytes", "size", "filesize", "length"],
    );
    let size = if size.is_empty() {
        0
    } else {
        size.replace([' ', '\u{a0}'], "")
            .parse::<i64>()
            .map_err(|_| anyhow::anyhow!("Export exact byte sizes without separators or units"))?
    };
    if size < 0 {
        bail!("Invalid negative file size");
    }
    let (modified, modified_ns) = date(first(
        &fields,
        &[
            "datemodified",
            "modified",
            "modificationdate",
            "lastmodified",
            "modifiedtime",
        ],
    ));
    let mut media = first(
        &fields,
        &[
            "disk",
            "disc",
            "medianame",
            "medium",
            "volumelabel",
            "volume",
            "diskname",
            "source",
        ],
    )
    .to_owned();
    let serial = first(&fields, &["serialnumber", "disknumber"]);
    if !serial.is_empty() {
        if !media.is_empty() {
            media.push_str(" · ");
        }
        media.push_str(serial);
    }
    let location = first(&fields, &["physicallocation", "location"]).to_owned();
    if media.len() > 4096 || location.len() > 4096 {
        bail!("Media label or location exceeds 4096-byte limit");
    }
    let dir = full
        .rsplit_once('/')
        .map(|(d, _)| if d.is_empty() { "/" } else { d })
        .unwrap_or("")
        .to_owned();
    let extension = Path::new(&name)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    Ok(Some(Record {
        file: ScannedFile {
            relative: full,
            name,
            dir,
            extension,
            size,
            modified,
            modified_ns,
        },
        media,
        location,
    }))
}
fn date(value: &str) -> (i64, i64) {
    if let Ok(n) = value.parse::<i64>() {
        let ns = if n > 100_000_000_000_000_000 {
            (n as i128 - 116_444_736_000_000_000) * 100
        } else if n > 1_000_000_000_000 {
            n as i128 * 1_000_000
        } else {
            n as i128 * 1_000_000_000
        };
        return i64::try_from(ns)
            .map(|ns| (ns.div_euclid(1_000_000_000), ns))
            .unwrap_or((0, 0));
    }
    if let Ok(d) = chrono::DateTime::parse_from_rfc3339(value) {
        return (d.timestamp(), d.timestamp_nanos_opt().unwrap_or(0));
    }
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%d.%m.%Y %H:%M:%S",
        "%m/%d/%Y %H:%M:%S",
        "%m/%d/%Y %I:%M:%S %p",
    ] {
        if let Ok(d) = chrono::NaiveDateTime::parse_from_str(value, format) {
            let d = d.and_utc();
            return (d.timestamp(), d.timestamp_nanos_opt().unwrap_or(0));
        }
    }
    for format in ["%Y-%m-%d", "%d.%m.%Y"] {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(value, format) {
            let d = d.and_hms_opt(0, 0, 0).unwrap().and_utc();
            return (d.timestamp(), d.timestamp_nanos_opt().unwrap_or(0));
        }
    }
    (0, 0)
}
struct TokenLimit<R> {
    reader: R,
    read: u64,
    limit: Rc<Cell<u64>>,
}
impl<R: Read> Read for TokenLimit<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let allowed = self.limit.get().saturating_sub(self.read) as usize;
        if allowed == 0 {
            return Err(io::Error::other("XML token exceeds 4 MiB limit"));
        }
        let max = out.len().min(allowed);
        let n = self.reader.read(&mut out[..max])?;
        self.read += n as u64;
        Ok(n)
    }
}
#[derive(Default)]
struct Frame {
    tag: String,
    fields: HashMap<String, String>,
    text: String,
    media: String,
    serial: String,
    location: String,
    folder: String,
    inherited_folder: String,
    context_used: bool,
    children: bool,
}
impl Frame {
    fn refresh_context(&mut self) -> Result<()> {
        let previous = (
            self.media.clone(),
            self.serial.clone(),
            self.location.clone(),
            self.folder.clone(),
        );
        if matches!(self.tag.as_str(), "disk" | "disc" | "volume" | "medium") {
            let media = first(&self.fields, &["name", "label", "volumelabel"]);
            if !media.is_empty() {
                self.media = media.into();
            }
            let serial = first(&self.fields, &["serialnumber", "disknumber"]);
            if !serial.is_empty() {
                self.serial = serial.into();
            }
            let location = first(&self.fields, &["physicallocation", "location"]);
            if !location.is_empty() {
                self.location = location.into();
            }
        }
        if matches!(self.tag.as_str(), "folder" | "directory") {
            let folder =
                first(&self.fields, &["fullpath", "filepath", "path", "name"]).replace('\\', "/");
            self.folder = if folder.is_empty() {
                self.inherited_folder.clone()
            } else if self.inherited_folder.is_empty()
                || folder.starts_with('/')
                || folder.as_bytes().get(1) == Some(&b':')
            {
                clean_path(&folder)
            } else {
                clean_path(&format!("{}/{}", self.inherited_folder, folder))
            };
            self.fields.insert("type".into(), "folder".into());
        }
        if self.folder.len() > 32768
            || self.media.len() > 4096
            || self.serial.len() > 4096
            || self.location.len() > 4096
        {
            bail!("XML folder path, media label or location exceeds its size limit");
        }
        if self.context_used
            && previous
                != (
                    self.media.clone(),
                    self.serial.clone(),
                    self.location.clone(),
                    self.folder.clone(),
                )
        {
            bail!(
                "XML folder and media metadata must precede their file records; export attributes or move these fields before the files"
            );
        }
        Ok(())
    }
}
fn xml(
    reader: impl Read,
    cancel: &AtomicBool,
    emit: &mut dyn FnMut(Record) -> Result<()>,
) -> Result<()> {
    let limit = Rc::new(Cell::new((4 << 20) + 8192));
    let bounded = TokenLimit {
        reader,
        read: 0,
        limit: limit.clone(),
    };
    let mut reader = Reader::from_reader(BufReader::new(bounded));
    reader.config_mut().expand_empty_elements = true;
    let mut buffer = Vec::new();
    let mut stack: Vec<Frame> = Vec::new();
    let mut count = 0;
    loop {
        scanner::check(cancel)?;
        buffer.clear();
        limit.set(reader.buffer_position() + (4 << 20) + 8192);
        match reader.read_event_into(&mut buffer)? {
            Event::Start(e) => {
                if stack.len() >= 256 || e.len() > 4 << 20 {
                    bail!("XML nesting or token limit exceeded");
                }
                let mut frame = Frame {
                    tag: normalize(std::str::from_utf8(e.local_name().as_ref())?),
                    ..Default::default()
                };
                if matches!(frame.tag.as_str(), "file" | "item" | "record" | "entry") {
                    for ancestor in &mut stack {
                        if matches!(ancestor.tag.as_str(), "folder" | "directory")
                            && first(&ancestor.fields, &["fullpath", "filepath", "path", "name"])
                                .is_empty()
                        {
                            bail!("XML folder must include its name or path before file records");
                        }
                        // Streaming cannot revise records already emitted. Late
                        // context changes are rejected during the preflight pass.
                        ancestor.context_used = true;
                    }
                }
                if let Some(parent) = stack.last_mut() {
                    parent.children = true;
                    frame.media = parent.media.clone();
                    frame.serial = parent.serial.clone();
                    frame.location = parent.location.clone();
                    frame.folder = parent.folder.clone();
                    frame.inherited_folder = parent.folder.clone();
                }
                for (attribute_index, attr) in e.attributes().enumerate() {
                    scanner::check(cancel)?;
                    // Count raw attributes: distinct namespace prefixes may
                    // normalize to the same field key.
                    if attribute_index >= 256 {
                        bail!("XML attribute count exceeds limit");
                    }
                    let a = attr?;
                    // Keep literal whitespace in catalog paths while decoding
                    // entities; XML attribute normalization would change it.
                    let value =
                        quick_xml::escape::unescape(std::str::from_utf8(&a.value)?)?.into_owned();
                    if value.len() > 1 << 20 {
                        bail!("XML field exceeds 1 MiB limit");
                    }
                    frame.fields.insert(
                        normalize(std::str::from_utf8(a.key.local_name().as_ref())?),
                        value,
                    );
                }
                frame.refresh_context()?;
                stack.push(frame);
            }
            Event::Text(e) => {
                if let Some(frame) = stack.last_mut() {
                    let text = quick_xml::escape::unescape(&e.decode()?)?.into_owned();
                    if frame.text.len() + text.len() > 1 << 20 {
                        bail!("XML field exceeds 1 MiB limit");
                    }
                    frame.text.push_str(&text);
                }
            }
            Event::GeneralRef(e) => {
                if let Some(frame) = stack.last_mut() {
                    let text = crate::extract::reference(&e)?;
                    if frame.text.len() + text.len() > 1 << 20 {
                        bail!("XML field exceeds 1 MiB limit");
                    }
                    frame.text.push_str(&text);
                }
            }
            Event::CData(e) => {
                if let Some(frame) = stack.last_mut() {
                    let text = e.decode()?;
                    if frame.text.len() + text.len() > 1 << 20 {
                        bail!("XML field exceeds 1 MiB limit");
                    }
                    frame.text.push_str(&text);
                }
            }
            Event::End(_) => {
                let mut frame = stack
                    .pop()
                    .ok_or_else(|| anyhow::anyhow!("Invalid XML stack"))?;
                if matches!(frame.tag.as_str(), "file" | "item" | "record" | "entry") {
                    if first(
                        &frame.fields,
                        &[
                            "disk",
                            "disc",
                            "medianame",
                            "medium",
                            "volumelabel",
                            "volume",
                            "diskname",
                            "source",
                        ],
                    )
                    .is_empty()
                    {
                        frame.fields.insert("disk".into(), frame.media);
                    }
                    if first(&frame.fields, &["serialnumber", "disknumber"]).is_empty()
                        && !frame.serial.is_empty()
                    {
                        frame.fields.insert("serialnumber".into(), frame.serial);
                    }
                    if first(&frame.fields, &["location", "physicallocation"]).is_empty() {
                        frame.fields.insert("location".into(), frame.location);
                    }
                    if first(
                        &frame.fields,
                        &["path", "filepath", "fullpath", "catalogpath"],
                    )
                    .is_empty()
                        && !frame.folder.is_empty()
                    {
                        let name = first(&frame.fields, &["filename", "name"]).replace('\\', "/");
                        if !name.is_empty() {
                            // This is a known directory, unlike ambiguous CSV
                            // Path fields. Always join its relative filename,
                            // even when the directory has the same basename.
                            let full =
                                if name.starts_with('/') || name.as_bytes().get(1) == Some(&b':') {
                                    name
                                } else {
                                    format!("{}/{}", frame.folder.trim_end_matches('/'), name)
                                };
                            frame.fields.insert("fullpath".into(), full);
                        }
                    }
                    if let Some(record) = record(frame.fields)? {
                        emit(record)?;
                        count += 1;
                    }
                } else if !frame.children
                    && let Some(parent) = stack.last_mut()
                {
                    let value = if frame.text.trim().is_empty() {
                        first(&frame.fields, &["value"]).to_owned()
                    } else {
                        frame.text.trim().to_owned()
                    };
                    let key = if matches!(frame.tag.as_str(), "field" | "column") {
                        normalize(first(&frame.fields, &["name", "key"]))
                    } else {
                        frame.tag
                    };
                    if !key.is_empty() {
                        if !parent.fields.contains_key(&key) && parent.fields.len() >= 256 {
                            bail!("XML field count exceeds limit");
                        }
                        parent.fields.insert(key, value);
                        parent.refresh_context()?;
                    }
                }
            }
            Event::DocType(_) => bail!("XML DTD is not supported"),
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() || count == 0 {
        bail!("Unsupported or incomplete XML report; export named file records or CSV");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn xml_records(input: &str) -> Result<Vec<Record>> {
        let mut records = Vec::new();
        xml(input.as_bytes(), &AtomicBool::new(false), &mut |record| {
            records.push(record);
            Ok(())
        })?;
        Ok(records)
    }

    #[test]
    fn xml_attribute_limit_counts_prefixed_names_before_normalization() -> Result<()> {
        for count in [255, 256, 10_000] {
            let attributes = (0..count)
                .map(|i| format!(" p{i}:ignored='x'"))
                .collect::<String>();
            let input = format!("<catalog><file name='safe.txt'{attributes}/></catalog>");
            let result = xml_records(&input);
            if count == 255 {
                let records = result?;
                assert_eq!(records.len(), 1);
                assert_eq!(records[0].file.relative, "safe.txt");
            } else {
                assert!(
                    result
                        .err()
                        .unwrap()
                        .to_string()
                        .contains("attribute count"),
                    "{count} prefixed attributes must not bypass the raw limit"
                );
            }
        }
        assert!(xml_records("<catalog><file name='one.txt' name='two.txt'/></catalog>").is_err());
        for path in [
            "Folder\tA/one &amp; two.txt",
            "Folder&#9;A/one &amp; two.txt",
        ] {
            let input = format!("<catalog><file fullpath='{path}'/></catalog>");
            assert_eq!(
                xml_records(&input)?[0].file.relative,
                "Folder\tA/one & two.txt"
            );
        }
        Ok(())
    }

    #[test]
    fn xml_nested_folder_elements_keep_distinct_paths_and_locations() -> Result<()> {
        for media_tag in ["disk", "disc", "volume", "medium"] {
            let input = format!(
                "<catalog><{media_tag}><label>Archive</label><physicallocation>Shelf A</physicallocation>\
                 <folder><name>Parent</name>\
                 <directory><field name='name'>Folder A</field><file name='one.txt' size='1'/></directory>\
                 <folder name='Folder B'><name>Folder B</name><file name='one.txt' size='2'/></folder>\
                 <folder><path>//server/share/Docs</path><file name='net.txt'/></folder>\
                 </folder></{media_tag}></catalog>"
            );
            let records = xml_records(&input)?;
            assert_eq!(records.len(), 3);
            assert_eq!(records[0].file.relative, "Parent/Folder A/one.txt");
            assert_eq!(records[1].file.relative, "Parent/Folder B/one.txt");
            assert_eq!(records[2].file.relative, "//server/share/Docs/net.txt");
            assert_eq!((records[0].file.size, records[1].file.size), (1, 2));
            for record in records {
                assert_eq!(record.media, "Archive");
                assert_eq!(record.location, "Shelf A");
            }
        }
        let explicit = xml_records(
            "<catalog><disk name='Inherited' location='Old shelf'><folder name='Inherited folder'><file name='remote.txt' catalogpath='D:/remote.txt' medianame='Explicit' location='New shelf'/></folder></disk></catalog>",
        )?;
        assert_eq!(explicit[0].file.relative, "D:/remote.txt");
        assert_eq!(explicit[0].media, "Explicit");
        assert_eq!(explicit[0].location, "New shelf");
        Ok(())
    }

    #[test]
    fn xml_inherited_directory_does_not_alias_a_same_named_file() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let store = Arc::new(Mutex::new(store::Store::open(&data, "")?));
        let source = temp.path().join("catalog.xml");
        fs::write(
            &source,
            "<catalog><disk name='Archive'><folder name='report.txt'><file name='report.txt' size='1'/></folder><file name='report.txt' size='2'/><folder name='Other'><file name='//server/share/report.txt' size='3'/></folder></disk></catalog>",
        )?;
        import(
            &store,
            ImportOptions {
                path: source.to_string_lossy().into_owned(),
                ..Default::default()
            },
            &AtomicBool::new(false),
            &|_| {},
        )?;
        let files = store
            .lock()
            .unwrap()
            .search(store::SearchOptions::default())?
            .results;
        assert_eq!(files.len(), 3);
        let paths = files
            .iter()
            .map(|file| (file.relative_path.as_str(), file.size))
            .collect::<HashMap<_, _>>();
        assert_eq!(paths["report.txt/report.txt"], 1);
        assert_eq!(paths["report.txt"], 2);
        assert_eq!(paths["//server/share/report.txt"], 3);

        // Keep the existing CSV convention: File Path may already be a full
        // filename; only the XML hierarchy tells us a path is a directory.
        let mut csv_files = Vec::new();
        csv(
            "Filename,File Path,Size\nreport.txt,report.txt,2\nreport.txt,Folder,3\n".as_bytes(),
            &AtomicBool::new(false),
            &mut |record| {
                csv_files.push(record.file.relative);
                Ok(())
            },
        )?;
        assert_eq!(csv_files, ["report.txt", "Folder/report.txt"]);
        Ok(())
    }

    #[test]
    fn xml_inherited_media_serials_keep_identically_named_disks_distinct() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let store = Arc::new(Mutex::new(store::Store::open(&data, "")?));
        let source = temp.path().join("catalog.xml");
        fs::write(
            &source,
            "<catalog><disk name='Backup' serialnumber='ONE'><folder name='Docs'><file name='report.txt' size='1'/><file name='report.txt' serialnumber='THREE' size='3'/></folder></disk><disk><name>Backup</name><disknumber>TWO</disknumber><folder name='Docs'><file name='report.txt' size='2'/></folder></disk></catalog>",
        )?;
        import(
            &store,
            ImportOptions {
                path: source.to_string_lossy().into_owned(),
                ..Default::default()
            },
            &AtomicBool::new(false),
            &|_| {},
        )?;
        let files = store
            .lock()
            .unwrap()
            .search(store::SearchOptions::default())?
            .results;
        assert_eq!(files.len(), 3);
        assert!(
            files
                .iter()
                .all(|file| file.relative_path == "Docs/report.txt")
        );
        let media = files
            .iter()
            .map(|file| (file.source_label.as_str(), file.size))
            .collect::<HashMap<_, _>>();
        assert_eq!(media["Backup · ONE"], 1);
        assert_eq!(media["Backup · TWO"], 2);
        assert_eq!(media["Backup · THREE"], 3);
        assert_eq!(store.lock().unwrap().stats()?["sources"], 3);
        Ok(())
    }

    #[test]
    fn xml_rejects_late_or_unsupported_folder_metadata_before_import_writes() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let store = Arc::new(Mutex::new(store::Store::open(&data, "")?));
        let source = temp.path().join("catalog.xml");
        let options = ImportOptions {
            path: source.to_string_lossy().into_owned(),
            ..Default::default()
        };
        fs::write(
            &source,
            "<catalog><disk name='Archive'><folder name='Original'><file name='keep.txt' size='5'/></folder></disk></catalog>",
        )?;
        import(&store, options.clone(), &AtomicBool::new(false), &|_| {})?;
        let before = store
            .lock()
            .unwrap()
            .search(store::SearchOptions::default())?
            .results;
        for input in [
            "<catalog><disk name='Archive'><folder><file name='lost.txt'/><name>Late</name></folder></disk></catalog>",
            "<catalog><disk name='Archive'><folder name='Early'><file name='lost.txt'/><name>Late</name></folder></disk></catalog>",
            "<catalog><disk name='Archive'><file name='lost.txt'/><location>Late shelf</location></disk></catalog>",
            "<catalog><disk><file name='lost.txt'/><name>Late archive</name></disk></catalog>",
            "<catalog><disk name='Archive'><file name='lost.txt'/><serialnumber>Late serial</serialnumber></disk></catalog>",
            "<catalog><folder><details><name>Unsupported</name></details><file name='lost.txt'/></folder></catalog>",
        ] {
            fs::write(&source, input)?;
            assert!(
                import(&store, options.clone(), &AtomicBool::new(false), &|_| {}).is_err(),
                "{input}"
            );
            let after = store
                .lock()
                .unwrap()
                .search(store::SearchOptions::default())?
                .results;
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].id, before[0].id);
            assert_eq!(after[0].relative_path, "Original/keep.txt");
            assert_eq!(store.lock().unwrap().stats()?["sources"], 1);
        }
        // Repeated metadata is harmless when it cannot revise any emitted path.
        let records = xml_records(
            "<catalog><disk name='Archive'><folder name='Docs'><file name='one.txt'/><name>Docs</name></folder><name>Archive</name></disk></catalog>",
        )?;
        assert_eq!(records[0].file.relative, "Docs/one.txt");
        Ok(())
    }

    #[test]
    fn csv_padded_unicode_fields_quotes_and_multiline_are_preserved() -> Result<()> {
        struct SingleByte<'a>(&'a [u8]);
        impl Read for SingleByte<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                let limit = out.len().min(1);
                self.0.read(&mut out[..limit])
            }
        }
        let input = " \u{2003}\"Filename\", \"Size\", Disk\r\n   \"a,b \"\"quote\"\".txt\", 12, \t\"Disc\"\r\n\"line\nname.txt\",2,Disc\r\na\"b.txt,3,Disc\r\n Čaša.txt,4,Disc\r\n";
        let mut records = Vec::new();
        csv(
            SingleByte(input.as_bytes()),
            &AtomicBool::new(false),
            &mut |record| {
                records.push(record);
                Ok(())
            },
        )?;
        assert_eq!(records.len(), 4);
        assert_eq!(records[0].file.name, "a,b \"quote\".txt");
        assert_eq!(records[0].file.size, 12);
        assert_eq!(records[1].file.name, "line\nname.txt");
        assert_eq!(records[2].file.name, "a\"b.txt");
        assert_eq!(records[3].file.name, "Čaša.txt");
        assert!(records.iter().all(|record| record.media == "Disc"));
        let mut tsv = Vec::new();
        csv(
            "Filename\tSize\tDisk\n \"a,b.txt\"\t 1\t \"Disc\"\n".as_bytes(),
            &AtomicBool::new(false),
            &mut |record| {
                tsv.push(record);
                Ok(())
            },
        )?;
        assert_eq!(tsv[0].file.name, "a,b.txt");
        assert_eq!(tsv[0].media, "Disc");
        Ok(())
    }

    #[test]
    fn csv_bounds_count_discarded_padding_and_quoted_newlines() {
        for input in [
            format!("Filename,Size\n{}\"a.txt\",1\n", " ".repeat(4 << 20)),
            format!(
                "Filename,Size\n{}\"a.txt\",1\n",
                "\u{2003}".repeat((4 << 20) / 3 + 1)
            ),
            format!("Filename,Size\n \"{}\",1\n", "a\n".repeat((4 << 20) / 2)),
            format!("Filename,{}\n", "Column,".repeat(257)),
        ] {
            let result = csv(input.as_bytes(), &AtomicBool::new(false), &mut |_| Ok(()));
            assert!(result.is_err());
            assert!(result.unwrap_err().to_string().contains("limit"));
        }
    }

    #[test]
    fn imported_unc_and_drive_roots_survive_normalization() -> Result<()> {
        for (input, expected) in [
            ("//server/share/a/../file.txt", "//server/share/file.txt"),
            ("//server/share/../../file.txt", "//server/share/file.txt"),
            ("C:/a/../../file.txt", "C:/file.txt"),
            ("/a/../file.txt", "/file.txt"),
            ("///a/file.txt", "/a/file.txt"),
            ("../a/../file.txt", "../file.txt"),
        ] {
            assert_eq!(clean_path(input), expected);
        }
        let fields = HashMap::from([("filename".into(), "\\\\server\\share\\Čaša.txt".into())]);
        let record = record(fields)?.unwrap();
        assert_eq!(record.file.relative, "//server/share/Čaša.txt");
        assert_eq!(record.file.dir, "//server/share");
        Ok(())
    }

    #[test]
    fn overflowing_export_is_rejected_before_any_source_or_file_write() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let store = Arc::new(Mutex::new(store::Store::open(&data, "")?));
        let source = temp.path().join("catalog.csv");
        fs::write(
            &source,
            "Filename,Size\nfirst.bin,9223372036854775807\nsecond.bin,1\n",
        )?;
        let result = import(
            &store,
            ImportOptions {
                path: source.to_string_lossy().into_owned(),
                ..Default::default()
            },
            &AtomicBool::new(false),
            &|_| {},
        );
        assert!(result.unwrap_err().to_string().contains("64-bit limit"));
        let stats = store.lock().unwrap().stats()?;
        assert_eq!(stats["files"], 0);
        assert_eq!(stats["sources"], 0);
        assert_eq!(stats["bytes"], 0);
        Ok(())
    }

    #[test]
    fn interleaved_import_cancel_preserves_old_rows_and_retry_finishes_all_media() -> Result<()> {
        use std::sync::atomic::Ordering;
        let temp = tempfile::tempdir()?;
        let data = temp.path().join("data");
        fs::create_dir(&data)?;
        let store = Arc::new(Mutex::new(store::Store::open(&data, "")?));
        let source = temp.path().join("catalog.csv");
        let input = |prefix: &str| {
            let mut csv = String::from("Filename,File Path,Size,Disk\n");
            for i in 0..4200 {
                csv.push_str(&format!("{prefix}-{i}.bin,shared,1,Disc {}\n", i % 10));
            }
            csv
        };
        let options = ImportOptions {
            path: source.to_string_lossy().into_owned(),
            ..Default::default()
        };
        fs::write(&source, input("old"))?;
        import(&store, options.clone(), &AtomicBool::new(false), &|_| {})?;
        fs::write(&source, input("new"))?;
        let cancel = AtomicBool::new(false);
        let result = import(&store, options.clone(), &cancel, &|progress| {
            if progress.scanned >= 4096 {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        assert!(result.unwrap_err().to_string().contains("Cancelled"));
        // Cancelled work keeps the previous scan and the committed new batch.
        assert_eq!(store.lock().unwrap().stats()?["files"], 4200 + 4096);
        import(&store, options, &AtomicBool::new(false), &|_| {})?;
        let s = store.lock().unwrap();
        let stats = s.stats()?;
        assert_eq!(stats["files"], 4200);
        assert_eq!(stats["bytes"], 4200);
        let sources = s.all_sources()?;
        assert_eq!(sources.len(), 10);
        assert!(sources.iter().all(|source| source.file_count == 420));
        let old: i64 = s.conn.query_row(
            "SELECT COUNT(*) FROM files WHERE name LIKE 'old-%'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(old, 0);
        Ok(())
    }
}
