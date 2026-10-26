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
    parse(&path, cancel, &mut |_| {
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
            if group.entries.len() >= scanner::BATCH {
                flush(store, group, &mut pending, &mut state, cancel, report)?;
            }
            if pending >= 4096 {
                for group in groups.values_mut() {
                    flush(store, group, &mut pending, &mut state, cancel, report)?;
                }
            }
            Ok(())
        })?;
        for group in groups.values_mut() {
            flush(store, group, &mut pending, &mut state, cancel, report)?;
        }
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
    group: &mut Batch,
    pending: &mut usize,
    state: &mut Progress,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    if group.entries.is_empty() {
        return Ok(());
    }
    scanner::check(cancel)?;
    store
        .lock()
        .unwrap()
        .upsert(&group.source.id, &state.id, &group.entries)?;
    state.scanned += group.entries.len() as i64;
    *pending -= group.entries.len();
    group.entries = Vec::new();
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
struct CsvLimit<R> {
    reader: R,
    quoted: bool,
    size: usize,
    columns: usize,
    header: bool,
    separator: u8,
}
impl<R: Read> Read for CsvLimit<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let size = out.len().min(4096);
        let n = self.reader.read(&mut out[..size])?;
        for &b in &out[..n] {
            self.size += 1;
            if b == b'"' {
                self.quoted = !self.quoted;
            }
            if !self.quoted && b == self.separator {
                self.columns += 1;
            }
            if self.size > if self.header { 65536 } else { 4 << 20 } || self.columns > 256 {
                return Err(io::Error::other("CSV record or column limit exceeded"));
            }
            if !self.quoted && b == b'\n' {
                self.size = 0;
                self.columns = 1;
                self.header = false;
            }
        }
        Ok(n)
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
    let bounded = CsvLimit {
        reader: std::io::Cursor::new(prefix).chain(reader),
        quoted: false,
        size: 0,
        columns: 1,
        header: true,
        separator,
    };
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
    let mut parts = Vec::new();
    for p in value.split('/') {
        match p {
            "" | "." => {}
            ".." if parts.last().is_some_and(|s| *s != "..") => {
                parts.pop();
            }
            ".." if absolute => {}
            _ => parts.push(p),
        }
    }
    let s = parts.join("/");
    if absolute {
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
    location: String,
    folder: String,
    children: bool,
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
                if let Some(parent) = stack.last_mut() {
                    parent.children = true;
                    frame.media = parent.media.clone();
                    frame.location = parent.location.clone();
                    frame.folder = parent.folder.clone();
                }
                for attr in e.attributes() {
                    let a = attr?;
                    if frame.fields.len() >= 256 {
                        bail!("XML attribute count exceeds limit");
                    }
                    let value = a.unescape_value()?.into_owned();
                    if value.len() > 1 << 20 {
                        bail!("XML field exceeds 1 MiB limit");
                    }
                    frame.fields.insert(
                        normalize(std::str::from_utf8(a.key.local_name().as_ref())?),
                        value,
                    );
                }
                if matches!(frame.tag.as_str(), "disk" | "disc" | "volume" | "medium") {
                    let media = first(&frame.fields, &["name", "label", "volumelabel"]);
                    if !media.is_empty() {
                        frame.media = media.into();
                    }
                    frame.location = first(&frame.fields, &["physicallocation", "location"]).into();
                }
                if matches!(frame.tag.as_str(), "folder" | "directory") {
                    let folder = first(&frame.fields, &["path", "name"]).replace('\\', "/");
                    frame.folder = if frame.folder.is_empty()
                        || folder.starts_with('/')
                        || folder.as_bytes().get(1) == Some(&b':')
                    {
                        clean_path(&folder)
                    } else {
                        clean_path(&format!("{}/{}", frame.folder, folder))
                    };
                    frame.fields.insert("type".into(), "folder".into());
                }
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
                    if first(&frame.fields, &["disk", "disc", "volume", "volumelabel"]).is_empty() {
                        frame.fields.insert("disk".into(), frame.media);
                    }
                    if first(&frame.fields, &["location", "physicallocation"]).is_empty() {
                        frame.fields.insert("location".into(), frame.location);
                    }
                    if first(&frame.fields, &["path", "filepath", "fullpath"]).is_empty()
                        && !frame.folder.is_empty()
                    {
                        frame.fields.insert("path".into(), frame.folder);
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
                        if matches!(parent.tag.as_str(), "disk" | "volume")
                            && matches!(key.as_str(), "name" | "label")
                        {
                            parent.media = value.clone();
                        }
                        parent.fields.insert(key, value);
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
