use crate::store::{ScanOptions, ScannedFile, Store};
use anyhow::{Result, bail};
pub fn seed(s: &mut Store) -> Result<()> {
    let stats = s.stats()?;
    if stats["files"] != 0 || stats["sources"] != 0 {
        bail!("Demo requires an empty catalog");
    }
    let sources = [
        ("Kancelarija · Studio", "computer", "Radna stanica"),
        ("MacBook Air", "computer", "Lični računar"),
        ("Arhiva · SSD 02", "external", "Kancelarija / fioka 2"),
        ("Projekti 2024", "bluray", "Fascikla A / disk 014"),
        ("LTO · Arhiva 07", "lto", "Polica 2 / LTO-007"),
        ("Kućni računar", "computer", "Kućni studio"),
    ];
    let names = [
        "Villa Aurora — osnova.dwg",
        "Kuhinja — finalni render.jpg",
        "Predmer i predračun.xlsx",
        "Ugovor o projektovanju.pdf",
        "Moodboard — prirodni materijali.pdf",
        "Dnevna soba — varijanta B.max",
        "Prezentacija projekta.pptx",
        "Ponuda za nameštaj.docx",
        "Teksture — hrast.zip",
        "Detalji stolarije.dwg",
        "Portfolio — jesen.pdf",
        "Fotografije lokacije.jpg",
        "Plan rasvete.dwg",
        "Specifikacija materijala.docx",
        "Izveštaj sa gradilišta.pdf",
        "Biblioteka materijala.zip",
        "Render — terasa.png",
        "Skica koncepta.pdf",
        "Budžet projekta.xlsx",
        "Zapisnik sastanka.docx",
    ];
    for (si, (label, kind, location)) in sources.iter().enumerate() {
        let source = s.begin_scan(&ScanOptions {
            root: format!("demo://{si}"),
            label: (*label).into(),
            kind: (*kind).into(),
            location: (*location).into(),
            ..Default::default()
        })?;
        let mut entries = Vec::new();
        for (pi, project) in [
            "Villa Aurora",
            "Stan Dorćol",
            "Studio North",
            "Kuća na obali",
        ]
        .iter()
        .enumerate()
        {
            for (ni, name) in names.iter().enumerate() {
                let dir = format!("Projekti/{project}");
                let sec = chrono::Utc::now().timestamp()
                    - 3 * 86400
                    - ((si * 48 + pi * 6 + ni) * 3600) as i64;
                entries.push(ScannedFile {
                    relative: format!("{dir}/{name}"),
                    name: (*name).into(),
                    dir,
                    extension: name.rsplit('.').next().unwrap().into(),
                    size: (128000 + (si + 1) * (ni + 3) * 178492) as i64,
                    modified: sec,
                    modified_ns: sec * 1_000_000_000,
                });
            }
        }
        for (row, entry) in s.upsert(&source.id, "demo", &entries)?.iter().zip(&entries) {
            if matches!(entry.extension.as_str(), "pdf" | "docx" | "xlsx" | "pptx") {
                let text = format!(
                    "DEMO — izmišljeni dokument za prikaz aplikacije AllFiles.\n\n{}\nProjekat enterijera: Villa Aurora. Materijali: prirodni hrast, kamen i toplo belo osvetljenje. Kuhinja i dnevna soba čine povezanu celinu. Ponuda obuhvata projektovanje, izradu nameštaja i koordinaciju radova. Rok isporuke i konačni budžet biće potvrđeni na sastanku.\nOvaj tekst nije preuzet iz korisnikovih dokumenata.",
                    entry.name
                );
                s.record_content(row.id, &row.version, "indexed", &text, "")?;
            }
        }
        s.finish_scan(&source.id, "demo", true)?;
    }
    s.conn.execute(
        "INSERT OR REPLACE INTO meta(key,value) VALUES('demo','true')",
        [],
    )?;
    println!("Demo catalog: 480 invented files.");
    Ok(())
}
