use crate::store::MAX_TEXT;
use anyhow::{Result, bail};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
pub fn candidate(extension: &str) -> bool {
    matches!(
        extension
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str(),
        "pdf"
            | "docx"
            | "xlsx"
            | "pptx"
            | "odt"
            | "ods"
            | "odp"
            | "txt"
            | "md"
            | "csv"
            | "tsv"
            | "log"
            | "json"
            | "xml"
            | "html"
            | "htm"
    )
}
#[derive(Serialize, Deserialize, Debug)]
pub struct Extracted {
    pub text: String,
    pub status: String,
    pub detail: String,
}
impl Extracted {
    pub fn error(detail: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            status: "error".into(),
            detail: detail.into(),
        }
    }
}
struct Sink {
    text: String,
    limited: bool,
}
impl Sink {
    fn new() -> Self {
        Self {
            text: String::new(),
            limited: false,
        }
    }
    fn add(&mut self, text: &str) -> Result<()> {
        let left = MAX_TEXT.saturating_sub(self.text.len());
        let mut end = text.len().min(left);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.text.push_str(&text[..end]);
        if end < text.len() {
            self.limited = true;
            bail!("text extraction limit reached");
        }
        Ok(())
    }
}
impl Write for Sink {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let text = std::str::from_utf8(bytes).map_err(std::io::Error::other)?;
        self.add(text)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Cancelled");
    }
    Ok(())
}
pub fn extract(path: &Path, cancel: &AtomicBool) -> Extracted {
    match std::panic::catch_unwind(|| extract_inner(path, cancel)) {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => Extracted::error(e.to_string()),
        Err(_) => Extracted::error("Document parser rejected malformed content"),
    }
}
fn extract_inner(path: &Path, cancel: &AtomicBool) -> Result<Extracted> {
    check(cancel)?;
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if !candidate(&ext) {
        return Ok(Extracted {
            text: String::new(),
            status: "unsupported".into(),
            detail: "Content extraction is not available for this file type.".into(),
        });
    }
    let before = fs::symlink_metadata(path)?;
    if !before.is_file() || before.file_type().is_symlink() {
        bail!("Content extraction only opens regular files, without following symbolic links.");
    }
    if before.len() > 256 << 20 {
        return Ok(Extracted {
            text: String::new(),
            status: "skipped".into(),
            detail: "Document exceeds the 256 MiB input limit; content was not extracted.".into(),
        });
    }
    let mut sink = Sink::new();
    let result = match ext.as_str() {
        "pdf" => pdf(path, &mut sink, cancel),
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" => office(path, &ext, &mut sink, cancel),
        _ => plain(path, &mut sink),
    };
    check(cancel)?;
    let after = fs::symlink_metadata(path)?;
    if before.len() != after.len()
        || before.modified()? != after.modified()?
        || !after.is_file()
        || after.file_type().is_symlink()
    {
        bail!("File changed during extraction; scan again to index its current contents.");
    }
    let text = sink.text.trim().to_owned();
    if sink.limited {
        return Ok(Extracted {
            text,
            status: "truncated".into(),
            detail:
                "Only a bounded portion was indexed (8 MiB text / 64 MiB expanded document limit)."
                    .into(),
        });
    }
    if let Err(e) = result {
        let detail = e.to_string();
        let lower = detail.to_lowercase();
        if ext == "pdf" && (lower.contains("encrypt") || lower.contains("password")) {
            return Ok(Extracted {
                text: String::new(),
                status: "encrypted".into(),
                detail: "Encrypted PDF could not be opened without a password.".into(),
            });
        }
        return Ok(Extracted::error(detail));
    }
    if text.is_empty() {
        return Ok(Extracted{text,status:if ext=="pdf"{"needs_ocr"}else{"empty"}.into(),detail:if ext=="pdf"{"No extractable text. This PDF may be image-only or use unsupported fonts; OCR is not included."}else{"Document contains no extractable text."}.into()});
    }
    Ok(Extracted {
        text,
        status: "indexed".into(),
        detail: String::new(),
    })
}
fn plain(path: &Path, sink: &mut Sink) -> Result<()> {
    let mut data = Vec::new();
    File::open(path)?
        .take((MAX_TEXT * 2 + 4) as u64)
        .read_to_end(&mut data)?;
    let oversized = data.len() > MAX_TEXT * 2;
    data.truncate(MAX_TEXT * 2);
    let text = if data.starts_with(&[0xff, 0xfe]) {
        encoding_rs::UTF_16LE
            .decode_without_bom_handling(&data[2..])
            .0
            .into_owned()
    } else if data.starts_with(&[0xfe, 0xff]) {
        encoding_rs::UTF_16BE
            .decode_without_bom_handling(&data[2..])
            .0
            .into_owned()
    } else {
        let data = data.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&data);
        if data.contains(&0) {
            bail!("Binary content was not indexed as text");
        }
        String::from_utf8_lossy(data).replace('\u{fffd}', " ")
    };
    sink.add(&text)?;
    if oversized {
        sink.limited = true;
    }
    Ok(())
}
fn pdf(path: &Path, sink: &mut Sink, cancel: &AtomicBool) -> Result<()> {
    let mut doc = pdf_extract::Document::load(path)?;
    if doc.is_encrypted() {
        doc.decrypt("")?;
    }
    let pages = doc.get_pages();
    if pages.len() > 20_000 {
        sink.limited = true;
    }
    for page in pages.keys().take(20_000) {
        check(cancel)?;
        let mut output = pdf_extract::PlainTextOutput::new(sink as &mut dyn Write);
        pdf_extract::output_doc_page(&doc, &mut output, *page)?;
        sink.add("\n")?;
    }
    Ok(())
}
fn part(name: &str, ext: &str) -> bool {
    match ext {
        "docx" => {
            matches!(
                name,
                "word/document.xml" | "word/footnotes.xml" | "word/endnotes.xml"
            ) || ((name.starts_with("word/header") || name.starts_with("word/footer"))
                && name.ends_with(".xml"))
        }
        "pptx" => {
            (name.starts_with("ppt/slides/slide") || name.starts_with("ppt/notesSlides/notesSlide"))
                && name.ends_with(".xml")
        }
        "xlsx" => {
            name == "xl/sharedStrings.xml"
                || (name.starts_with("xl/worksheets/sheet") && name.ends_with(".xml"))
        }
        _ => name == "content.xml",
    }
}
fn office(path: &Path, ext: &str, sink: &mut Sink, cancel: &AtomicBool) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(path)?)?;
    if zip.len() > 10_000 {
        bail!("Document archive has too many entries");
    }
    let mut names = Vec::new();
    let mut expanded = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index(i)?;
        if !part(f.name(), ext) {
            continue;
        }
        if f.size() > 32 << 20
            || f.size() > ((64u64 << 20) - expanded)
            || (f.compressed_size() > 0 && f.size() / f.compressed_size() > 1000)
        {
            bail!("Document expanded XML limit exceeded");
        }
        expanded += f.size();
        names.push(f.name().to_owned());
    }
    if names.is_empty() {
        bail!("Document archive has no recognized text parts");
    }
    names.sort_by_key(|n| (n != "xl/sharedStrings.xml", n.clone()));
    let mut shared = Vec::new();
    for name in names {
        check(cancel)?;
        let f = zip.by_name(&name)?;
        let mut bytes = Vec::new();
        f.take((32 << 20) + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 32 << 20 {
            bail!("Document XML limit exceeded");
        }
        if name == "xl/sharedStrings.xml" {
            shared = shared_strings(&bytes, cancel)?;
        } else if ext == "xlsx" {
            sheet(&bytes, &shared, sink, cancel)?;
        } else {
            document_xml(&bytes, ext, sink, cancel)?;
        }
        sink.add("\n")?;
    }
    Ok(())
}
fn decoded_text(event: &quick_xml::events::BytesText<'_>) -> Result<String> {
    Ok(quick_xml::escape::unescape(&event.decode()?)?.into_owned())
}
pub fn reference(event: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    Ok(quick_xml::escape::unescape(&format!("&{};", event.decode()?))?.into_owned())
}
fn document_xml(bytes: &[u8], ext: &str, sink: &mut Sink, cancel: &AtomicBool) -> Result<()> {
    let mut reader = Reader::from_reader(bytes);
    let mut text = 0u32;
    let mut paragraphs = 0u32;
    let odf = matches!(ext, "odt" | "ods" | "odp");
    loop {
        check(cancel)?;
        match reader.read_event()? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"p" | b"h" if odf => paragraphs += 1,
                b"t" if !odf => text += 1,
                b"tab" | b"br" | b"line-break" | b"s" => sink.add(" ")?,
                _ => {}
            },
            Event::Empty(e) => {
                if matches!(
                    e.local_name().as_ref(),
                    b"tab" | b"br" | b"line-break" | b"s"
                ) {
                    sink.add(" ")?;
                }
            }
            Event::Text(e) if text > 0 || paragraphs > 0 => sink.add(&decoded_text(&e)?)?,
            Event::GeneralRef(e) if text > 0 || paragraphs > 0 => sink.add(&reference(&e)?)?,
            Event::CData(e) if text > 0 || paragraphs > 0 => sink.add(&e.decode()?)?,
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => text = text.saturating_sub(1),
                b"p" | b"h" => {
                    paragraphs = paragraphs.saturating_sub(1);
                    sink.add("\n")?;
                }
                _ => {}
            },
            Event::DocType(_) => bail!("Document DTD is not supported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
fn shared_strings(bytes: &[u8], cancel: &AtomicBool) -> Result<Vec<String>> {
    let mut reader = Reader::from_reader(bytes);
    let mut result = Vec::new();
    let mut current = String::new();
    let mut text = false;
    let mut total = 0;
    loop {
        check(cancel)?;
        match reader.read_event()? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"si" => current.clear(),
                b"t" => text = true,
                _ => {}
            },
            Event::Text(e) if text => {
                let s = decoded_text(&e)?;
                total += s.len();
                if total > MAX_TEXT {
                    bail!("Shared strings exceed text limit");
                }
                current.push_str(&s);
            }
            Event::GeneralRef(e) if text => {
                let s = reference(&e)?;
                total += s.len();
                if total > MAX_TEXT {
                    bail!("Shared strings exceed text limit");
                }
                current.push_str(&s);
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => text = false,
                b"si" => {
                    result.push(std::mem::take(&mut current));
                    if result.len() > 500_000 {
                        bail!("Too many shared strings");
                    }
                }
                _ => {}
            },
            Event::DocType(_) => bail!("Document DTD is not supported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(result)
}
fn sheet(bytes: &[u8], shared: &[String], sink: &mut Sink, cancel: &AtomicBool) -> Result<()> {
    let mut reader = Reader::from_reader(bytes);
    let mut kind = String::new();
    let mut value = String::new();
    let mut inline = String::new();
    let mut in_value = false;
    let mut in_text = false;
    loop {
        check(cancel)?;
        match reader.read_event()? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"c" => {
                    kind.clear();
                    value.clear();
                    inline.clear();
                    for a in e.attributes() {
                        let a = a?;
                        if a.key.local_name().as_ref() == b"t" {
                            kind = a.unescape_value()?.into_owned();
                        }
                    }
                }
                b"v" => in_value = true,
                b"t" => in_text = true,
                _ => {}
            },
            Event::Text(e) if in_value || in_text => {
                let s = decoded_text(&e)?;
                let target = if in_value { &mut value } else { &mut inline };
                if target.len() + s.len() > MAX_TEXT {
                    bail!("Cell text exceeds limit");
                }
                target.push_str(&s);
            }
            Event::GeneralRef(e) if in_value || in_text => {
                let s = reference(&e)?;
                let target = if in_value { &mut value } else { &mut inline };
                if target.len() + s.len() > MAX_TEXT {
                    bail!("Cell text exceeds limit");
                }
                target.push_str(&s);
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"v" => in_value = false,
                b"t" => in_text = false,
                b"c" => {
                    if kind == "s" {
                        let index = value.parse::<usize>()?;
                        sink.add(
                            shared
                                .get(index)
                                .ok_or_else(|| anyhow::anyhow!("Invalid shared string index"))?,
                        )?;
                    } else if kind == "inlineStr" {
                        sink.add(&inline)?;
                    } else {
                        sink.add(&value)?;
                    }
                    sink.add(" ")?;
                }
                b"row" => sink.add("\n")?,
                _ => {}
            },
            Event::DocType(_) => bail!("Document DTD is not supported"),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}
