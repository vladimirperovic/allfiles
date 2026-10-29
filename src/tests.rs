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
    let other = Store::open(other.path(), "")?;
    other.ensure_content(true)?;
    let other_path = other.dir.clone();
    drop(other);
    drop(s);
    fs::copy(
        other_path.join("content.sqlite"),
        temp.path().join("content.sqlite"),
    )?;
    let s = Store::open(temp.path(), "")?;
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
#[test]
fn hybrid_search_paths_agree_with_plain_sql() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let src = s.begin_scan(&ScanOptions {
        root: "demo://hybrid".into(),
        ..Default::default()
    })?;
    let entries = (0..1500)
        .map(|i| {
            let name = format!("{} {i:04}.txt", if i % 3 == 0 { "Plan" } else { "Drawing" });
            let dir = if i % 500 == 0 {
                "Rare/Needle".to_owned()
            } else {
                format!("Dir {}", i % 7)
            };
            ScannedFile {
                relative: format!("{dir}/{name}"),
                dir,
                ..entry(&name)
            }
        })
        .collect::<Vec<_>>();
    s.upsert(&src.id, "scan", &entries)?;
    s.finish_scan(&src.id, "scan", true)?;
    // "drawing" exceeds the trigram threshold, "dr" is too short for trigrams,
    // "needle" and "0003" are selective; all must match an independent query.
    for query in ["drawing", "dr", "needle", "plan 0003", "dir draw", "zzz"] {
        let mut sql =
            "SELECT f.id FROM files f JOIN directories d ON d.id=f.dir_id WHERE 1=1".to_owned();
        let terms = query.split_whitespace().collect::<Vec<_>>();
        for _ in &terms {
            sql.push_str(" AND (f.search_name GLOB ? OR d.search_path GLOB ?)");
        }
        let args = terms
            .iter()
            .flat_map(|t| [format!("*{t}*"), format!("*{t}*")])
            .collect::<Vec<_>>();
        sql.push_str(" ORDER BY f.search_name,f.id");
        let expected = s
            .conn
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(&args), |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for offset in [0, 7, 990] {
            let found = s.search(SearchOptions {
                query: query.into(),
                offset,
                limit: 25,
                ..Default::default()
            })?;
            assert_eq!(found.total, expected.len() as i64, "{query}");
            let page = expected.iter().skip(offset as usize).take(25).copied();
            assert!(
                found.results.iter().map(|f| f.id).eq(page),
                "{query} @{offset}"
            );
        }
    }
    let blank = s.search(SearchOptions {
        offset: 1490,
        sort: "size".into(),
        ..Default::default()
    })?;
    assert_eq!((blank.total, blank.results.len()), (1500, 10));
    Ok(())
}
#[test]
fn snippets_ignore_diacritics_and_stop_early() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let src = s.begin_scan(&ScanOptions {
        root: "demo://snippets".into(),
        ..Default::default()
    })?;
    let f = s.upsert(&src.id, "scan", &[entry("kuca.txt")])?.remove(0);
    let text = format!(
        "{} Projekat: Velika KUĆA na obali. {}",
        "uvod ".repeat(40),
        "kraj ".repeat(200_000)
    );
    s.record_content(f.id, &f.version, "indexed", &text, "")?;
    let found = s.search(SearchOptions {
        query: "kuca".into(),
        mode: "content".into(),
        ..Default::default()
    })?;
    assert_eq!(found.total, 1);
    let snippet = &found.results[0].snippet;
    assert!(
        snippet.starts_with('…') && snippet.ends_with('…'),
        "{snippet}"
    );
    assert!(snippet.contains("Velika KUĆA na obali."), "{snippet}");
    let all = s.search(SearchOptions {
        mode: "content".into(),
        ..Default::default()
    })?;
    assert!(all.results[0].snippet.starts_with("uvod uvod"));
    for (name, folded) in [
        ("ČAROBNI Đak šuma Žena ć", "carobni djak suma zena c"),
        ("Plan A", "plan a"),
    ] {
        assert_eq!(fold(name), folded);
    }
    Ok(())
}

