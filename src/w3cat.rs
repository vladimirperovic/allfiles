//! Read-only reader for the native WinCatalog database (`.w3cat`).
//!
//! A `.w3cat` file is a plain SQLite database. The catalog is opened read-only
//! and `immutable`, so no journal or lock file is ever written next to it (it
//! often lives on a read-only or removable disk).
//!
//! Layout used here:
//! * `w3_volumes` lists the scanned media; each one is a `w3_items` row whose
//!   `itype` says what kind of medium it is (187 optical disc, 172 fixed disk).
//! * `w3_strict` is the parent/child tree; folders are `itype` 200.
//! * every real file has a `w3_fileInfo` row with its name, size and date.
//! * entries that live *inside* an archive (children of an `itype` 100 item)
//!   are skipped, because a scan with AllFiles never looks inside archives
//!   either and would otherwise inflate totals.
use crate::{
    details::{Audio, Exif, FileExtra, Thumbnail},
    import::Record,
    scanner,
};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Statement, types::ValueRef};
#[cfg(test)]
use std::fs;
use std::{collections::HashMap, path::Path, sync::atomic::AtomicBool};

const MAX_DEPTH: usize = 512;
/// `itype` values that denote a file with a `w3_fileInfo` row.
const FILE_TYPES: &str = "1, 2, 5, 6, 7, 100";
const ARCHIVE: i64 = 100;
const OPTICAL: i64 = 187;

pub fn is_w3cat(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("w3cat"))
}

struct Node {
    parent: i64,
    name: String,
    archive: bool,
}

