//! Optional per-file details that a source can supply next to the plain file
//! record: thumbnail, EXIF, audio tags, CRC32, rating, comment and tags.
//!
//! Everything here is additive. The small text/number details live in extra
//! tables of `catalog.sqlite` (older versions ignore tables they do not know;
//! deleting a file cascades). Thumbnails are derived data like extracted text,
//! so they live in `content.sqlite` bound to the file version. A catalog
//! exported without content therefore stays small and loses only thumbnails.
use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::{Value, json};

/// Thumbnails above this size are not stored (WinCatalog's own are < 300 KB).
pub const MAX_THUMBNAIL: usize = 512 * 1024;
const MAX_TEXT: usize = 4096;
const MAX_TAGS: usize = 256;

#[derive(Clone, Default)]
pub struct Exif {
    pub camera: String,
    pub lens: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub taken: String,
    pub iso: String,
    pub exposure: String,
    pub aperture: String,
    pub focal_length: String,
    pub flash: String,
    pub gps: String,
}
impl Exif {
    fn is_empty(&self) -> bool {
        self.camera.is_empty()
            && self.lens.is_empty()
            && self.width.is_none()
            && self.height.is_none()
            && self.taken.is_empty()
            && self.iso.is_empty()
            && self.exposure.is_empty()
            && self.aperture.is_empty()
            && self.focal_length.is_empty()
            && self.flash.is_empty()
            && self.gps.is_empty()
    }
}

#[derive(Clone, Default)]
pub struct Audio {
    pub artist: String,
    pub album: String,
    pub year: String,
    pub track: Option<i64>,
    pub genre: String,
    pub title: String,
    pub duration: Option<f64>,
    pub bitrate: Option<i64>,
    pub channels: Option<i64>,
    pub sample_rate: Option<i64>,
}
impl Audio {
    fn is_empty(&self) -> bool {
        self.artist.is_empty()
            && self.album.is_empty()
            && self.year.is_empty()
            && self.track.is_none()
            && self.genre.is_empty()
            && self.title.is_empty()
            && self.duration.is_none()
            && self.bitrate.is_none()
    }
}

#[derive(Clone)]
pub struct Thumbnail {
    pub mime: &'static str,
    pub data: Vec<u8>,
}
impl Thumbnail {
    /// Accepts only JPEG and PNG (by magic bytes), so the stored type can
    /// never be something a browser would run.
    pub fn new(data: Vec<u8>) -> Option<Self> {
        if data.is_empty() || data.len() > MAX_THUMBNAIL {
            return None;
        }
        let mime = if data.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else {
            return None;
        };
        Some(Self { mime, data })
    }
}

#[derive(Clone, Default)]
pub struct FileExtra {
    pub crc32: String,
    /// Source scale 1–100; shown as up to five stars.
    pub rating: Option<i64>,
    pub comment: String,
    pub tags: Vec<String>,
    pub exif: Exif,
    pub audio: Audio,
    pub thumbnail: Option<Thumbnail>,
}
impl FileExtra {
    pub fn is_empty(&self) -> bool {
        self.crc32.is_empty()
            && self.rating.is_none()
            && self.comment.is_empty()
            && self.tags.is_empty()
            && self.exif.is_empty()
            && self.audio.is_empty()
            && self.thumbnail.is_none()
    }
    pub fn thumbnail_bytes(&self) -> usize {
        self.thumbnail.as_ref().map_or(0, |t| t.data.len())
    }
}