#[test]
fn catalog_size_overflow_rolls_back_and_keeps_integer_counters() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://size-boundary".into(),
        ..Default::default()
    })?;
    let mut largest = entry("largest.txt");
    largest.size = i64::MAX;
    let indexed = s
        .upsert(&source.id, "initial", &[largest.clone()])?
        .remove(0);
    // Updating content at the maximum total must not overflow transiently by
    // adding the new size before subtracting the old size.
    s.record_content(
        indexed.id,
        &indexed.version,
        "indexed",
        "boundary needle",
        "",
    )?;
    assert_eq!(s.stats()?["bytes"], i64::MAX);
    assert_eq!(s.stats()?["documents"], 1);
    let extra = entry("extra.txt");
    assert!(
        s.upsert(&source.id, "overflow", std::slice::from_ref(&extra))
            .is_err()
    );
    assert_eq!(s.stats()?["files"], 1);
    assert_eq!(s.stats()?["bytes"], i64::MAX);
    assert_eq!(s.file(indexed.id, 0, 100)?["text"], "boundary needle");
    assert_eq!(s.search(SearchOptions::default())?.total, 1);
    // An overflowing update must retain both the old metadata and its text.
    largest.size -= extra.size;
    s.upsert(&source.id, "shrink", &[largest.clone(), extra.clone()])?;
    assert_eq!(s.stats()?["bytes"], i64::MAX);
    largest.size = i64::MAX;
    assert!(
        s.upsert(&source.id, "overflow-update", &[largest.clone()])
            .is_err()
    );
    assert_eq!(s.metadata(indexed.id)?.size, i64::MAX - extra.size);
    assert_eq!(s.stats()?["bytes"], i64::MAX);
    // Roll back the whole batch, including a valid earlier decrease.
    largest.size = 10;
    let mut overflow = entry("overflow.txt");
    overflow.size = i64::MAX;
    assert!(
        s.upsert(&source.id, "batch-overflow", &[largest, overflow])
            .is_err()
    );
    assert_eq!(s.metadata(indexed.id)?.size, i64::MAX - extra.size);
    assert_eq!(s.stats()?["files"], 2);
    let kind: String = s
        .conn
        .query_row("SELECT typeof(bytes) FROM catalog_stats", [], |r| r.get(0))?;
    assert_eq!(kind, "integer");
    drop(s);
    let mut reopened = Store::open(temp.path(), "")?;
    assert_eq!(reopened.stats()?["bytes"], i64::MAX);
    let out = temp.path().join("copy");
    reopened.export(&out, true, None)?;
    assert_eq!(Store::open(&out, "")?.stats()?["bytes"], i64::MAX);
    Ok(())
}

#[test]
fn stats_upgrade_replaces_legacy_triggers_without_recounting_on_reopen() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://stats-upgrade".into(),
        ..Default::default()
    })?;
    let mut largest = entry("large.bin");
    largest.size = i64::MAX;
    s.upsert(&source.id, "initial", &[largest])?;
    // Simulate the trigger definitions in pre-fix Rust/Go catalogs.
    s.conn.execute_batch("DELETE FROM meta WHERE key='stats_schema';
        DROP TRIGGER stats_files_ai;
        CREATE TRIGGER stats_files_ai AFTER INSERT ON files BEGIN
        UPDATE catalog_stats SET files=files+1,bytes=bytes+new.size,documents=documents+(new.text_status IN('indexed','truncated')) WHERE id=1; END;
        DROP TRIGGER stats_files_au;
        CREATE TRIGGER stats_files_au AFTER UPDATE OF size,text_status ON files WHEN old.size!=new.size OR old.text_status!=new.text_status BEGIN
        UPDATE catalog_stats SET bytes=bytes+new.size-old.size,documents=documents+(new.text_status IN('indexed','truncated'))-(old.text_status IN('indexed','truncated')) WHERE id=1; END;")?;
    drop(s);
    let mut upgraded = Store::open(temp.path(), "")?;
    assert!(
        upgraded
            .upsert(&source.id, "overflow", &[entry("one.txt")])
            .is_err()
    );
    assert_eq!(upgraded.stats()?["files"], 1);
    let schema_version: i64 = upgraded
        .conn
        .query_row("PRAGMA schema_version", [], |r| r.get(0))?;
    drop(upgraded);
    let reopened = Store::open(temp.path(), "")?;
    assert_eq!(
        reopened
            .conn
            .query_row("PRAGMA schema_version", [], |r| r.get::<_, i64>(0))?,
        schema_version
    );
    assert_eq!(reopened.stats()?["bytes"], i64::MAX);
    Ok(())
}

