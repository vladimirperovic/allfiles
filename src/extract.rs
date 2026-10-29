use crate::store::MAX_TEXT;
use anyhow::{Result, bail};
use quick_xml::{Reader, events::Event};
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    fs::{self, File},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
const MAX_XML_PART: usize = 32 << 20;
const MAX_XML_TOTAL: u64 = 64 << 20;
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
    let end = sink.text.trim_end().len();
    sink.text.truncate(end);
    let start = sink.text.len() - sink.text.trim_start().len();
    sink.text.drain(..start);
    let text = sink.text;
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
        if f.size() > MAX_XML_PART as u64
            || f.size() > (MAX_XML_TOTAL - expanded)
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
    names.sort_by(|a, b| (a != "xl/sharedStrings.xml", a).cmp(&(b != "xl/sharedStrings.xml", b)));
    let mut shared = Vec::new();
    let mut decoded_expanded = 0;
    for name in names {
        check(cancel)?;
        let f = zip.by_name(&name)?;
        let mut bytes = Vec::new();
        f.take((MAX_XML_PART + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_XML_PART {
            bail!("Document XML limit exceeded");
        }
        let xml = xml_utf8(&bytes, cancel)?;
        decoded_expanded += xml.len() as u64;
        if decoded_expanded > MAX_XML_TOTAL {
            bail!("Document expanded XML limit exceeded");
        }
        if name == "xl/sharedStrings.xml" {
            shared = shared_strings(xml.as_bytes(), cancel)?;
        } else if ext == "xlsx" {
            sheet(xml.as_bytes(), &shared, sink, cancel)?;
        } else {
            document_xml(xml.as_bytes(), ext, sink, cancel)?;
        }
        sink.add("\n")?;
    }
    Ok(())
}
// quick-xml's byte reader expects UTF-8 here. Decode UTF-16 XML before looking
// for element names, and reject malformed input instead of reporting it empty.
fn xml_utf8<'a>(bytes: &'a [u8], cancel: &AtomicBool) -> Result<Cow<'a, str>> {
    check(cancel)?;
    if bytes.len() > MAX_XML_PART {
        bail!("Document XML limit exceeded");
    }
    let (body, little_endian) = if let Some(body) = bytes.strip_prefix(&[0xff, 0xfe]) {
        (body, true)
    } else if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
        (body, false)
    } else if bytes.starts_with(&[b'<', 0]) {
        (bytes, true)
    } else if bytes.starts_with(&[0, b'<']) {
        (bytes, false)
    } else {
        let text = std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))?;
        if text.contains('\0') {
            bail!("Invalid NUL character in document XML");
        }
        return Ok(Cow::Borrowed(text));
    };
    if body.len() % 2 != 0 {
        bail!("Incomplete UTF-16 code unit in document XML");
    }
    let units = body.as_chunks::<2>().0.iter().map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    let mut text = String::with_capacity(body.len().min(MAX_XML_PART));
    for (index, character) in char::decode_utf16(units).enumerate() {
        if index % 4096 == 0 {
            check(cancel)?;
        }
        let character = character?;
        if character == '\0' {
            bail!("Invalid NUL character in document XML");
        }
        if text.len() + character.len_utf8() > MAX_XML_PART {
            bail!("Document XML limit exceeded after decoding");
        }
        text.push(character);
    }
    Ok(Cow::Owned(text))
}
fn xstring_unit(bytes: &[u8]) -> Option<u16> {
    if bytes.len() < 7 || !bytes.starts_with(b"_x") || bytes[6] != b'_' {
        return None;
    }
    let mut unit = 0u16;
    for digit in &bytes[2..6] {
        unit = unit * 16 + (*digit as char).to_digit(16)? as u16;
    }
    Some(unit)
}
// Decode once per XML text element, after joining text/entity/CDATA events.
// In particular _x005F_x000D_ is the literal string _x000D_, not a CR.
fn xstring<'a>(text: &'a str, cancel: &AtomicBool) -> Result<Cow<'a, str>> {
    if !text.contains("_x") {
        return Ok(Cow::Borrowed(text));
    }
    let bytes = text.as_bytes();
    let mut result = String::with_capacity(text.len());
    let mut index = 0;
    let mut checked = 0;
    while index < bytes.len() {
        if index >= checked {
            check(cancel)?;
            checked = index + 4096;
        }
        if let Some(unit) = xstring_unit(&bytes[index..]) {
            index += 7;
            let character = if (0xd800..=0xdbff).contains(&unit) {
                let low = xstring_unit(&bytes[index..])
                    .filter(|low| (0xdc00..=0xdfff).contains(low))
                    .ok_or_else(|| {
                        anyhow::anyhow!("Invalid UTF-16 surrogate in spreadsheet string")
                    })?;
                index += 7;
                char::from_u32(0x10000 + (((unit as u32 - 0xd800) << 10) | (low as u32 - 0xdc00)))
            } else {
                char::from_u32(unit as u32)
            }
            .ok_or_else(|| anyhow::anyhow!("Invalid UTF-16 surrogate in spreadsheet string"))?;
            result.push(character);
        } else {
            let character = text[index..].chars().next().unwrap();
            result.push(character);
            index += character.len_utf8();
        }
    }
    Ok(Cow::Owned(result))
}
fn append_shared(current: &mut String, text: &str, total: &mut usize) -> Result<()> {
    if text.len() > MAX_TEXT.saturating_sub(*total) {
        bail!("Shared strings exceed text limit");
    }
    *total += text.len();
    current.push_str(text);
    Ok(())
}
fn push_shared(result: &mut Vec<String>, current: String) -> Result<()> {
    if result.len() >= 500_000 {
        bail!("Too many shared strings");
    }
    result.push(current);
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
    let mut raw_text = String::new();
    let mut text = false;
    let mut total = 0;
    loop {
        check(cancel)?;
        match reader.read_event()? {
            Event::Start(e) => match e.local_name().as_ref() {
                b"si" => current.clear(),
                b"t" => {
                    raw_text.clear();
                    text = true;
                }
                _ => {}
            },
            Event::Empty(e) if e.local_name().as_ref() == b"si" => {
                push_shared(&mut result, String::new())?;
            }
            Event::Text(e) if text => {
                append_shared(&mut raw_text, &decoded_text(&e)?, &mut total)?;
            }
            Event::GeneralRef(e) if text => {
                append_shared(&mut raw_text, &reference(&e)?, &mut total)?;
            }
            Event::CData(e) if text => {
                append_shared(&mut raw_text, &e.decode()?, &mut total)?;
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"t" => {
                    current.push_str(&xstring(&raw_text, cancel)?);
                    raw_text.clear();
                    text = false;
                }
                b"si" => {
                    push_shared(&mut result, std::mem::take(&mut current))?;
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
    let mut raw_text = String::new();
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
                    raw_text.clear();
                    in_value = false;
                    in_text = false;
                    for a in e.attributes() {
                        let a = a?;
                        if a.key.local_name().as_ref() == b"t" {
                            kind = a.unescape_value()?.into_owned();
                        }
                    }
                }
                b"v" => in_value = true,
                b"t" => {
                    raw_text.clear();
                    in_text = true;
                }
                _ => {}
            },
            Event::Text(e) if in_value || in_text => {
                let s = decoded_text(&e)?;
                let target = if in_value { &mut value } else { &mut raw_text };
                if inline.len() + target.len() + s.len() > MAX_TEXT {
                    bail!("Cell text exceeds limit");
                }
                target.push_str(&s);
            }
            Event::GeneralRef(e) if in_value || in_text => {
                let s = reference(&e)?;
                let target = if in_value { &mut value } else { &mut raw_text };
                if inline.len() + target.len() + s.len() > MAX_TEXT {
                    bail!("Cell text exceeds limit");
                }
                target.push_str(&s);
            }
            Event::CData(e) if in_value || in_text => {
                let s = e.decode()?;
                let target = if in_value { &mut value } else { &mut raw_text };
                if inline.len() + target.len() + s.len() > MAX_TEXT {
                    bail!("Cell text exceeds limit");
                }
                target.push_str(&s);
            }
            Event::End(e) => match e.local_name().as_ref() {
                b"v" => in_value = false,
                b"t" => {
                    inline.push_str(&xstring(&raw_text, cancel)?);
                    raw_text.clear();
                    in_text = false;
                }
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
                        sink.add(&xstring(&value, cancel)?)?;
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

#[cfg(test)]
mod regression_tests {
    use super::*;

    #[test]
    fn xlsx_empty_strings_cdata_and_escaped_characters() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let shared = shared_strings(
            b"<sst><si/><si><t/></si><si><t>A<![CDATA[&]]>B_x000<![CDATA[D_]]>C_x005F_x000D__xD83C__xDF0A_</t></si><si><r><t>_x005F_</t></r><r><t>x000D_</t></r></si></sst>",
            &cancel,
        )?;
        assert_eq!(shared, ["", "", "A&B\rC_x000D_🌊", "_x000D_"]);
        let mut sink = Sink::new();
        sheet(
            b"<worksheet><row><c t='s'><v>2</v></c><c t='inlineStr'><is><t>less<![CDATA[<]]>more_x000D_end_x005F_x000D__xD83C__xDF0A_</t></is></c><c t='str'><v><![CDATA[value]]>_x000D_end</v></c></row></worksheet>",
            &shared,
            &mut sink,
            &cancel,
        )?;
        assert_eq!(
            sink.text,
            "A&B\rC_x000D_🌊 less<more\rend_x000D_🌊 value\rend \n"
        );
        // Encoded underscores and adjacent rich-text runs must not be decoded twice.
        assert_eq!(
            xstring("_x005F_xD83C__x005F_xDF0A_", &cancel)?,
            "_xD83C__xDF0A_"
        );
        assert_eq!(xstring("č _xNOPE_ _x000d_", &cancel)?, "č _xNOPE_ \r");
        for malformed in ["_xD800_", "_xDC00_", "_xD800__x0041_"] {
            assert!(xstring(malformed, &cancel).is_err());
        }
        Ok(())
    }

    #[test]
    fn office_utf16_endianness_and_malformed_input() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let temp = tempfile::tempdir()?;
        let xml = "<?xml version='1.0' encoding='UTF-16'?><w:document xmlns:w='w'><w:p><w:t>Unicode čaša 🌊</w:t></w:p></w:document>";
        for little in [true, false] {
            let mut bytes = if little {
                vec![0xff, 0xfe]
            } else {
                vec![0xfe, 0xff]
            };
            for unit in xml.encode_utf16() {
                bytes.extend(if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
            let path = temp.path().join(format!("utf16-{little}.docx"));
            let mut zip = zip::ZipWriter::new(File::create(&path)?);
            zip.start_file(
                "word/document.xml",
                zip::write::SimpleFileOptions::default(),
            )?;
            zip.write_all(&bytes)?;
            zip.finish()?;
            let result = extract(&path, &cancel);
            assert_eq!(result.status, "indexed", "{result:?}");
            assert_eq!(result.text, "Unicode čaša 🌊");
        }
        for malformed in [
            &[0xff, 0xfe, 0x3c][..],
            &[0xfe, 0xff, 0xd8, 0x00][..],
            &[0xff, 0xfe, 0x00, 0xdc][..],
            b"<t>\xff</t>",
            b"<t>\0</t>",
        ] {
            assert!(xml_utf8(malformed, &cancel).is_err());
        }
        Ok(())
    }

    #[test]
    fn new_xml_paths_preserve_size_and_cancel_limits() -> Result<()> {
        let cancel = AtomicBool::new(false);
        let text = "x".repeat(MAX_TEXT + 1);
        assert!(
            shared_strings(
                format!("<sst><si><t><![CDATA[{text}]]></t></si></sst>").as_bytes(),
                &cancel
            )
            .is_err()
        );
        assert!(sheet(format!("<worksheet><row><c t='inlineStr'><is><t><![CDATA[{text}]]></t></is></c></row></worksheet>").as_bytes(), &[], &mut Sink::new(), &cancel).is_err());
        let mut full = vec![String::new(); 500_000];
        assert!(push_shared(&mut full, String::new()).is_err());
        drop(full);
        // UTF-16 can expand when converted to UTF-8; the decoded part is bounded too.
        let mut expanded = vec![0xff, 0xfe];
        for _ in 0..MAX_XML_PART / 3 + 1 {
            expanded.extend([0x2d, 0x4e]);
        }
        assert!(xml_utf8(&expanded, &cancel).is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(xml_utf8(b"<t>cancelled</t>", &cancel).is_err());
        assert!(shared_strings(b"<sst><si/></sst>", &cancel).is_err());
        assert!(sheet(b"<worksheet/>", &[], &mut Sink::new(), &cancel).is_err());
        assert!(xstring("_x000D_", &cancel).is_err());
        Ok(())
    }
}