fn limited(value: &str) -> String {
    let value = value.trim();
    if value.len() <= MAX_TEXT {
        return value.to_owned();
    }
    let mut end = MAX_TEXT;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// Prepared statements for one transaction.
pub struct Writer<'a> {
    info: rusqlite::Statement<'a>,
    exif: rusqlite::Statement<'a>,
    audio: rusqlite::Statement<'a>,
    clear_tags: rusqlite::Statement<'a>,
    add_tag: rusqlite::Statement<'a>,
    link_tag: rusqlite::Statement<'a>,
    drop_info: rusqlite::Statement<'a>,
    drop_exif: rusqlite::Statement<'a>,
    drop_audio: rusqlite::Statement<'a>,
    thumbnail: Option<rusqlite::Statement<'a>>,
    drop_thumbnail: Option<rusqlite::Statement<'a>>,
}
impl<'a> Writer<'a> {
    pub fn new(tx: &'a Transaction<'_>, thumbnails: bool) -> Result<Self> {
        Ok(Self {
            info: tx.prepare("INSERT OR REPLACE INTO file_info(file_id,crc32,rating,comment) VALUES(?,?,?,?)")?,
            exif: tx.prepare("INSERT OR REPLACE INTO file_exif(file_id,camera,lens,width,height,taken,iso,exposure,aperture,focal_length,flash,gps) VALUES(?,?,?,?,?,?,?,?,?,?,?,?)")?,
            audio: tx.prepare("INSERT OR REPLACE INTO file_audio(file_id,artist,album,year,track,genre,title,duration,bitrate,channels,sample_rate) VALUES(?,?,?,?,?,?,?,?,?,?,?)")?,
            clear_tags: tx.prepare("DELETE FROM file_tags WHERE file_id=?")?,
            add_tag: tx.prepare("INSERT OR IGNORE INTO tags(name) VALUES(?)")?,
            link_tag: tx.prepare("INSERT OR IGNORE INTO file_tags(file_id,tag_id) SELECT ?,id FROM tags WHERE name=?")?,
            drop_info: tx.prepare("DELETE FROM file_info WHERE file_id=?")?,
            drop_exif: tx.prepare("DELETE FROM file_exif WHERE file_id=?")?,
            drop_audio: tx.prepare("DELETE FROM file_audio WHERE file_id=?")?,
            thumbnail: if thumbnails {
                Some(tx.prepare("INSERT OR REPLACE INTO content.thumbnails(file_id,version,mime,data) VALUES(?,?,?,?)")?)
            } else {
                None
            },
            drop_thumbnail: if thumbnails {
                Some(tx.prepare("DELETE FROM content.thumbnails WHERE file_id=?")?)
            } else {
                None
            },
        })
    }

    /// Replace everything stored for `id` with `extra`. `version` identifies
    /// the file revision a thumbnail belongs to.
    pub fn save(&mut self, id: i64, version: &str, extra: &FileExtra) -> Result<()> {
        if extra.crc32.is_empty() && extra.rating.is_none() && extra.comment.is_empty() {
            self.drop_info.execute([id])?;
        } else {
            self.info.execute(params![
                id,
                extra.crc32,
                extra.rating,
                limited(&extra.comment)
            ])?;
        }
        if extra.exif.is_empty() {
            self.drop_exif.execute([id])?;
        } else {
            let e = &extra.exif;
            self.exif.execute(params![
                id,
                limited(&e.camera),
                limited(&e.lens),
                e.width,
                e.height,
                limited(&e.taken),
                limited(&e.iso),
                limited(&e.exposure),
                limited(&e.aperture),
                limited(&e.focal_length),
                limited(&e.flash),
                limited(&e.gps)
            ])?;
        }
        if extra.audio.is_empty() {
            self.drop_audio.execute([id])?;
        } else {
            let a = &extra.audio;
            self.audio.execute(params![
                id,
                limited(&a.artist),
                limited(&a.album),
                limited(&a.year),
                a.track,
                limited(&a.genre),
                limited(&a.title),
                a.duration,
                a.bitrate,
                a.channels,
                a.sample_rate
            ])?;
        }
        self.clear_tags.execute([id])?;
        for tag in extra.tags.iter().take(MAX_TAGS) {
            let tag = limited(tag);
            if !tag.is_empty() {
                self.add_tag.execute([&tag])?;
                self.link_tag.execute(params![id, tag])?;
            }
        }
        if let (Some(add), Some(drop)) = (&mut self.thumbnail, &mut self.drop_thumbnail) {
            match &extra.thumbnail {
                Some(t) => add.execute(params![id, version, t.mime, t.data])?,
                None => drop.execute([id])?,
            };
        }
        Ok(())
    }
}