#[test]
fn cached_stats_initialization_does_not_scan_files() -> anyhow::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut conn = rusqlite::Connection::open_in_memory()?;
    conn.execute_batch(
        "CREATE TABLE files(size INTEGER,text_status TEXT);CREATE TABLE sources(id TEXT);
        WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<100000)
        INSERT INTO files SELECT 1024,'indexed' FROM n;",
    )?;
    conn.execute_batch(include_str!("sql/stats.sql"))?;
    let calls = Arc::new(AtomicUsize::new(0));
    let progress = calls.clone();
    conn.progress_handler(
        1000,
        Some(move || {
            progress.fetch_add(1, Ordering::Relaxed);
            false
        }),
    )?;
    conn.execute_batch(include_str!("sql/stats.sql"))?;
    conn.progress_handler(0, None::<fn() -> bool>)?;
    assert!(
        calls.load(Ordering::Relaxed) < 10,
        "cached statistics must not scan 100k rows"
    );
    assert_eq!(
        conn.query_row("SELECT bytes FROM catalog_stats", [], |r| r
            .get::<_, i64>(0))?,
        102400000
    );
    // Rebuild missing counters, still with existing triggers installed.
    conn.execute("DELETE FROM catalog_stats", [])?;
    conn.execute_batch(include_str!("sql/stats.sql"))?;
    assert_eq!(
        conn.query_row("SELECT files FROM catalog_stats", [], |r| r
            .get::<_, i64>(0))?,
        100000
    );
    Ok(())
}

#[test]
fn search_unicode_limit_counts_characters() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    for text in ["ž", "🌊", "a"] {
        assert_eq!(
            s.search(SearchOptions {
                query: text.repeat(1000),
                ..Default::default()
            })?
            .total,
            0
        );
        assert!(
            s.search(SearchOptions {
                query: text.repeat(1001),
                ..Default::default()
            })
            .is_err()
        );
    }
    Ok(())
}

#[test]
fn storage_batch_groups_are_isolated_and_roll_back_together() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let a = s.begin_scan(&ScanOptions {
        root: "demo://a".into(),
        label: "Same label".into(),
        ..Default::default()
    })?;
    let b = s.begin_scan(&ScanOptions {
        root: "demo://b".into(),
        label: "Same label".into(),
        ..Default::default()
    })?;
    let file = entry("same.txt");
    let rows = s.upsert_groups(
        "first",
        &[
            (&a.id, std::slice::from_ref(&file)),
            (&b.id, std::slice::from_ref(&file)),
        ],
    )?;
    assert_eq!(rows.len(), 2);
    assert_ne!(rows[0].id, rows[1].id);
    assert_eq!(s.metadata(rows[0].id)?.source_id, a.id);
    assert_eq!(s.metadata(rows[1].id)?.source_id, b.id);
    s.record_content(
        rows[0].id,
        &rows[0].version,
        "indexed",
        "original content",
        "",
    )?;
    let mut changed = file.clone();
    changed.size = 11;
    let mut overflow = entry("too-big.txt");
    overflow.size = i64::MAX;
    assert!(
        s.upsert_groups("failed", &[(&a.id, &[changed]), (&b.id, &[overflow])])
            .is_err()
    );
    assert_eq!(s.metadata(rows[0].id)?.size, file.size);
    assert_eq!(s.stats()?["files"], 2);
    assert_eq!(s.stats()?["bytes"], 40);
    assert_eq!(s.stats()?["documents"], 1);
    assert_eq!(s.file(rows[0].id, 0, 100)?["text"], "original content");
    let (sources, total) = s.sources("", 0, 200)?;
    assert_eq!(total, 2);
    assert!(
        sources
            .iter()
            .all(|source| source.file_count == 1 && source.total_size == 20)
    );
    assert_eq!(s.sources(&b.id, 0, 1)?.0[0].file_count, 1);
    assert_eq!(s.sources("", 1, 1)?.0.len(), 1);
    Ok(())
}

