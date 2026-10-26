use crate::{crypto, store::*};
use std::{fs, path::Path};
fn entry(name: &str) -> ScannedFile {
    ScannedFile {
        relative: name.into(),
        name: name.into(),
        dir: "".into(),
        extension: "txt".into(),
        size: 20,
        modified: 1700000000,
        modified_ns: 1700000000000000000,
    }
}
#[test]
fn pdf_text_empty_and_corrupt() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let path = temp.path().join("test.pdf");
    let cancel = std::sync::atomic::AtomicBool::new(false);
    for text in ["Archive project 2026", ""] {
        let stream = if text.is_empty() {
            String::new()
        } else {
            format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET")
        };
        let objects=["<< /Type /Catalog /Pages 2 0 R >>".to_owned(),"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>".into(),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),format!("<< /Length {} >>\nstream\n{stream}\nendstream",stream.len())];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, obj) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{obj}\nendobj\n", i + 1));
        }
        let xref = pdf.len();
        pdf.push_str("xref\n0 6\n0000000000 65535 f \n");
        for offset in offsets {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
        ));
        fs::write(&path, pdf)?;
        let result = crate::extract::extract(&path, &cancel);
        assert_eq!(
            result.status,
            if text.is_empty() {
                "needs_ocr"
            } else {
                "indexed"
            },
            "{result:?}"
        );
        assert!(result.text.contains(text));
    }
    fs::write(&path, "not a pdf")?;
    assert_eq!(crate::extract::extract(&path, &cancel).status, "error");
    Ok(())
}
#[test]
fn encrypted_rollback_and_rekey() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let source = temp.path().join("plain");
    fs::create_dir(&source)?;
    let mut s = Store::open(&source, "")?;
    let src = s.begin_scan(&ScanOptions {
        root: "demo://rollback".into(),
        ..Default::default()
    })?;
    let f = s.upsert(&src.id, "scan", &[entry("secret.txt")])?.remove(0);
    s.record_content(f.id, &f.version, "indexed", "Original secret", "")?;
    let protected = temp.path().join("protected");
    s.export(&protected, true, Some("  password with spaces  "))?;
    drop(s);
    let mut s = Store::open(&protected, "  password with spaces  ")?;
    s.ensure_content(false)?;
    s.conn.execute_batch("BEGIN;UPDATE files SET name='Uncommitted';UPDATE content.documents SET payload=x'00';ROLLBACK;")?;
    assert_eq!(s.file(f.id, 0, 100)?["text"], "Original secret");
    assert_eq!(s.metadata(f.id)?.name, "secret.txt");
    let new = temp.path().join("rekey");
    s.export(&new, true, Some("different long password"))?;
    assert!(Store::open(&new, "  password with spaces  ").is_err());
    let mut copy = Store::open(&new, "different long password")?;
    assert_eq!(copy.file(f.id, 0, 100)?["text"], "Original secret");
    assert!(s.export(&new, true, None).is_err());
    assert_eq!(copy.stats()?["files"], 1);
    Ok(())
}
#[test]
fn transactions_search_and_encrypted_exports() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    private_dir(&data)?;
    let mut store = Store::open(&data, "")?;
    let source = store.begin_scan(&ScanOptions {
        root: "demo://rust-test".into(),
        label: "Unicode čćšžđ".into(),
        ..Default::default()
    })?;
    let files = store.upsert(
        &source.id,
        "first",
        &[entry("Čarobni tekst.txt"), entry("Other.txt")],
    )?;
    for f in &files {
        store.record_content(f.id, &f.version, "indexed", "Kuća 🌊 garden timber", "")?;
    }
    store.finish_scan(&source.id, "first", true)?;
    assert_eq!(store.stats()?["files"], 2);
    assert_eq!(
        store
            .search(SearchOptions {
                query: "carobni".into(),
                ..Default::default()
            })?
            .total,
        1
    );
    assert_eq!(
        store
            .search(SearchOptions {
                query: "garden".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        2
    );
    assert_eq!(store.file(files[0].id, 5, 1)?["text"], "🌊");
    assert!(store.file(files[0].id, 0, 0)?.get("total_chars").is_none());
    let encrypted = temp.path().join("encrypted");
    store.export(&encrypted, true, Some("long test password"))?;
    assert_ne!(
        &fs::read(encrypted.join("catalog.sqlite"))?[..16],
        b"SQLite format 3\0"
    );
    assert!(Store::open(&encrypted, "incorrect password").is_err());
    let mut protected = Store::open(&encrypted, "long test password")?;
    assert_eq!(protected.stats()?["files"], 2);
    assert_eq!(
        protected
            .search(SearchOptions {
                query: "garden".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        2
    );
    let added = protected.upsert(&source.id, "second", &[entry("New.txt")])?;
    protected.record_content(
        added[0].id,
        &added[0].version,
        "indexed",
        "Rust encrypted update",
        "",
    )?;
    protected.finish_scan(&source.id, "second", false)?;
    assert_eq!(protected.stats()?["files"], 3);
    let catalog = temp.path().join("catalog-only");
    protected.export(&catalog, false, None)?;
    let mut catalog = Store::open(&catalog, "long test password")?;
    assert_eq!(catalog.stats()?["documents"], 0);
    assert_eq!(
        catalog
            .search(SearchOptions {
                query: "garden".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        0
    );
    drop(protected);
    let mut reopened = Store::open(&encrypted, "long test password")?;
    assert_eq!(
        reopened
            .search(SearchOptions {
                query: "encrypted".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        1
    );
    Ok(())
}
#[test]
#[ignore = "Set ALLFILES_GO_FIXTURE to a v0.2.0 encrypted synthetic catalog and run with --include-ignored"]
fn reads_and_updates_go_encryption_format() -> anyhow::Result<()> {
    let fixture = std::env::var_os("ALLFILES_GO_FIXTURE")
        .ok_or_else(|| anyhow::anyhow!("Set ALLFILES_GO_FIXTURE"))?;
    let temp = tempfile::tempdir()?;
    let fixture = Path::new(&fixture);
    for name in ["catalog.sqlite", "content.sqlite", "encryption.json"] {
        fs::copy(fixture.join(name), temp.path().join(name))?;
    }
    let mut store = Store::open(temp.path(), "cross-language-test-password")?;
    assert_eq!(store.stats()?["files"], 480);
    assert!(
        !store
            .search(SearchOptions {
                mode: "content".into(),
                ..Default::default()
            })?
            .results
            .is_empty()
    );
    let source = store.begin_scan(&ScanOptions {
        root: "demo://cross-language".into(),
        label: "Rust addition".into(),
        ..Default::default()
    })?;
    let files = store.upsert(&source.id, "scan", &[entry("compatibility.txt")])?;
    store.record_content(
        files[0].id,
        &files[0].version,
        "indexed",
        "Compatibility needle from Rust",
        "",
    )?;
    store.finish_scan(&source.id, "scan", true)?;
    assert_eq!(
        store
            .search(SearchOptions {
                query: "Compatibility".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        1
    );
    if let Some(output) = std::env::var_os("ALLFILES_RUST_FIXTURE") {
        store.export(Path::new(&output), true, None)?;
    }
    drop(store);
    assert_eq!(
        Store::open(temp.path(), "cross-language-test-password")?.stats()?["files"],
        481
    );
    Ok(())
}
#[test]
fn password_and_lock_guards() -> anyhow::Result<()> {
    assert!(crypto::validate_password("short").is_err());
    let temp = tempfile::tempdir()?;
    let _store = Store::open(temp.path(), "")?;
    assert!(Store::open(temp.path(), "").is_err());
    Ok(())
}
#[test]
fn scanner_refresh_and_partial_cleanup() -> anyhow::Result<()> {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    let root = temp.path().join("files");
    fs::create_dir(&data)?;
    fs::create_dir(&root)?;
    fs::write(root.join("a.txt"), "originalneedle")?;
    fs::write(root.join("b.txt"), "shared document")?;
    let s = Arc::new(Mutex::new(Store::open(&data, "")?));
    let cancel = AtomicBool::new(false);
    let o = ScanOptions {
        root: root.to_string_lossy().into_owned(),
        label: "Test".into(),
        index_content: true,
        ..Default::default()
    };
    crate::scanner::scan(&s, o.clone(), &cancel, &|_| {})?;
    assert_eq!(s.lock().unwrap().stats()?["documents"], 2);
    let ids = s
        .lock()
        .unwrap()
        .search(SearchOptions::default())?
        .results
        .into_iter()
        .map(|f| f.id)
        .collect::<Vec<_>>();
    crate::scanner::scan(&s, o.clone(), &cancel, &|_| {})?;
    assert_eq!(
        s.lock()
            .unwrap()
            .search(SearchOptions::default())?
            .results
            .into_iter()
            .map(|f| f.id)
            .collect::<Vec<_>>(),
        ids
    );
    fs::write(root.join("a.txt"), "replacementneedle longer")?;
    fs::remove_file(root.join("b.txt"))?;
    crate::scanner::scan(
        &s,
        ScanOptions {
            index_content: false,
            ..o.clone()
        },
        &cancel,
        &|_| {},
    )?;
    assert_eq!(s.lock().unwrap().stats()?["files"], 1);
    assert_eq!(
        s.lock()
            .unwrap()
            .search(SearchOptions {
                query: "originalneedle".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        0
    );
    crate::scanner::scan(&s, o.clone(), &cancel, &|_| {})?;
    assert_eq!(
        s.lock()
            .unwrap()
            .search(SearchOptions {
                query: "replacementneedle".into(),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
        1
    );
    cancel.store(true, Ordering::Relaxed);
    assert!(crate::scanner::scan(&s, o, &cancel, &|_| {}).is_err());
    assert_eq!(s.lock().unwrap().stats()?["files"], 1);
    Ok(())
}
fn office_file(
    dir: &Path,
    extension: &str,
    parts: &[(&str, &str)],
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write;
    let path = dir.join(format!("fixture.{extension}"));
    let mut zip = zip::ZipWriter::new(fs::File::create(&path)?);
    for (name, text) in parts {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())?;
        zip.write_all(text.as_bytes())?;
    }
    zip.finish()?;
    Ok(path)
}
#[test]
fn office_formats_entities_and_limits() -> anyhow::Result<()> {
    use std::sync::atomic::AtomicBool;
    let temp = tempfile::tempdir()?;
    let cancel = AtomicBool::new(false);
    let cases = [
        (
            "docx",
            vec![
                (
                    "word/document.xml",
                    "<w:document xmlns:w='word'><w:p><w:r><w:t>Kuća &amp; &#x1F30A; &lt;test&gt;</w:t></w:r></w:p></w:document>",
                ),
                (
                    "word/header1.xml",
                    "<w:p xmlns:w='w'><w:t>Header</w:t></w:p>",
                ),
            ],
            vec!["Kuća & 🌊 <test>", "Header"],
        ),
        (
            "pptx",
            vec![(
                "ppt/slides/slide1.xml",
                "<p:sld xmlns:p='p' xmlns:a='a'><a:p><a:t>Presentation</a:t></a:p></p:sld>",
            )],
            vec!["Presentation"],
        ),
        (
            "xlsx",
            vec![
                (
                    "xl/sharedStrings.xml",
                    "<sst><si><t>Troškovi &amp; plan</t></si><si><t>unusedtext</t></si></sst>",
                ),
                (
                    "xl/worksheets/sheet1.xml",
                    "<worksheet><row><c t='s'><v>0</v></c><c><v>42</v></c><c t='inlineStr'><is><t>Oak &amp; stone</t></is></c></row></worksheet>",
                ),
            ],
            vec!["Troškovi & plan", "42", "Oak & stone"],
        ),
        (
            "odt",
            vec![(
                "content.xml",
                "<office:document xmlns:office='o' xmlns:text='t'><text:p>Stan <text:span>Marković &amp; family</text:span></text:p></office:document>",
            )],
            vec!["Stan Marković & family"],
        ),
    ];
    for (ext, parts, wanted) in cases {
        let path = office_file(temp.path(), ext, &parts)?;
        let v = crate::extract::extract(&path, &cancel);
        assert_eq!(v.status, "indexed", "{v:?}");
        for text in wanted {
            assert!(v.text.contains(text), "Missing {text}: {v:?}");
        }
        assert!(!v.text.contains("unusedtext"));
    }
    let path = temp.path().join("large.txt");
    fs::write(&path, "č".repeat(MAX_TEXT / 2 + 10))?;
    let v = crate::extract::extract(&path, &cancel);
    assert_eq!(v.status, "truncated");
    assert!(v.text.len() <= MAX_TEXT);
    let path = office_file(
        temp.path(),
        "docx",
        &[(
            "word/document.xml",
            "<!DOCTYPE doc [<!ENTITY x SYSTEM 'file:///secret'>]><doc><t>&x;</t></doc>",
        )],
    )?;
    assert_eq!(crate::extract::extract(&path, &cancel).status, "error");
    Ok(())
}
#[test]
fn import_csv_xml_and_failed_preflight() -> anyhow::Result<()> {
    use std::sync::{Arc, Mutex, atomic::AtomicBool};
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    fs::create_dir(&data)?;
    let s = Arc::new(Mutex::new(Store::open(&data, "")?));
    let cancel = AtomicBool::new(false);
    let path = temp.path().join("catalog.csv");
    fs::write(
        &path,
        "Filename,Size,Disk,Location\nC:\\Projects\\Kuća.pdf,120,Disc 01,Shelf A\nC:\\Other\\Photo.jpg,45,Disc 02,Shelf B\n",
    )?;
    let options = crate::import::ImportOptions {
        path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    crate::import::import(&s, options.clone(), &cancel, &|_| {})?;
    assert_eq!(s.lock().unwrap().stats()?["files"], 2);
    assert_eq!(s.lock().unwrap().stats()?["sources"], 2);
    fs::write(
        &path,
        "Filename,Size,Disk\nC:\\valid.txt,12,Disc 01\nC:\\bad.txt,invalid,Disc 01\n",
    )?;
    assert!(crate::import::import(&s, options, &cancel, &|_| {}).is_err());
    assert_eq!(s.lock().unwrap().stats()?["files"], 2);
    let path = temp.path().join("catalog.xml");
    fs::write(
        &path,
        "<catalog><disk name='Archive'><folder name='Projects'><file><name>Plan &amp; &#x1F30A;.pdf</name><size>123</size></file></folder></disk></catalog>",
    )?;
    crate::import::import(
        &s,
        crate::import::ImportOptions {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        },
        &cancel,
        &|_| {},
    )?;
    let found = s.lock().unwrap().search(SearchOptions {
        query: "Plan".into(),
        ..Default::default()
    })?;
    assert_eq!(found.total, 1);
    assert_eq!(found.results[0].name, "Plan & 🌊.pdf");
    assert_eq!(found.results[0].relative_path, "Projects/Plan & 🌊.pdf");
    Ok(())
}
#[test]
fn paged_large_text_and_content_owner_guard() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://pages".into(),
        ..Default::default()
    })?;
    let f = s.upsert(&source.id, "scan", &[entry("big.txt")])?.remove(0);
    let text = "abc🌊č".repeat(100_000);
    s.record_content(f.id, &f.version, "indexed", &text, "")?;
    let page = s.file(f.id, 4094, 17)?;
    assert_eq!(
        page["text"],
        text.chars().skip(4094).take(17).collect::<String>()
    );
    assert_eq!(page["has_more"], true);
    assert!(page.get("total_chars").is_none());
    let page = s.file(f.id, 499_997, 20)?;
    assert_eq!(page["text"], "c🌊č");
    assert_eq!(page["total_chars"], 500_000);
    assert_eq!(page["has_more"], false);
    let other = tempfile::tempdir()?;
    let mut other = Store::open(other.path(), "")?;
    other.ensure_content(true)?;
    let other_path = other.dir.clone();
    drop(other);
    drop(s);
    fs::copy(
        other_path.join("content.sqlite"),
        temp.path().join("content.sqlite"),
    )?;
    let mut s = Store::open(temp.path(), "")?;
    assert!(s.ensure_content(false).is_err());
    Ok(())
}
#[test]
fn mcp_validation_and_open_boundary() -> anyhow::Result<()> {
    for url in [
        "http://localhost:8787",
        "http://127.0.0.1:8787/path",
        "https://127.0.0.1:8787",
        "http://127.0.0.1:8787?x=1",
        "http://127.0.0.1:8787@evil.test",
    ] {
        assert!(crate::mcp::validate_url(url).is_err());
    }
    assert!(crate::mcp::validate_url("http://127.0.0.1:8787").is_ok());
    let tools: serde_json::Value = serde_json::from_str(include_str!("mcp_tools.json"))?;
    let search = &tools[1];
    assert!(crate::mcp::validate(search, &serde_json::json!({"limit":51})).is_err());
    assert!(crate::mcp::validate(search, &serde_json::json!({"query":null})).is_err());
    assert!(crate::mcp::validate(search, &serde_json::json!({"Query":"text"})).is_err());
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("root");
    fs::create_dir(&root)?;
    fs::write(root.join("safe.txt"), "safe")?;
    fs::write(temp.path().join("outside.txt"), "outside")?;
    let mut f = FileResult {
        root: root.to_string_lossy().into_owned(),
        path: root.join("safe.txt").to_string_lossy().into_owned(),
        ..Default::default()
    };
    assert!(crate::server::validate_open(&f, false).is_ok());
    f.path = root.join("../outside.txt").to_string_lossy().into_owned();
    assert!(crate::server::validate_open(&f, false).is_err());
    Ok(())
}