/// Details of one file as JSON for the file view; `null` members are omitted.
pub fn load(conn: &Connection, id: i64, thumbnails: bool, version: &str) -> Result<Value> {
    let mut out = serde_json::Map::new();
    if let Some((crc32, rating, comment)) = conn
        .query_row(
            "SELECT crc32,rating,comment FROM file_info WHERE file_id=?",
            [id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                ))
            },
        )
        .optional()?
    {
        if !crc32.is_empty() {
            out.insert("crc32".into(), crc32.into());
        }
        if let Some(rating) = rating {
            out.insert("rating".into(), json!(stars(rating)));
        }
        if !comment.is_empty() {
            out.insert("comment".into(), comment.into());
        }
    }
    let tags = conn
        .prepare("SELECT t.name FROM file_tags ft JOIN tags t ON t.id=ft.tag_id WHERE ft.file_id=? ORDER BY t.name COLLATE NOCASE")?
        .query_map([id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !tags.is_empty() {
        out.insert("tags".into(), json!(tags));
    }
    if let Some(v) = conn
        .query_row(
            "SELECT camera,lens,width,height,taken,iso,exposure,aperture,focal_length,flash,gps FROM file_exif WHERE file_id=?",
            [id],
            |r| {
                let text = |i| -> rusqlite::Result<String> {
                    Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default())
                };
                let mut m = serde_json::Map::new();
                for (key, i) in [
                    ("camera", 0),
                    ("lens", 1),
                    ("taken", 4),
                    ("iso", 5),
                    ("exposure", 6),
                    ("aperture", 7),
                    ("focal_length", 8),
                    ("flash", 9),
                    ("gps", 10),
                ] {
                    let value = text(i)?;
                    if !value.is_empty() {
                        m.insert(key.into(), value.into());
                    }
                }
                for (key, i) in [("width", 2), ("height", 3)] {
                    if let Some(n) = r.get::<_, Option<i64>>(i)?.filter(|n| *n > 0) {
                        m.insert(key.into(), n.into());
                    }
                }
                Ok(Value::Object(m))
            },
        )
        .optional()?
    {
        out.insert("exif".into(), v);
    }
    if let Some(v) = conn
        .query_row(
            "SELECT artist,album,year,track,genre,title,duration,bitrate,channels,sample_rate FROM file_audio WHERE file_id=?",
            [id],
            |r| {
                let mut m = serde_json::Map::new();
                for (key, i) in [("artist", 0), ("album", 1), ("year", 2), ("genre", 4), ("title", 5)] {
                    let value = r.get::<_, Option<String>>(i)?.unwrap_or_default();
                    if !value.is_empty() {
                        m.insert(key.into(), value.into());
                    }
                }
                if let Some(d) = r.get::<_, Option<f64>>(6)?.filter(|d| *d > 0.0) {
                    m.insert("duration".into(), json!(d));
                }
                for (key, i) in [("track", 3), ("bitrate", 7), ("channels", 8), ("sample_rate", 9)] {
                    if let Some(n) = r.get::<_, Option<i64>>(i)?.filter(|n| *n > 0) {
                        m.insert(key.into(), n.into());
                    }
                }
                Ok(Value::Object(m))
            },
        )
        .optional()?
    {
        out.insert("audio".into(), v);
    }
    if thumbnails
        && let Some((mime, data)) = conn
            .query_row(
                "SELECT mime,data FROM content.thumbnails WHERE file_id=? AND version=?",
                params![id, version],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
            )
            .optional()?
        // Only the two types accepted at import are ever returned.
        && matches!(mime.as_str(), "image/jpeg" | "image/png")
        && data.len() <= MAX_THUMBNAIL
    {
        out.insert(
            "thumbnail".into(),
            format!("data:{mime};base64,{}", STANDARD.encode(data)).into(),
        );
    }
    Ok(Value::Object(out))
}

/// WinCatalog rates 1–99 (25 per star); display as 1–5 stars.
fn stars(rating: i64) -> i64 {
    ((rating.clamp(1, 100) + 19) / 20).clamp(1, 5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnails_accept_only_jpeg_and_png_within_the_limit() {
        assert!(Thumbnail::new(vec![0xff, 0xd8, 0xff, 0xe0, 1]).is_some());
        assert_eq!(
            Thumbnail::new(b"\x89PNG\r\n\x1a\nrest".to_vec())
                .unwrap()
                .mime,
            "image/png"
        );
        assert!(Thumbnail::new(b"<svg onload=alert(1)>".to_vec()).is_none());
        assert!(Thumbnail::new(b"GIF89a".to_vec()).is_none());
        assert!(Thumbnail::new(Vec::new()).is_none());
        let mut big = vec![0xff, 0xd8, 0xff];
        big.resize(MAX_THUMBNAIL + 1, 0);
        assert!(Thumbnail::new(big).is_none());
    }

    #[test]
    fn rating_maps_to_stars() {
        assert_eq!(
            [1, 18, 25, 50, 75, 99, 100].map(stars),
            [1, 1, 2, 3, 4, 5, 5]
        );
    }
}