#[test]
fn storage_availability_is_derived_without_destroying_saved_status() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let data = temp.path().join("data");
    fs::create_dir(&data)?;
    let mut s = Store::open(&data, "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://availability".into(),
        ..Default::default()
    })?;
    let entries = [entry("one.txt"), entry("two.txt")];
    let files = s.upsert(&source.id, "first", &entries)?;
    for file in &files {
        s.record_content(file.id, &file.version, "indexed", "restorable document", "")?;
    }
    assert_eq!(s.stats()?["documents"], 2);
    drop(s);
    let saved = temp.path().join("saved-content.sqlite");
    fs::rename(data.join("content.sqlite"), &saved)?;
    let mut s = Store::open(&data, "")?;
    let stats = s.stats()?;
    assert_eq!(stats["documents"], 0);
    assert_eq!(stats["stored_documents"], 2);
    assert_eq!(stats["content_available"], false);
    assert!(!stats["content_error"].as_str().unwrap().is_empty());
    assert!(
        s.search(SearchOptions::default())?
            .results
            .iter()
            .all(|f| f.text_status == "not_indexed")
    );
    assert_eq!(s.file(files[0].id, 0, 100)?["text_status"], "not_indexed");
    let stored: i64 = s.conn.query_row(
        "SELECT count(*) FROM files WHERE text_status='indexed'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(stored, 2);
    // Missing content has no open handle, so restoring the matching file makes
    // the saved statuses usable again without rescanning or rewriting them.
    fs::rename(&saved, data.join("content.sqlite"))?;
    assert_eq!(s.stats()?["documents"], 2);
    assert_eq!(s.file(files[0].id, 0, 100)?["text"], "restorable document");
    drop(s);
    fs::rename(data.join("content.sqlite"), &saved)?;
    let mut s = Store::open(&data, "")?;
    let first = s.upsert(&source.id, "partial", &entries[..1])?.remove(0);
    assert!(first.needs_text);
    s.record_content(first.id, &first.version, "indexed", "rebuilt document", "")?;
    assert_eq!(s.stats()?["documents"], 1);
    assert_eq!(s.stats()?["stored_documents"], 2);
    assert_eq!(
        s.search(SearchOptions {
            mode: "content".into(),
            ..Default::default()
        })?
        .total,
        1
    );
    s.conn.execute_batch("BEGIN; DELETE FROM content.links;")?;
    assert_eq!(s.stats()?["documents"], 0);
    s.conn.execute_batch("ROLLBACK")?;
    assert_eq!(s.stats()?["documents"], 1);
    let copied = temp.path().join("copy");
    s.export(&copied, true, None)?;
    let copy = Store::open(&copied, "")?;
    assert_eq!(copy.stats()?["documents"], 1);
    assert_eq!(copy.stats()?["stored_documents"], 2);
    Ok(())
}

