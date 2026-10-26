use crate::{
    crypto::{self, Encryption},
    encrypted_vfs::EncryptedVfs,
};
use anyhow::{Context, Result, bail};
use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Row, params, params_from_iter,
    types::Value as SqlValue,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub const MAX_TEXT: usize = 8 << 20;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
// Decode only the requested character page plus one lookahead character.
// The decompressed document never needs to be materialized for the details panel.
fn text_page(payload: &[u8], offset: usize, limit: usize) -> Result<(String, Option<usize>, bool)> {
    if payload.len() > MAX_TEXT + 65536 {
        bail!("Stored document exceeds compressed size limit");
    }
    let mut decoder = ZlibDecoder::new(payload);
    let mut bytes = [0u8; 8192];
    let mut carry = Vec::with_capacity(8196);
    let mut total_bytes = 0;
    let mut count = 0;
    let mut selected = String::new();
    loop {
        let n = decoder.read(&mut bytes)?;
        total_bytes += n;
        if total_bytes > MAX_TEXT {
            bail!("Stored document exceeds text limit");
        }
        carry.extend_from_slice(&bytes[..n]);
        let valid = match std::str::from_utf8(&carry) {
            Ok(_) => carry.len(),
            Err(e) if e.error_len().is_none() && n > 0 => e.valid_up_to(),
            Err(_) => bail!("Stored document contains invalid UTF-8"),
        };
        for ch in std::str::from_utf8(&carry[..valid])?.chars() {
            if count >= offset + limit {
                return Ok((selected, None, true));
            }
            if count >= offset {
                selected.push(ch);
            }
            count += 1;
        }
        carry.drain(..valid);
        if n == 0 {
            return Ok((selected, Some(count), false));
        }
    }
}
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .replace(['č', 'ć'], "c")
        .replace('š', "s")
        .replace('ž', "z")
        .replace('đ', "dj")
}
pub fn absolute(path: &Path) -> Result<PathBuf> {
    Ok(std::path::absolute(path)?)
}
pub fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}
pub fn private_dir(path: &Path) -> Result<()> {
    #[allow(unused_mut)]
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Source {
    pub id: String,
    pub root: String,
    pub label: String,
    pub kind: String,
    pub location: String,
    pub last_scan: String,
    pub status: String,
    pub file_count: i64,
    pub total_size: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileResult {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub relative_path: String,
    pub source_id: String,
    pub source_label: String,
    pub kind: String,
    pub location: String,
    pub size: i64,
    pub modified: i64,
    pub extension: String,
    pub text_status: String,
    pub text_detail: String,
    pub snippet: String,
    #[serde(skip)]
    pub root: String,
}
#[derive(Debug, Default, Deserialize, Clone)]
#[serde(default, deny_unknown_fields)]
pub struct ScanOptions {
    pub root: String,
    pub label: String,
    pub kind: String,
    pub location: String,
    pub source_id: String,
    pub index_content: bool,
}
#[derive(Clone)]
pub struct ScannedFile {
    pub relative: String,
    pub name: String,
    pub dir: String,
    pub extension: String,
    pub size: i64,
    pub modified: i64,
    pub modified_ns: i64,
}
pub struct IndexedFile {
    pub id: i64,
    pub relative: String,
    pub version: String,
    pub needs_text: bool,
}
#[derive(Default, Deserialize, Clone)]
#[serde(default)]
pub struct SearchOptions {
    #[serde(rename = "q")]
    pub query: String,
    pub mode: String,
    pub source: String,
    #[serde(rename = "ext")]
    pub extension: String,
    pub sort: String,
    pub offset: i64,
    pub limit: i64,
}
#[derive(Serialize)]
pub struct SearchResult {
    pub results: Vec<FileResult>,
    pub total: i64,
    pub elapsed_ms: f64,
    pub offset: i64,
    pub limit: i64,
}
const COLUMNS: &str = "f.id,f.name,d.path,s.id,s.label,s.kind,s.location,f.size,f.modified,f.extension,f.text_status,f.text_detail,s.root";
const FROM: &str =
    " FROM files f JOIN directories d ON d.id=f.dir_id JOIN sources s ON s.id=f.source_id ";

pub struct Store {
    // Drop connection (and all attached files) before unregistering its VFS.
    pub conn: Connection,
    vfs: Option<EncryptedVfs>,
    pub encryption: Option<Arc<Encryption>>,
    pub dir: PathBuf,
    identity: same_file::Handle,
    _lock: File,
    pub catalog_id: String,
    attached: bool,
}
fn open_connection(path: &Path, vfs: Option<&EncryptedVfs>) -> Result<Connection> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_file() => {
            bail!("Database must be a regular file, not a symlink")
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            drop(private_file(path)?);
        }
        Err(e) => return Err(e.into()),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = match vfs {
        Some(v) => Connection::open_with_flags_and_vfs(path, flags, v.name())?,
        None => Connection::open_with_flags(path, flags)?,
    };
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch("PRAGMA temp_store=MEMORY;PRAGMA trusted_schema=ON;PRAGMA mmap_size=0;")?;
    Ok(conn)
}
fn uri(path: &Path, vfs: Option<&EncryptedVfs>) -> Result<String> {
    let mut url =
        url::Url::from_file_path(path).map_err(|_| anyhow::anyhow!("Invalid database path"))?;
    if let Some(v) = vfs {
        url.query_pairs_mut().append_pair("vfs", v.name());
    }
    Ok(url.to_string())
}
impl Store {
    pub fn open(dir: &Path, password: &str) -> Result<Self> {
        let dir = dunce::canonicalize(dir).context("Direktorijum podataka mora već postojati")?;
        if !dir.is_dir() {
            bail!("Data directory is not a folder");
        }
        if dir.join(".allfiles-export-incomplete").exists() {
            bail!(
                "Ovaj izvoz nije dovršen. Otvorite izvorni katalog i ponovite izvoz u novi folder."
            );
        }
        let identity = same_file::Handle::from_path(&dir)?;
        let mut lock_options = OpenOptions::new();
        lock_options
            .read(true)
            .write(true)
            .create(true)
            .truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            lock_options.mode(0o600);
        }
        let lock = lock_options.open(dir.join(".allfiles.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .context("Katalog već koristi druga AllFiles instanca")?;
        let manifest = crypto::manifest(&dir)?;
        let encryption = match manifest {
            Some(m) => {
                if !dir.join("catalog.sqlite").is_file()
                    || dir.join("catalog.sqlite").metadata()?.len() == 0
                {
                    bail!("Enkriptovani katalog nedostaje ili je nepotpun.");
                }
                Some(Arc::new(Encryption::derive(m, password)?))
            }
            None => {
                if !password.is_empty() {
                    bail!("Ovaj katalog nije enkriptovan.");
                }
                None
            }
        };
        let vfs = encryption
            .as_ref()
            .map(|e| EncryptedVfs::new(&dir, e.clone()))
            .transpose()?;
        let conn = open_connection(&dir.join("catalog.sqlite"), vfs.as_ref())?;
        if let Err(e) = conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        }) {
            if encryption.is_some() {
                bail!("Pogrešna lozinka ili oštećena enkriptovana baza.");
            }
            return Err(e.into());
        }
        let existing: i64 = conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='meta'",
            [],
            |r| r.get(0),
        )?;
        if existing > 0 {
            let version: Option<String> = conn
                .query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                    r.get(0)
                })
                .optional()?;
            if version.as_deref() != Some("1") {
                bail!("Unsupported catalog schema");
            }
        }
        conn.execute_batch(include_str!("sql/catalog.sql"))?;
        let schema: String =
            conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| {
                r.get(0)
            })?;
        if schema != "1" {
            bail!("Unsupported catalog schema");
        }
        conn.execute(
            "INSERT OR IGNORE INTO meta(key,value) VALUES('catalog_id',?)",
            [crypto::random_id()],
        )?;
        let catalog_id =
            conn.query_row("SELECT value FROM meta WHERE key='catalog_id'", [], |r| {
                r.get(0)
            })?;
        conn.execute_batch(&format!("BEGIN;{}COMMIT;", include_str!("sql/stats.sql")))?;
        conn.execute(
            "UPDATE sources SET status='interrupted' WHERE status='scanning'",
            [],
        )?;
        Ok(Self {
            conn,
            vfs,
            encryption,
            dir,
            identity,
            _lock: lock,
            catalog_id,
            attached: false,
        })
    }
    pub fn check(&self) -> Result<()> {
        if same_file::Handle::from_path(&self.dir).ok().as_ref() != Some(&self.identity) {
            bail!(
                "Direktorijum podataka više nije isti ili nije dostupan. Ponovo montirajte kontejner i pokrenite aplikaciju."
            );
        }
        Ok(())
    }
    pub fn ensure_content(&mut self, create: bool) -> Result<bool> {
        self.check()?;
        if self.attached {
            return Ok(true);
        }
        let path = self.dir.join("content.sqlite");
        if !create && !path.exists() {
            return Ok(false);
        }
        if !path.exists() {
            drop(private_file(&path)?);
        } else if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            bail!("Content database must not be a symlink");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }
        self.conn.execute(
            "ATTACH DATABASE ? AS content",
            [uri(&path, self.vfs.as_ref())?],
        )?;
        let result = (|| -> Result<()> {
            let tables: i64 = self.conn.query_row(
                "SELECT count(*) FROM content.sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )?;
            if tables > 0 {
                let owner: String = self.conn.query_row(
                    "SELECT value FROM content.meta WHERE key='catalog_id'",
                    [],
                    |r| r.get(0),
                )?;
                if owner != self.catalog_id {
                    bail!("Baza sadržaja pripada drugom katalogu ili nije AllFiles baza.");
                }
            }
            self.conn.execute_batch(include_str!("sql/content.sql"))?;
            self.conn.execute(
                "INSERT OR IGNORE INTO content.meta(key,value) VALUES('catalog_id',?)",
                [&self.catalog_id],
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = self.conn.execute_batch("DETACH DATABASE content");
            return Err(e);
        }
        self.attached = true;
        Ok(true)
    }
    pub fn stats(&self) -> Result<Value> {
        self.check()?;
        let (files, bytes, documents, sources) = self.conn.query_row(
            "SELECT files,bytes,documents,sources FROM catalog_stats WHERE id=1",
            [],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?;
        let demo: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='demo'", [], |r| r.get(0))
            .optional()?;
        let size = |name: &str| self.dir.join(name).metadata().map(|s| s.len()).unwrap_or(0);
        Ok(
            json!({"version":VERSION,"backend":"rust","sqlite_version":rusqlite::version(),"data_dir":self.dir,"demo":demo.as_deref()==Some("true"),"files":files,"bytes":bytes,"documents":documents,"sources":sources,"catalog_bytes":size("catalog.sqlite"),"content_bytes":size("content.sqlite"),"fts5":true,"encrypted":self.encryption.is_some()}),
        )
    }
    pub fn sources(&self, id: &str, offset: i64, limit: i64) -> Result<(Vec<Source>, i64)> {
        self.check()?;
        if !(0..=1_000_000).contains(&offset) || !(1..=200).contains(&limit) || id.len() > 256 {
            bail!("Neispravna stranica izvora");
        }
        let where_sql = if id.is_empty() { "" } else { " WHERE s.id=?" };
        let mut args: Vec<SqlValue> = Vec::new();
        if !id.is_empty() {
            args.push(id.to_owned().into());
        }
        let total = self.conn.query_row(
            &format!("SELECT count(*) FROM sources s{where_sql}"),
            params_from_iter(args.iter()),
            |r| r.get(0),
        )?;
        args.push(limit.into());
        args.push(offset.into());
        let sql = format!(
            "SELECT s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status,COUNT(f.id),COALESCE(SUM(f.size),0) FROM (SELECT * FROM sources s{where_sql} ORDER BY s.label,s.id LIMIT ? OFFSET ?) s LEFT JOIN files f ON f.source_id=s.id GROUP BY s.id ORDER BY s.label,s.id"
        );
        let out = self
            .conn
            .prepare(&sql)?
            .query_map(params_from_iter(args.iter()), |r| {
                Ok(Source {
                    id: r.get(0)?,
                    root: r.get(1)?,
                    label: r.get(2)?,
                    kind: r.get(3)?,
                    location: r.get(4)?,
                    last_scan: r.get(5)?,
                    status: r.get(6)?,
                    file_count: r.get(7)?,
                    total_size: r.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok((out, total))
    }
    pub fn all_sources(&self) -> Result<Vec<Source>> {
        let mut out = Vec::new();
        loop {
            let (page, total) = self.sources("", out.len() as i64, 200)?;
            out.extend(page);
            if out.len() as i64 >= total {
                break;
            }
        }
        Ok(out)
    }
    pub fn begin_scan(&mut self, o: &ScanOptions) -> Result<Source> {
        self.check()?;
        let label = if o.label.is_empty() {
            Path::new(&o.root)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        } else {
            o.label.clone()
        };
        let id = if !o.source_id.is_empty() {
            self.conn
                .query_row("SELECT id FROM sources WHERE id=?", [&o.source_id], |r| {
                    r.get::<_, String>(0)
                })?
        } else {
            self.conn
                .query_row(
                    "SELECT id FROM sources WHERE root=? AND label=?",
                    params![o.root, label],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
                .unwrap_or_else(crypto::random_id)
        };
        let kind = if o.kind.is_empty() { "folder" } else { &o.kind };
        self.conn.execute("INSERT INTO sources(id,root,label,kind,location,status) VALUES(?,?,?,?,?,'scanning') ON CONFLICT(id) DO UPDATE SET root=excluded.root,label=excluded.label,kind=excluded.kind,location=excluded.location,status='scanning'",params![id,o.root,label,kind,o.location])?;
        Ok(Source {
            id,
            root: o.root.clone(),
            label,
            kind: kind.into(),
            location: o.location.clone(),
            status: "scanning".into(),
            ..Default::default()
        })
    }
    pub fn upsert(
        &mut self,
        source: &str,
        scan: &str,
        entries: &[ScannedFile],
    ) -> Result<Vec<IndexedFile>> {
        self.check()?;
        let has_content = self.ensure_content(false)?;
        let tx = self.conn.transaction()?;
        let mut dirs = HashMap::new();
        let mut out = Vec::with_capacity(entries.len());
        for f in entries {
            let dir = if f.dir == "." {
                String::new()
            } else {
                f.dir.replace('\\', "/")
            };
            let did = if let Some(d) = dirs.get(&dir) {
                *d
            } else {
                tx.execute(
                    "INSERT OR IGNORE INTO directories(source_id,path,search_path) VALUES(?,?,?)",
                    params![source, dir, fold(&dir)],
                )?;
                let d: i64 = tx.query_row(
                    "SELECT id FROM directories WHERE source_id=? AND path=?",
                    params![source, dir],
                    |r| r.get(0),
                )?;
                dirs.insert(dir, d);
                d
            };
            let version = format!("{}:{}", f.size, f.modified_ns);
            let old: Option<(i64, String, String)> = tx
                .query_row(
                    "SELECT id,version,text_status FROM files WHERE dir_id=? AND name=?",
                    params![did, f.name],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let fresh = old.is_none();
            let (id, previous, status) = if let Some((id, previous, status)) = old {
                tx.execute("UPDATE files SET size=?,modified=?,modified_ns=?,version=?,seen_scan=?,text_status=CASE WHEN version!=? THEN 'not_indexed' ELSE text_status END,text_detail=CASE WHEN version!=? THEN '' ELSE text_detail END WHERE id=?",params![f.size,f.modified,f.modified_ns,version,scan,version,version,id])?;
                (id, previous, status)
            } else {
                tx.execute("INSERT INTO files(source_id,dir_id,name,search_name,extension,size,modified,modified_ns,version,seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?)",params![source,did,f.name,fold(&f.name),f.extension.trim_start_matches('.').to_lowercase(),f.size,f.modified,f.modified_ns,version,scan])?;
                (
                    tx.last_insert_rowid(),
                    String::new(),
                    "not_indexed".to_owned(),
                )
            };
            let candidate = crate::extract::candidate(&f.extension);
            if !candidate && (fresh || previous != version) {
                tx.execute(
                    "UPDATE files SET text_status='unsupported',text_detail='' WHERE id=?",
                    [id],
                )?;
            }
            let expects = matches!(status.as_str(), "indexed" | "truncated");
            let mut needs = candidate
                && (fresh
                    || previous != version
                    || status == "not_indexed"
                    || status == "error"
                    || (!has_content && expects));
            if candidate && has_content && expects && !needs {
                let linked:i64=tx.query_row("SELECT count(*) FROM content.links l JOIN content.documents d ON d.id=l.doc_id WHERE l.file_id=? AND l.version=?",params![id,version],|r|r.get(0))?;
                needs = linked == 0;
            }
            out.push(IndexedFile {
                id,
                relative: f.relative.clone(),
                version,
                needs_text: needs,
            });
        }
        tx.commit()?;
        Ok(out)
    }
    pub fn record_content(
        &mut self,
        id: i64,
        version: &str,
        status: &str,
        text: &str,
        detail: &str,
    ) -> Result<()> {
        self.check()?;
        if text.len() > MAX_TEXT {
            bail!("Document text limit exceeded");
        }
        let has_text = !text.is_empty() && matches!(status, "indexed" | "truncated");
        if has_text || self.attached {
            self.ensure_content(true)?;
        }
        let tx = self.conn.transaction()?;
        let current: String =
            tx.query_row("SELECT version FROM files WHERE id=?", [id], |r| r.get(0))?;
        if current != version {
            bail!("Dokument je izmenjen tokom indeksiranja");
        }
        if self.attached {
            tx.execute("DELETE FROM content.links WHERE file_id=?", [id])?;
        }
        if has_text {
            let hash = hex::encode(Sha256::digest(text.as_bytes()));
            let doc: Option<i64> = tx
                .query_row(
                    "SELECT id FROM content.documents WHERE hash=?",
                    [&hash],
                    |r| r.get(0),
                )
                .optional()?;
            let doc = if let Some(id) = doc {
                id
            } else {
                let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                z.write_all(text.as_bytes())?;
                let payload = z.finish()?;
                tx.execute(
                    "INSERT INTO content.documents(hash,payload,plain_size) VALUES(?,?,?)",
                    params![hash, payload, text.len() as i64],
                )?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO content.fts(rowid,body) VALUES(?,?)",
                    params![id, text],
                )?;
                id
            };
            tx.execute(
                "INSERT INTO content.links(file_id,version,doc_id) VALUES(?,?,?)",
                params![id, version, doc],
            )?;
        }
        tx.execute(
            "UPDATE files SET text_status=?,text_detail=? WHERE id=?",
            params![status, detail, id],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish_scan(&mut self, source: &str, scan: &str, complete: bool) -> Result<()> {
        self.check()?;
        self.ensure_content(false)?;
        let tx = self.conn.transaction()?;
        if complete {
            if self.attached {
                tx.execute("DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE source_id=? AND seen_scan!=?)",params![source,scan])?;
            }
            tx.execute(
                "DELETE FROM files WHERE source_id=? AND seen_scan!=?",
                params![source, scan],
            )?;
            tx.execute("DELETE FROM directories WHERE source_id=? AND id NOT IN(SELECT dir_id FROM files WHERE source_id=?)",params![source,source])?;
        }
        if self.attached {
            tx.execute_batch("DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE text_status NOT IN ('indexed','truncated'));DELETE FROM content.fts WHERE rowid IN(SELECT id FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links));DELETE FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links);")?;
        }
        tx.execute(
            "UPDATE sources SET status=?,last_scan=? WHERE id=?",
            params![
                if complete { "ready" } else { "partial" },
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                source
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub fn search(&mut self, mut o: SearchOptions) -> Result<SearchResult> {
        let start = Instant::now();
        self.check()?;
        if o.query.len() > 1000 {
            bail!("Upit je predugačak");
        }
        if o.limit <= 0 || o.limit > 200 {
            o.limit = 100;
        }
        o.offset = o.offset.max(0);
        if o.mode.is_empty() {
            o.mode = "files".into();
        }
        if !matches!(o.mode.as_str(), "files" | "content" | "all") {
            bail!("Nepoznat režim pretrage");
        }
        let mut clauses = vec!["1=1".to_owned()];
        let mut args: Vec<SqlValue> = Vec::new();
        if !o.source.is_empty() {
            clauses.push("f.source_id=?".into());
            args.push(o.source.clone().into());
        }
        if !o.extension.is_empty() {
            clauses.push("f.extension=?".into());
            args.push(o.extension.trim_start_matches('.').to_lowercase().into());
        }
        let query = o.query.trim();
        let mut names = Vec::new();
        let mut name_args: Vec<SqlValue> = Vec::new();
        for term in query.replace('"', "").split_whitespace() {
            let g = format!("*{}*", glob_literal(&fold(term)));
            names.push("f.id IN(SELECT rowid FROM names WHERE search_name GLOB ? UNION SELECT pf.id FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))");
            name_args.push(g.clone().into());
            name_args.push(g.into());
        }
        let name_sql = if names.is_empty() {
            "1=1".into()
        } else {
            names.join(" AND ")
        };
        let mut content_sql = "0";
        let mut content_args: Vec<SqlValue> = Vec::new();
        if o.mode != "files" && self.ensure_content(false)? {
            content_sql = "f.id IN(SELECT l.file_id FROM content.links l JOIN files cf ON cf.id=l.file_id AND cf.version=l.version)";
            let tq = text_query(query);
            if !tq.is_empty() {
                content_sql = "f.id IN(SELECT l.file_id FROM content.links l JOIN files cf ON cf.id=l.file_id AND cf.version=l.version WHERE l.doc_id IN(SELECT rowid FROM content.fts WHERE fts MATCH ?))";
                content_args.push(tq.into());
            }
        }
        match o.mode.as_str() {
            "files" => {
                clauses.push(format!("({name_sql})"));
                args.extend(name_args);
            }
            "content" => {
                clauses.push(content_sql.into());
                args.extend(content_args);
            }
            _ => {
                clauses.push(format!("(({name_sql}) OR ({content_sql}))"));
                args.extend(name_args);
                args.extend(content_args);
            }
        }
        let condition = format!(" WHERE {}", clauses.join(" AND "));
        let total = if o.source.is_empty()
            && o.extension.is_empty()
            && name_sql == "1=1"
            && o.mode != "content"
        {
            self.conn
                .query_row("SELECT files FROM catalog_stats WHERE id=1", [], |r| {
                    r.get(0)
                })?
        } else {
            self.conn.query_row(
                &format!("SELECT COUNT(*) FROM files f{condition}"),
                params_from_iter(args.iter()),
                |r| r.get(0),
            )?
        };
        let order = match o.sort.as_str() {
            "size" => "f.size DESC,f.id",
            "modified" => "f.modified DESC,f.id",
            _ => "f.search_name,f.id",
        };
        args.push(o.limit.into());
        args.push(o.offset.into());
        let mut results = self
            .conn
            .prepare(&format!(
                "SELECT {COLUMNS}{FROM}{condition} ORDER BY {order} LIMIT ? OFFSET ?"
            ))?
            .query_map(params_from_iter(args.iter()), file_result)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if o.mode != "files" && self.attached {
            let mut cache = HashMap::new();
            for f in &mut results {
                if !matches!(f.text_status.as_str(), "indexed" | "truncated") {
                    continue;
                }
                let doc:Option<i64>=self.conn.query_row("SELECT l.doc_id FROM content.links l JOIN files f ON f.id=l.file_id WHERE f.id=? AND f.version=l.version",[f.id],|r|r.get(0)).optional()?;
                if let Some(doc) = doc {
                    if let Some(s) = cache.get(&doc) {
                        f.snippet = String::clone(s);
                    } else {
                        let text = self.document_text(doc)?;
                        let s = snippet(&text, query);
                        f.snippet = s.clone();
                        cache.insert(doc, s);
                    }
                }
            }
        }
        Ok(SearchResult {
            results,
            total,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.,
            offset: o.offset,
            limit: o.limit,
        })
    }
    fn document_text(&self, doc: i64) -> Result<String> {
        let payload: Vec<u8> = self.conn.query_row(
            "SELECT payload FROM content.documents WHERE id=?",
            [doc],
            |r| r.get(0),
        )?;
        let mut bytes = Vec::new();
        ZlibDecoder::new(payload.as_slice())
            .take((MAX_TEXT + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_TEXT {
            bail!("Stored document exceeds the text limit");
        }
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
    pub fn file(&mut self, id: i64, offset: usize, max_chars: usize) -> Result<Value> {
        self.check()?;
        if offset > MAX_TEXT || max_chars > MAX_TEXT {
            bail!("Invalid document text page");
        }
        let mut f = self.conn.query_row(
            &format!("SELECT {COLUMNS}{FROM} WHERE f.id=?"),
            [id],
            file_result,
        )?;
        let mut doc = None;
        if matches!(f.text_status.as_str(), "indexed" | "truncated") {
            if !self.ensure_content(false)? {
                f.text_status = "not_indexed".into();
                f.text_detail = "Baza sadržaja nije prisutna u ovoj kopiji kataloga.".into();
            } else {
                doc=self.conn.query_row("SELECT l.doc_id FROM content.links l JOIN files f ON f.id=l.file_id WHERE f.id=? AND f.version=l.version",[id],|r|r.get::<_,i64>(0)).optional()?;
                if doc.is_none() {
                    f.text_status = "not_indexed".into();
                    f.text_detail="Tekst dokumenta nije dostupan u ovoj kopiji baze sadržaja. Ponovo indeksirajte izvor.".into();
                }
            }
        }
        let mut response = json!({"file":f,"text":"","text_status":f.text_status,"text_detail":f.text_detail,"offset":offset,"returned_chars":0,"has_more":doc.is_some()});
        if max_chars > 0 {
            let (selected, total, more) = if let Some(doc) = doc {
                let payload: Vec<u8> = self.conn.query_row(
                    "SELECT payload FROM content.documents WHERE id=?",
                    [doc],
                    |r| r.get(0),
                )?;
                text_page(&payload, offset, max_chars)?
            } else {
                (String::new(), Some(0), false)
            };
            response["returned_chars"] = selected.chars().count().into();
            response["text"] = selected.into();
            if let Some(total) = total {
                response["total_chars"] = total.into();
            }
            response["has_more"] = more.into();
        }
        Ok(response)
    }
    pub fn metadata(&self, id: i64) -> Result<FileResult> {
        self.check()?;
        Ok(self.conn.query_row(
            &format!("SELECT {COLUMNS}{FROM} WHERE f.id=?"),
            [id],
            file_result,
        )?)
    }
    pub fn export(
        &mut self,
        destination: &Path,
        content: bool,
        password: Option<&str>,
    ) -> Result<PathBuf> {
        self.check()?;
        if content {
            self.ensure_content(false)?;
        }
        let encryption = match password {
            Some(p) => Some(Arc::new(Encryption::new(p)?)),
            None => self.encryption.clone(),
        };
        let destination = absolute(destination)?;
        private_dir(&destination).context("Odredišni folder već postoji ili roditeljski folder nije dostupan. Izaberite novo ime za izvoz.")?;
        let mut cleanup = ExportCleanup::new(&destination)?;
        cleanup.create(
            ".allfiles-export-incomplete",
            b"AllFiles: export incomplete\n",
        )?;
        if let Some(e) = &encryption {
            cleanup.create("encryption.json", &serde_json::to_vec_pretty(&e.manifest)?)?;
        }
        let vfs = encryption
            .map(|e| EncryptedVfs::new(&destination, e))
            .transpose()?;
        cleanup.create("catalog.sqlite", b"")?;
        self.conn.execute(
            "VACUUM main INTO ?",
            [uri(&destination.join("catalog.sqlite"), vfs.as_ref())?],
        )?;
        if !content {
            let copy = open_connection(&destination.join("catalog.sqlite"), vfs.as_ref())?;
            copy.execute("UPDATE files SET text_status='not_indexed',text_detail='Izvezen je samo katalog; tekst dokumenata nije uključen.' WHERE text_status IN ('indexed','truncated')",[])?;
        } else if self.attached {
            cleanup.create("content.sqlite", b"")?;
            self.conn.execute(
                "VACUUM content INTO ?",
                [uri(&destination.join("content.sqlite"), vfs.as_ref())?],
            )?;
        }
        for name in ["catalog.sqlite", "content.sqlite"] {
            let path = destination.join(name);
            if !path.exists() {
                continue;
            }
            let copy = open_connection(&path, vfs.as_ref())?;
            let integrity: String = copy.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
            let owner: String =
                copy.query_row("SELECT value FROM meta WHERE key='catalog_id'", [], |r| {
                    r.get(0)
                })?;
            if integrity != "ok" || owner != self.catalog_id {
                bail!("Export verification failed");
            }
            drop(copy);
            OpenOptions::new().write(true).open(&path)?.sync_all()?;
        }
        fs::remove_file(destination.join(".allfiles-export-incomplete"))?;
        #[cfg(unix)]
        File::open(&destination)?.sync_all()?;
        cleanup.complete = true;
        Ok(destination)
    }
}
struct ExportCleanup {
    dir: PathBuf,
    identity: same_file::Handle,
    files: Vec<(PathBuf, same_file::Handle)>,
    complete: bool,
}
impl ExportCleanup {
    fn new(dir: &Path) -> Result<Self> {
        Ok(Self {
            dir: dir.into(),
            identity: same_file::Handle::from_path(dir)?,
            files: Vec::new(),
            complete: false,
        })
    }
    fn create(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        let path = self.dir.join(name);
        let mut file = private_file(&path)?;
        self.files
            .push((path, same_file::Handle::from_file(file.try_clone()?)?));
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(())
    }
}
impl Drop for ExportCleanup {
    fn drop(&mut self) {
        if self.complete
            || same_file::Handle::from_path(&self.dir).ok().as_ref() != Some(&self.identity)
        {
            return;
        }
        for (path, identity) in &self.files {
            if path
                .file_name()
                .is_some_and(|n| n == ".allfiles-export-incomplete")
            {
                continue;
            }
            if same_file::Handle::from_path(path).ok().as_ref() == Some(identity) {
                let _ = fs::remove_file(path);
            }
        }
        if let Ok(entries) = fs::read_dir(&self.dir) {
            let entries = entries.filter_map(|e| e.ok()).collect::<Vec<_>>();
            if entries.len() == 1 && entries[0].file_name() == ".allfiles-export-incomplete" {
                let path = entries[0].path();
                if self.files.iter().any(|(p, id)| {
                    *p == path && same_file::Handle::from_path(&path).ok().as_ref() == Some(id)
                }) {
                    let _ = fs::remove_file(path);
                    let _ = fs::remove_dir(&self.dir);
                }
            }
        }
    }
}
fn file_result(row: &Row<'_>) -> rusqlite::Result<FileResult> {
    let dir: String = row.get(2)?;
    let name: String = row.get(1)?;
    let root: String = row.get(12)?;
    let relative = if dir.is_empty() {
        name.clone()
    } else {
        format!("{}/{name}", dir.trim_end_matches('/'))
    };
    let path = if root.starts_with("import://") || root.starts_with("demo://") {
        relative.clone()
    } else if root.as_bytes().get(1) == Some(&b':') || root.starts_with("\\\\") {
        format!(
            "{}\\{}",
            root.trim_end_matches(['\\', '/']),
            relative.replace('/', "\\")
        )
    } else {
        format!("{}/{}", root.trim_end_matches('/'), relative)
    };
    Ok(FileResult {
        id: row.get(0)?,
        name,
        path,
        relative_path: relative,
        source_id: row.get(3)?,
        source_label: row.get(4)?,
        kind: row.get(5)?,
        location: row.get(6)?,
        size: row.get(7)?,
        modified: row.get(8)?,
        extension: row.get(9)?,
        text_status: row.get(10)?,
        text_detail: row.get(11)?,
        snippet: String::new(),
        root,
    })
}
fn glob_literal(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '[' => "[[]".to_owned(),
            '*' => "[*]".to_owned(),
            '?' => "[?]".to_owned(),
            _ => c.to_string(),
        })
        .collect()
}
fn text_query(query: &str) -> String {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in query.chars() {
        if c == '"' || (matches!(c, ' ' | '\t' | '\n') && !quoted) {
            if !current.is_empty() {
                words.push(format!("\"{}\"", current.replace('"', "\"\"")));
                current.clear();
            }
            if c == '"' {
                quoted = !quoted;
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        words.push(format!("\"{}\"", current.replace('"', "\"\"")));
    }
    words.join(" AND ")
}
fn snippet(text: &str, query: &str) -> String {
    let lower = text.to_lowercase();
    let mut index = 0;
    for term in query.replace('"', "").split_whitespace() {
        if let Some(p) = lower.find(&term.to_lowercase()) {
            index = lower[..p].chars().count();
            break;
        }
    }
    let begin = index.saturating_sub(90);
    let end = index + 200;
    let mut characters = text.chars().skip(begin);
    let selected = characters.by_ref().take(end - begin).collect::<String>();
    let has_more = characters.next().is_some();
    format!(
        "{}{}{}",
        if begin > 0 { "…" } else { "" },
        selected.split_whitespace().collect::<Vec<_>>().join(" "),
        if has_more { "…" } else { "" }
    )
}