pub fn read(
    path: &Path,
    cancel: &AtomicBool,
    details: bool,
    emit: &mut dyn FnMut(Record) -> Result<()>,
) -> Result<()> {
    let mut uri = url::Url::from_file_path(path)
        .map_err(|_| anyhow::anyhow!("Catalog path must be absolute"))?;
    uri.set_query(Some("mode=ro&immutable=1"));
    let db = Connection::open_with_flags(
        uri.as_str(),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .context("Cannot open the WinCatalog database")?;
    for table in ["w3_items", "w3_fileInfo", "w3_strict", "w3_volumes"] {
        let found: i64 = db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [table],
            |r| r.get(0),
        )?;
        if found == 0 {
            bail!("Not a WinCatalog database: table {table} is missing");
        }
    }

    // Media. Two media with the same catalog name stay distinct by serial.
    struct Volume {
        label: String,
        kind: &'static str,
    }
    let mut volumes: HashMap<i64, Volume> = HashMap::new();
    {
        let mut st = db.prepare(
            "SELECT i.id, i.name, i.itype, coalesce(CAST(v.serial AS TEXT), '')
             FROM w3_volumes w
             JOIN w3_items i ON i.id = w.id_volume
             LEFT JOIN w3_volumeInfo v ON v.id_item = i.id
             ORDER BY i.id",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut names: HashMap<&str, usize> = HashMap::new();
        for (_, name, _, _) in &rows {
            *names.entry(name.as_str()).or_default() += 1;
        }
        for (id, name, itype, serial) in &rows {
            let label = if names[name.as_str()] > 1 && !serial.is_empty() {
                format!("{name} · {serial}")
            } else if name.is_empty() {
                format!("WinCatalog {id}")
            } else {
                name.clone()
            };
            volumes.insert(
                *id,
                Volume {
                    label,
                    kind: if *itype == OPTICAL {
                        "bluray"
                    } else {
                        "external"
                    },
                },
            );
        }
    }
    if volumes.is_empty() {
        bail!("The WinCatalog database contains no scanned media");
    }

    // Folders (and archives, so that archive contents can be recognised).
    let mut folders: HashMap<i64, Node> = HashMap::new();
    {
        let mut st = db.prepare(
            "SELECT i.id, s.id_parent, i.name, i.itype
             FROM w3_items i JOIN w3_strict s ON s.id_item = i.id
             WHERE i.itype IN (200, 100) AND s.id_parent IS NOT NULL",
        )?;
        let mut rows = st.query([])?;
        while let Some(r) = rows.next()? {
            folders.insert(
                r.get(0)?,
                Node {
                    parent: r.get(1)?,
                    name: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    archive: r.get::<_, i64>(3)? == ARCHIVE,
                },
            );
        }
    }

    let mut extras = if details {
        Some(Extras::new(&db)?)
    } else {
        None
    };
    let mut st = db.prepare(&format!(
        "SELECT s.id_parent, f.name, f.size, f.date_change, i.id, i.rating, i.comments, f.crc32
         FROM w3_items i
         JOIN w3_fileInfo f ON f.id_item = i.id
         JOIN w3_strict s ON s.id_item = i.id
         WHERE i.itype IN ({FILE_TYPES}) AND f.name IS NOT NULL AND f.name <> ''
         ORDER BY i.id"
    ))?;
    let mut rows = st.query([])?;
    let mut seen = 0u64;
    let mut segments: Vec<&str> = Vec::new();
    while let Some(r) = rows.next()? {
        seen += 1;
        if seen.is_multiple_of(4096) {
            scanner::check(cancel)?;
        }
        // Walk up to the medium; anything inside an archive or detached from
        // a medium is not a file on that medium.
        segments.clear();
        let mut current: i64 = r.get(0)?;
        let mut volume = None;
        for _ in 0..MAX_DEPTH {
            if let Some(v) = volumes.get(&current) {
                volume = Some(v);
                break;
            }
            match folders.get(&current) {
                Some(node) if !node.archive => {
                    segments.push(node.name.as_str());
                    current = node.parent;
                }
                _ => break,
            }
        }
        let Some(volume) = volume else { continue };
        let name: String = r.get(1)?;
        let size = match r.get_ref(2)? {
            ValueRef::Integer(n) => n,
            ValueRef::Real(f) => f as i64,
            _ => 0,
        };
        let date = text(r.get_ref(3)?);
        let mut full = String::new();
        for segment in segments.iter().rev() {
            full.push_str(segment);
            full.push('/');
        }
        full.push_str(&name);
        let fields = HashMap::from([
            ("fullpath".to_owned(), full),
            ("size".to_owned(), size.max(0).to_string()),
            ("datemodified".to_owned(), iso_date(date)),
            ("disk".to_owned(), volume.label.clone()),
        ]);
        if let Some(mut record) = crate::import::record(fields)? {
            record.kind = volume.kind;
            if let Some(extras) = &mut extras {
                let extra = extras.load(
                    r.get(4)?,
                    r.get_ref(5)?,
                    text(r.get_ref(6)?),
                    text(r.get_ref(7)?),
                )?;
                if !extra.is_empty() {
                    record.file.extra = Some(Box::new(extra));
                }
            }
            emit(record)?;
        }
    }
    Ok(())
}

fn text(value: ValueRef<'_>) -> &str {
    match value {
        ValueRef::Text(t) => std::str::from_utf8(t).unwrap_or(""),
        _ => "",
    }
}
fn owned(value: ValueRef<'_>) -> String {
    match value {
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned(),
        ValueRef::Integer(n) => n.to_string(),
        ValueRef::Real(f) => f.to_string(),
        _ => String::new(),
    }
}
/// Drop NULs/control characters that old cameras leave in text fields.
fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_owned()
}
/// `3.5 (35/10)` -> `3.5`: WinCatalog appends the raw EXIF fraction.
fn plain(value: &str) -> String {
    clean(value.split(" (").next().unwrap_or(""))
}
fn number<T: std::str::FromStr>(value: &str) -> Option<T> {
    plain(value).parse().ok()
}
/// Comments are often HTML written by other tools or the camera's
/// `ImageDescription` padding. Keep the readable text only.
fn comment(value: &str) -> String {
    // Cameras pad the field with NULs.
    let value = value.replace(|c: char| c.is_control() && !c.is_whitespace(), " ");
    let mut out = String::with_capacity(value.len());
    let mut tag = false;
    for c in value.chars() {
        match c {
            '<' => tag = true,
            '>' if tag => {
                tag = false;
                out.push(' ');
            }
            _ if tag => {}
            c => out.push(c),
        }
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&");
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    out.trim_matches(|c: char| c == '/' || c.is_whitespace())
        .to_owned()
}
fn exposure(value: &str) -> String {
    match number::<f64>(value) {
        Some(v) if v > 0.0 && v < 1.0 => format!("1/{} s", (1.0 / v).round() as i64),
        Some(v) if v >= 1.0 => format!("{} s", (v * 10.0).round() / 10.0),
        _ => String::new(),
    }
}
fn flash(value: &str) -> String {
    match number::<i64>(value) {
        Some(code) if code & 1 == 1 => "fired".into(),
        Some(_) => "not_fired".into(),
        None => String::new(),
    }
}
/// `44; 49.52; 0 N` -> `44°49.52′ N`
fn coordinate(value: &str) -> String {
    let parts: Vec<&str> = value.split(';').map(str::trim).collect();
    let [degrees, minutes, rest] = parts[..] else {
        return String::new();
    };
    let mut rest = rest.split_whitespace();
    let (seconds, direction) = (rest.next().unwrap_or("0"), rest.next().unwrap_or(""));
    let seconds = if number::<f64>(seconds).is_some_and(|s| s != 0.0) {
        format!("{seconds}″")
    } else {
        String::new()
    };
    format!("{degrees}°{minutes}′{seconds} {direction}")
        .trim()
        .to_owned()
}

/// Per-file lookups for the optional WinCatalog tables. A table that a
/// particular WinCatalog version does not have is simply skipped.
struct Extras<'a> {
    tags: HashMap<i64, Vec<String>>,
    exif: Option<Statement<'a>>,
    audio: Option<Statement<'a>>,
    custom: Option<Statement<'a>>,
    picture: Option<Statement<'a>>,
}
impl<'a> Extras<'a> {
    fn new(db: &'a Connection) -> Result<Self> {
        let has = |table: &str| -> Result<bool> {
            Ok(db.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0),
            )? > 0)
        };
        let mut tags: HashMap<i64, Vec<String>> = HashMap::new();
        if has("w3_keyword_links")? {
            let mut st = db.prepare(
                "SELECT l.id_item, k.name FROM w3_keyword_links l
                 JOIN w3_items k ON k.id = l.id_keyword
                 WHERE k.name IS NOT NULL AND k.name <> '' ORDER BY k.name",
            )?;
            let mut rows = st.query([])?;
            while let Some(r) = rows.next()? {
                tags.entry(r.get(0)?).or_default().push(r.get(1)?);
            }
            // The source repeats some links; rows are ordered by name.
            tags.values_mut().for_each(|names| names.dedup());
        }
        Ok(Self {
            tags,
            exif: if has("w3_exifInfo")? {
                Some(db.prepare(
                    "SELECT camera_make, camera_model, width, height, date_taken, iso_equivalent,
                            exposure_time, aperture_number, focal_length, flash_used
                     FROM w3_exifInfo WHERE id_item = ?1",
                )?)
            } else {
                None
            },
            audio: if has("w3_id3Info")? {
                Some(db.prepare(
                    "SELECT artist, album, year, track, genre, title, duration, bitrate, channels, sample_rate
                     FROM w3_id3Info WHERE id_item = ?1",
                )?)
            } else {
                None
            },
            custom: if has("w3_customfieldInfo")? {
                Some(db.prepare("SELECT fields FROM w3_customfieldInfo WHERE id_item = ?1")?)
            } else {
                None
            },
            picture: if has("w3_picture")? {
                Some(db.prepare(
                    "SELECT data FROM w3_picture WHERE id_item = ?1 ORDER BY number LIMIT 1",
                )?)
            } else {
                None
            },
        })
    }

    fn load(
        &mut self,
        id: i64,
        rating: ValueRef<'_>,
        comments: &str,
        crc32: &str,
    ) -> Result<FileExtra> {
        let mut extra = FileExtra {
            crc32: crc32
                .trim()
                .chars()
                .filter(char::is_ascii_hexdigit)
                .collect::<String>()
                .to_lowercase(),
            rating: match rating {
                ValueRef::Integer(n) if n > 0 => Some(n),
                _ => None,
            },
            comment: comment(comments),
            tags: self.tags.remove(&id).unwrap_or_default(),
            ..Default::default()
        };
        if let Some(st) = &mut self.exif {
            extra.exif = st
                .query_row([id], |r| {
                    let make = clean(&owned(r.get_ref(0)?));
                    let model = clean(&owned(r.get_ref(1)?));
                    Ok(Exif {
                        camera: if model.starts_with(&make) || make.is_empty() {
                            model
                        } else {
                            format!("{make} {model}").trim().to_owned()
                        },
                        width: number(&owned(r.get_ref(2)?)).filter(|n| *n > 0),
                        height: number(&owned(r.get_ref(3)?)).filter(|n| *n > 0),
                        taken: clean(&owned(r.get_ref(4)?)),
                        iso: number::<i64>(&owned(r.get_ref(5)?))
                            .filter(|n| *n > 0)
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        exposure: exposure(&owned(r.get_ref(6)?)),
                        aperture: number::<f64>(&owned(r.get_ref(7)?))
                            .filter(|n| *n > 0.0)
                            .map(|n| format!("f/{n}"))
                            .unwrap_or_default(),
                        focal_length: number::<f64>(&owned(r.get_ref(8)?))
                            .filter(|n| *n > 0.0)
                            .map(|n| format!("{n} mm"))
                            .unwrap_or_default(),
                        flash: flash(&owned(r.get_ref(9)?)),
                        ..Default::default()
                    })
                })
                .optional()?
                .unwrap_or_default();
        }
        if let Some(st) = &mut self.custom
            && let Some(json) = st
                .query_row([id], |r| Ok(owned(r.get_ref(0)?)))
                .optional()?
            // Old tools leave raw control characters inside JSON strings.
            && let Ok(serde_json::Value::Object(fields)) =
                serde_json::from_str(&clean_json(&json))
        {
            let field = |key: &str| {
                fields
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(clean)
                    .unwrap_or_default()
            };
            // 5040/5043: lens model; 5029/5030: GPS latitude/longitude.
            extra.exif.lens = [field("5040"), field("5043")]
                .into_iter()
                .find(|l| !l.is_empty())
                .unwrap_or_default();
            let (lat, lon) = (coordinate(&field("5029")), coordinate(&field("5030")));
            if !lat.is_empty() && !lon.is_empty() {
                extra.exif.gps = format!("{lat}, {lon}");
            }
        }
        if let Some(st) = &mut self.audio {
            extra.audio = st
                .query_row([id], |r| {
                    Ok(Audio {
                        artist: clean(&owned(r.get_ref(0)?)),
                        album: clean(&owned(r.get_ref(1)?)),
                        year: Some(clean(&owned(r.get_ref(2)?)))
                            .filter(|y| y != "0")
                            .unwrap_or_default(),
                        track: number(&owned(r.get_ref(3)?)).filter(|n| *n > 0),
                        genre: clean(&owned(r.get_ref(4)?)),
                        title: clean(&owned(r.get_ref(5)?)),
                        duration: number::<f64>(&owned(r.get_ref(6)?)).filter(|n| *n > 0.0),
                        bitrate: number(&owned(r.get_ref(7)?)).filter(|n| *n > 0),
                        channels: number(&owned(r.get_ref(8)?)).filter(|n| *n > 0),
                        sample_rate: number(&owned(r.get_ref(9)?)).filter(|n| *n > 0),
                    })
                })
                .optional()?
                .unwrap_or_default();
        }
        if let Some(st) = &mut self.picture {
            extra.thumbnail = st
                .query_row([id], |r| {
                    Ok(match r.get_ref(0)? {
                        ValueRef::Blob(b) if b.len() <= crate::details::MAX_THUMBNAIL => {
                            Thumbnail::new(b.to_vec())
                        }
                        _ => None,
                    })
                })
                .optional()?
                .flatten();
        }
        Ok(extra)
    }
}
fn clean_json(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// WinCatalog stores most dates as `YYYY-MM-DD HH:MM:SS` but some as
/// `DD/MM/YYYY HH:MM:SS` (day first: no month field exceeds 12 in practice).
fn iso_date(value: &str) -> String {
    let b = value.as_bytes();
    let digits = |i: &[usize]| i.iter().all(|&n| b.get(n).is_some_and(u8::is_ascii_digit));
    if b.len() >= 10
        && b[2] == b'/'
        && b[5] == b'/'
        && digits(&[0, 1, 3, 4, 6, 7, 8, 9])
        && value.is_char_boundary(10)
    {
        format!(
            "{}-{}-{}{}",
            &value[6..10],
            &value[3..5],
            &value[0..2],
            &value[10..]
        )
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_lose_markup_padding_and_trailing_separators() {
        assert_eq!(
            comment("OLYMPUS DIGITAL CAMERA / \0\0\0 "),
            "OLYMPUS DIGITAL CAMERA"
        );
        assert_eq!(comment("\0\0\0\0"), "");
        assert_eq!(comment("<b>Bold</b> &amp; plain"), "Bold & plain");
    }

    #[test]
    fn day_first_dates_become_iso() {
        assert_eq!(iso_date("31/07/2024 14:52:04"), "2024-07-31 14:52:04");
        assert_eq!(iso_date("2024-07-31 14:52:04"), "2024-07-31 14:52:04");
        assert_eq!(iso_date(""), "");
        assert_eq!(iso_date("ž/ž/žžžž"), "ž/ž/žžžž");
    }

    fn catalog(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("Synthetic.w3cat");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE w3_items (id INTEGER PRIMARY KEY, flags INTEGER, itype INTEGER, rating INTEGER, name TEXT, comments TEXT);
             CREATE TABLE w3_fileInfo (id_item INTEGER PRIMARY KEY, name TEXT, date_change TEXT, date_create TEXT, size INTEGER, fileflags INTEGER, md5 TEXT, crc32 TEXT);
             CREATE TABLE w3_strict (id_item INTEGER PRIMARY KEY, id_parent INTEGER, refcount INTEGER);
             CREATE TABLE w3_volumes (id_volume INTEGER PRIMARY KEY);
             CREATE TABLE w3_volumeInfo (id_item INTEGER PRIMARY KEY, serial INTEGER);
             INSERT INTO w3_items (id, itype, name) VALUES
               (1, 187, 'Disc A'), (2, 172, 'Disk B'), (3, 200, 'Photos'), (4, 200, 'Trip'),
               (5, 2, 'IMG_1.JPG'), (6, 7, 'Display title'), (7, 100, 'bundle.zip'),
               (8, 1, 'inside.txt'), (9, 200, 'inner folder'), (10, 1, 'deep.txt'),
               (11, 1, 'on-b.txt'), (12, 110, 'a contact'), (13, 1, 'orphan.txt');
             INSERT INTO w3_fileInfo (id_item, name, date_change, size) VALUES
               (5, 'IMG_1.JPG', '31/07/2024 14:52:04', 1000),
               (6, 'Real name.epub', '2020-07-12 12:45:06', 20),
               (7, 'bundle.zip', '2021-01-02 03:04:05', 300),
               (8, 'inside.txt', '2021-01-02 03:04:05', 4),
               (10, 'deep.txt', '2021-01-02 03:04:05', 5),
               (11, 'on-b.txt', '', 6),
               (13, 'orphan.txt', '', 7);
             INSERT INTO w3_strict VALUES
               (1, 0, 1), (2, 0, 1), (3, 1, 1), (4, 3, 1), (5, 4, 1), (6, 4, 1), (7, 1, 1),
               (8, 7, 1), (9, 7, 1), (10, 9, 1), (11, 2, 1), (12, 0, 1), (13, 99, 1);
             INSERT INTO w3_volumes VALUES (1), (2);
             INSERT INTO w3_volumeInfo VALUES (1, 111), (2, 222);
             UPDATE w3_items SET rating = 75, comments = '<font face=\"x\">Holiday &amp; sea</font> / ' WHERE id = 5;
             CREATE TABLE w3_exifInfo (id_item INTEGER PRIMARY KEY, camera_make TEXT, camera_model TEXT, width INTEGER, height INTEGER, date_taken TEXT, iso_equivalent TEXT, exposure_time TEXT, aperture_number TEXT, focal_length TEXT, flash_used TEXT);
             INSERT INTO w3_exifInfo VALUES (5, 'OLYMPUS', 'OLYMPUS E-PL1', 4032, 3024, '2012-04-12 17:24:51', '500', '0.0003125', '3.5 (35/10)', '14 (14/1)', '8');
             CREATE TABLE w3_customfieldInfo (id_item INTEGER PRIMARY KEY, fields TEXT);
             INSERT INTO w3_customfieldInfo VALUES (5, '{\"5040\":\"EF-S18-135mm\\u0000\",\"5029\":\"44; 49.52; 0 N\",\"5030\":\"20; 27.09; 0 E\"}');
             CREATE TABLE w3_id3Info (id_item INTEGER PRIMARY KEY, artist TEXT, album TEXT, year, track, genre TEXT, title TEXT, duration TEXT, bitrate TEXT, channels TEXT, sample_rate TEXT);
             INSERT INTO w3_id3Info VALUES (6, 'Artist', 'Album', 2000, 9, 'Pop', 'Song', '286.639', '192016', '2', '44100'), (11, '', '', 0, 0, '', '', '', '', '', '');
             CREATE TABLE w3_picture (id_item INTEGER, number INTEGER, data BLOB);
             INSERT INTO w3_picture VALUES (5, 0, x'ffd8ffe000'), (6, 0, x'3c7376673e');
             CREATE TABLE w3_keyword_links (id_keyword INTEGER, id_item INTEGER);
             INSERT INTO w3_keyword_links VALUES (12, 5), (12, 5), (11, 5);",
        )
        .unwrap();
        path
    }

    #[test]
    fn reads_paths_media_kinds_and_skips_archive_contents() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = catalog(dir.path());
        let mut found = Vec::new();
        read(&path, &AtomicBool::new(false), true, &mut |r| {
            found.push((
                r.media,
                r.kind,
                r.file.relative,
                r.file.size,
                r.file.modified,
            ));
            Ok(())
        })?;
        found.sort();
        let names: Vec<_> = found
            .iter()
            .map(|f| (f.0.as_str(), f.1, f.2.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("Disc A", "bluray", "Photos/Trip/IMG_1.JPG"),
                ("Disc A", "bluray", "Photos/Trip/Real name.epub"),
                ("Disc A", "bluray", "bundle.zip"),
                ("Disk B", "external", "on-b.txt"),
            ]
        );
        assert_eq!(found[0].3, 1000);
        // 31 July 2024, day-first
        assert_eq!(found[0].4, 1_722_437_524);
        Ok(())
    }

    #[test]
    fn reads_thumbnail_exif_audio_tags_rating_and_comment() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = catalog(dir.path());
        let mut found = HashMap::new();
        read(&path, &AtomicBool::new(false), true, &mut |r| {
            found.insert(r.file.name.clone(), r.file.extra.clone());
            Ok(())
        })?;
        let photo = found["IMG_1.JPG"].as_ref().unwrap();
        assert_eq!(photo.rating, Some(75));
        assert_eq!(photo.comment, "Holiday & sea");
        assert_eq!(photo.tags, ["a contact", "on-b.txt"]);
        assert_eq!(photo.thumbnail.as_ref().unwrap().mime, "image/jpeg");
        let exif = &photo.exif;
        assert_eq!(exif.camera, "OLYMPUS E-PL1");
        assert_eq!((exif.width, exif.height), (Some(4032), Some(3024)));
        assert_eq!(exif.iso, "500");
        assert_eq!(exif.exposure, "1/3200 s");
        assert_eq!(exif.aperture, "f/3.5");
        assert_eq!(exif.focal_length, "14 mm");
        assert_eq!(exif.flash, "not_fired");
        assert_eq!(exif.lens, "EF-S18-135mm");
        assert_eq!(exif.gps, "44°49.52′ N, 20°27.09′ E");
        // The "thumbnail" that is really an SVG is refused.
        let epub = found["Real name.epub"].as_ref().unwrap();
        assert!(epub.thumbnail.is_none());
        assert_eq!(epub.audio.artist, "Artist");
        assert_eq!(epub.audio.year, "2000");
        assert_eq!(epub.audio.track, Some(9));
        assert_eq!(epub.audio.duration, Some(286.639));
        assert_eq!(epub.audio.bitrate, Some(192016));
        // Rows with only empty values add nothing.
        assert!(found["on-b.txt"].is_none());
        // The validation pass does not read details.
        read(&path, &AtomicBool::new(false), false, &mut |r| {
            assert!(r.file.extra.is_none());
            Ok(())
        })?;
        Ok(())
    }

    #[test]
    fn rejects_other_sqlite_files_and_leaves_no_side_files() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("Other.w3cat");
        Connection::open(&path)?.execute_batch("CREATE TABLE unrelated (x)")?;
        let error = read(&path, &AtomicBool::new(false), true, &mut |_| Ok(())).unwrap_err();
        assert!(format!("{error:#}").contains("Not a WinCatalog database"));
        let good = catalog(dir.path());
        read(&good, &AtomicBool::new(false), true, &mut |_| Ok(()))?;
        let mut files: Vec<_> = fs::read_dir(dir.path())?
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["Other.w3cat", "Synthetic.w3cat"]);
        Ok(())
    }
}