#[test]
fn storage_availability_cancelled_initialization_can_retry() -> anyhow::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://availability-cancel".into(),
        ..Default::default()
    })?;
    let entries = (0..4000)
        .map(|i| entry(&format!("doc-{i}.txt")))
        .collect::<Vec<_>>();
    let files = s.upsert(&source.id, "scan", &entries)?;
    let contents = files
        .iter()
        .map(|f| Content {
            id: f.id,
            version: &f.version,
            status: "indexed",
            text: "same document",
            detail: "",
        })
        .collect::<Vec<_>>();
    s.record_contents(&contents)?;
    drop(s);
    let mut s = Store::open(temp.path(), "")?;
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    s.conn.progress_handler(
        100,
        Some(move || counter.fetch_add(1, Ordering::Relaxed) >= 30),
    )?;
    assert!(s.ensure_content(false).is_err());
    let attached: i64 = s.conn.query_row(
        "SELECT count(*) FROM pragma_database_list WHERE name='content'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(attached, 0);
    let temporary: i64 = s.conn.query_row(
        "SELECT count(*) FROM temp.sqlite_master WHERE name LIKE 'available_%'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(temporary, 0);
    assert!(s.ensure_content(false)?);
    assert_eq!(s.stats()?["documents"], 4000);
    let ticks = Arc::new(AtomicUsize::new(0));
    let counter = ticks.clone();
    s.conn.progress_handler(
        1000,
        Some(move || {
            counter.fetch_add(1, Ordering::Relaxed);
            false
        }),
    )?;
    for _ in 0..100 {
        assert_eq!(s.stats()?["documents"], 4000);
    }
    s.conn.progress_handler(0, None::<fn() -> bool>)?;
    assert!(
        ticks.load(Ordering::Relaxed) < 20,
        "status polls must use the derived O(1) count"
    );
    Ok(())
}

#[test]
fn storage_search_whitespace_dedup_and_fts_snippets() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://fts-normalization".into(),
        ..Default::default()
    })?;
    let files = s.upsert(
        &source.id,
        "scan",
        &[
            entry("alpha.txt"),
            entry("punctuation.txt"),
            entry("combining.txt"),
            entry("long-token.txt"),
        ],
    )?;
    let texts = [
        "alpha intermediate beta".to_owned(),
        format!("{}north west end", "intro ".repeat(15000)),
        format!("{}re\u{301}sume\u{301} end", " ".repeat(65533)),
        format!("{} longtailneedle end", "x".repeat(2_000_123)),
    ];
    for (file, text) in files.iter().zip(&texts) {
        s.record_content(file.id, &file.version, "indexed", text, "")?;
    }
    for separator in [" ", "\t", "\n", "\r", "\u{a0}", "\u{2003}"] {
        assert_eq!(
            s.search(SearchOptions {
                query: format!("alpha{separator}beta"),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
            1
        );
        assert_eq!(
            s.search(SearchOptions {
                query: format!("\"alpha{separator}beta\""),
                mode: "content".into(),
                ..Default::default()
            })?
            .total,
            0
        );
    }
    for mode in ["files", "content", "all"] {
        assert_eq!(
            s.search(SearchOptions {
                query: "alpha ".repeat(160),
                mode: mode.into(),
                ..Default::default()
            })?
            .total,
            1
        );
    }
    for (query, expected) in [
        ("north-west", "north west"),
        ("resume", "re\u{301}sume\u{301}"),
        ("longtailneedle", "longtailneedle"),
    ] {
        let found = s.search(SearchOptions {
            query: query.into(),
            mode: "content".into(),
            ..Default::default()
        })?;
        assert_eq!(found.total, 1);
        assert!(
            found.results[0].snippet.contains(expected),
            "{}",
            found.results[0].snippet
        );
    }
    Ok(())
}

#[test]
fn storage_rust_text_work_is_cancellable_and_name_only_preview_is_bounded() -> anyhow::Result<()> {
    use std::cell::Cell;
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://cancel-text".into(),
        ..Default::default()
    })?;
    let file = s
        .upsert(&source.id, "scan", &[entry("filenameonly.txt")])?
        .remove(0);
    let text = format!("{}tailneedle", "intro ".repeat(400_000));
    s.record_content(file.id, &file.version, "indexed", &text, "")?;
    let calls = Cell::new(0);
    let cancelled = || {
        calls.set(calls.get() + 1);
        calls.get() > 20
    };
    assert!(
        s.search_with_cancel(
            SearchOptions {
                query: "tailneedle".into(),
                mode: "content".into(),
                ..Default::default()
            },
            &cancelled
        )
        .is_err()
    );
    calls.set(0);
    let preview = s.search_with_cancel(
        SearchOptions {
            query: "filenameonly".into(),
            mode: "all".into(),
            ..Default::default()
        },
        &cancelled,
    )?;
    assert_eq!(preview.total, 1);
    assert!(preview.results[0].snippet.starts_with("intro"));
    assert!(calls.get() <= 20);
    calls.set(0);
    assert!(
        s.file_with_cancel(file.id, 2_000_000, 100, &cancelled)
            .is_err()
    );
    assert_eq!(s.file(file.id, 0, 5)?["text"], "intro");
    Ok(())
}

#[test]
fn storage_ascii_snippets_preserve_unicode61_boundaries() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let mut s = Store::open(temp.path(), "")?;
    let source = s.begin_scan(&ScanOptions {
        root: "demo://ascii-snippets".into(),
        ..Default::default()
    })?;
    let cases = [
        (
            "uppercase",
            format!("{}MiXeDcAsE final", "intro ".repeat(15000)),
            "mixedcase",
            "MiXeDcAsE",
        ),
        (
            "substring",
            format!(
                "{}target needle marker",
                "needleprefix suffixneedle ".repeat(5000)
            ),
            "needle",
            "target needle marker",
        ),
        (
            "punctuation",
            format!("{}before_ascii_marker", "intro ".repeat(15000)),
            "ascii-marker",
            "before_ascii_marker",
        ),
        (
            "boundary",
            format!("{}boundaryneedle end", " ".repeat(65530)),
            "boundaryneedle",
            "boundaryneedle",
        ),
        (
            "digits",
            format!("{}needle42 final", "needle42x xneedle42 ".repeat(5000)),
            "needle42",
            "needle42 final",
        ),
    ];
    for (name, text, query, expected) in cases {
        let file = s
            .upsert(&source.id, "scan", &[entry(&format!("{name}.txt"))])?
            .remove(0);
        s.record_content(file.id, &file.version, "indexed", &text, "")?;
        let result = s.search(SearchOptions {
            query: query.into(),
            mode: "content".into(),
            ..Default::default()
        })?;
        assert_eq!(result.total, 1, "{name}");
        assert!(
            result.results[0].snippet.contains(expected),
            "{name}: {}",
            result.results[0].snippet
        );
    }
    Ok(())
}
