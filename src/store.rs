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
#[path = "fts_tokens.rs"]
mod fts_tokens;
use fts_tokens::Tokenizer;
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
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
fn check_operation(should_cancel: &dyn Fn() -> bool) -> Result<()> {
    if should_cancel() {
        bail!("Operation cancelled or timed out");
    }
    Ok(())
}
fn text_page(
    payload: &[u8],
    offset: usize,
    limit: usize,
    should_cancel: &dyn Fn() -> bool,
) -> Result<(String, Option<usize>, bool)> {
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
        check_operation(should_cancel)?;
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
    let lower = s.to_lowercase();
    if lower.is_ascii() {
        return lower;
    }
    let mut out = String::with_capacity(lower.len());
    for c in lower.chars() {
        match c {
            'č' | 'ć' => out.push('c'),
            'š' => out.push('s'),
            'ž' => out.push('z'),
            'đ' => out.push_str("dj"),
            _ => out.push(c),
        }
    }
    out
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
// Keep the Go-compatible token for ordinary timestamps. Outside i64's
// nanosecond range the seconds disambiguate the wrapped nanoseconds, retaining
// subsecond change detection without changing the on-disk integer columns.
pub fn file_version(size: i64, seconds: i64, nanos: i64) -> String {
    if seconds == nanos.div_euclid(1_000_000_000) {
        format!("{size}:{nanos}")
    } else {
        format!("{size}:{seconds}:{nanos}")
    }
}
pub struct IndexedFile {
    pub id: i64,
    pub relative: String,
    pub version: String,
    pub needs_text: bool,
}
pub struct Content<'a> {
    pub id: i64,
    pub version: &'a str,
    pub status: &'a str,
    pub text: &'a str,
    pub detail: &'a str,
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
    attached: Cell<bool>,
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
    // Without mmap every page-cache miss is a read call (and a decryption for
    // protected catalogs); 64 MiB keeps typical catalogs cached between searches.
    conn.execute_batch("PRAGMA temp_store=MEMORY;PRAGMA trusted_schema=ON;PRAGMA mmap_size=0;PRAGMA cache_size=-65536;")?;
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
fn initialize_stats(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction()?;
    let version: Option<String> = tx
        .query_row("SELECT value FROM meta WHERE key='stats_schema'", [], |r| {
            r.get(0)
        })
        .optional()?;
    if version.as_deref() != Some("2") {
        // Preserve the trigger names used by Go so that reopening a catalog
        // there does not install duplicate counters. Upgrade just once.
        tx.execute_batch(
            "DROP TRIGGER IF EXISTS stats_files_ai; DROP TRIGGER IF EXISTS stats_files_au;",
        )?;
    }
    tx.execute_batch(include_str!("sql/stats.sql"))?;
    if version.as_deref() != Some("2") {
        tx.execute("INSERT INTO meta(key,value) VALUES('stats_schema','2') ON CONFLICT(key) DO UPDATE SET value=excluded.value", [])?;
    }
    tx.commit()?;
    Ok(())
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
        let mut conn = open_connection(&dir.join("catalog.sqlite"), vfs.as_ref())?;
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
        initialize_stats(&mut conn)?;
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
            attached: Cell::new(false),
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
    pub fn ensure_content(&self, create: bool) -> Result<bool> {
        self.check()?;
        if self.attached.get() {
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
            self.conn.execute_batch("SAVEPOINT content_availability")?;
            self.conn
                .execute_batch(include_str!("sql/availability.sql"))?;
            self.conn.execute_batch("RELEASE content_availability")?;
            Ok(())
        })();
        if let Err(e) = result {
            // A cancellation handler must not interrupt rollback/detach too.
            // The request owner clears/reinstalls it around each operation.
            unsafe {
                rusqlite::ffi::sqlite3_progress_handler(
                    self.conn.handle(),
                    0,
                    None,
                    std::ptr::null_mut(),
                );
            }
            let _ = self
                .conn
                .execute_batch("ROLLBACK TO content_availability; RELEASE content_availability");
            let _ = self.conn.execute_batch("DETACH DATABASE content");
            return Err(e);
        }
        self.attached.set(true);
        Ok(true)
    }
    pub fn stats(&self) -> Result<Value> {
        self.check()?;
        let (files, bytes, stored_documents, sources) = self.conn.query_row(
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
        let (documents, mut content_error) = match self.ensure_content(false) {
            Ok(true) => (
                self.conn
                    .query_row("SELECT value FROM temp.available_content_count", [], |r| {
                        r.get::<_, i64>(0)
                    })?,
                String::new(),
            ),
            Ok(false) => (0, String::new()),
            Err(error) => (0, format!("Document content is unavailable: {error}")),
        };
        if content_error.is_empty() && documents < stored_documents {
            content_error = format!(
                "Document text is available for {documents} of {stored_documents} indexed files. Restore the matching content.sqlite or rescan the original media."
            );
        }
        let content_available = content_error.is_empty();
        let demo: Option<String> = self
            .conn
            .query_row("SELECT value FROM meta WHERE key='demo'", [], |r| r.get(0))
            .optional()?;
        let size = |name: &str| self.dir.join(name).metadata().map(|s| s.len()).unwrap_or(0);
        Ok(
            json!({"version":VERSION,"backend":"rust","sqlite_version":rusqlite::version(),"data_dir":self.dir,"demo":demo.as_deref()==Some("true"),"files":files,"bytes":bytes,"documents":documents,"stored_documents":stored_documents,"content_available":content_available,"content_error":content_error,"sources":sources,"catalog_bytes":size("catalog.sqlite"),"content_bytes":size("content.sqlite"),"fts5":true,"encrypted":self.encryption.is_some()}),
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
            "SELECT s.id,s.root,s.label,s.kind,s.location,s.last_scan,s.status,(SELECT COUNT(*) FROM files f WHERE f.source_id=s.id),(SELECT COALESCE(SUM(f.size),0) FROM files f WHERE f.source_id=s.id) FROM sources s{where_sql} ORDER BY s.label,s.id LIMIT ? OFFSET ?"
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
        self.upsert_groups(scan, &[(source, entries)])
    }
    pub fn upsert_groups(
        &mut self,
        scan: &str,
        groups: &[(&str, &[ScannedFile])],
    ) -> Result<Vec<IndexedFile>> {
        self.check()?;
        let has_content = self.ensure_content(false)?;
        let tx = self.conn.transaction()?;
        let mut out = Vec::with_capacity(groups.iter().map(|(_, files)| files.len()).sum());
        {
            // Prepare once per batch; per-row preparation dominated large imports.
            let mut find_dir =
                tx.prepare("SELECT id FROM directories WHERE source_id=? AND path=?")?;
            let mut add_dir =
                tx.prepare("INSERT INTO directories(source_id,path,search_path) VALUES(?,?,?)")?;
            let mut find_file =
                tx.prepare("SELECT id,version,text_status FROM files WHERE dir_id=? AND name=?")?;
            let mut update_file = tx.prepare("UPDATE files SET size=?,modified=?,modified_ns=?,version=?,seen_scan=?,text_status=CASE WHEN version!=? THEN 'not_indexed' ELSE text_status END,text_detail=CASE WHEN version!=? THEN '' ELSE text_detail END WHERE id=?")?;
            let mut add_file = tx.prepare("INSERT INTO files(source_id,dir_id,name,search_name,extension,size,modified,modified_ns,version,seen_scan) VALUES(?,?,?,?,?,?,?,?,?,?)")?;
            let mut unsupported =
                tx.prepare("UPDATE files SET text_status='unsupported',text_detail='' WHERE id=?")?;
            let mut linked = if has_content {
                Some(tx.prepare("SELECT count(*) FROM content.links l JOIN content.documents d ON d.id=l.doc_id WHERE l.file_id=? AND l.version=?")?)
            } else {
                None
            };
            for &(source, entries) in groups {
                // Directory IDs are scoped to a source, even when two media
                // have exactly the same relative folder and filename.
                let mut dirs = HashMap::new();
                for f in entries {
                    let dir = if f.dir == "." {
                        String::new()
                    } else {
                        f.dir.replace('\\', "/")
                    };
                    let did = if let Some(d) = dirs.get(&dir) {
                        *d
                    } else {
                        let d = match find_dir
                            .query_row(params![source, dir], |r| r.get::<_, i64>(0))
                            .optional()?
                        {
                            Some(d) => d,
                            None => {
                                add_dir.execute(params![source, dir, fold(&dir)])?;
                                tx.last_insert_rowid()
                            }
                        };
                        dirs.insert(dir, d);
                        d
                    };
                    let version = file_version(f.size, f.modified, f.modified_ns);
                    let old: Option<(i64, String, String)> = find_file
                        .query_row(params![did, f.name], |r| {
                            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                        })
                        .optional()?;
                    let fresh = old.is_none();
                    let (id, previous, status) = if let Some((id, previous, status)) = old {
                        update_file.execute(params![
                            f.size,
                            f.modified,
                            f.modified_ns,
                            version,
                            scan,
                            version,
                            version,
                            id
                        ])?;
                        (id, previous, status)
                    } else {
                        add_file.execute(params![
                            source,
                            did,
                            f.name,
                            fold(&f.name),
                            f.extension.trim_start_matches('.').to_lowercase(),
                            f.size,
                            f.modified,
                            f.modified_ns,
                            version,
                            scan
                        ])?;
                        (
                            tx.last_insert_rowid(),
                            String::new(),
                            "not_indexed".to_owned(),
                        )
                    };
                    let candidate = crate::extract::candidate(&f.extension);
                    if !candidate && (fresh || previous != version) {
                        unsupported.execute([id])?;
                    }
                    let expects = matches!(status.as_str(), "indexed" | "truncated");
                    let mut needs = candidate
                        && (fresh
                            || previous != version
                            || status == "not_indexed"
                            || status == "error"
                            || (!has_content && expects));
                    if let Some(linked) = linked.as_mut()
                        && candidate
                        && expects
                        && !needs
                    {
                        needs =
                            linked.query_row(params![id, version], |r| r.get::<_, i64>(0))? == 0;
                    }
                    out.push(IndexedFile {
                        id,
                        relative: f.relative.clone(),
                        version,
                        needs_text: needs,
                    });
                }
            }
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
        self.record_contents(&[Content {
            id,
            version,
            status,
            text,
            detail,
        }])
    }
    // Store extracted text for several files in one transaction. With
    // synchronous=FULL each commit syncs both databases and a super-journal,
    // which dominated scans of many small documents.
    pub fn record_contents(&mut self, items: &[Content<'_>]) -> Result<()> {
        self.check()?;
        if items.iter().any(|c| c.text.len() > MAX_TEXT) {
            bail!("Document text limit exceeded");
        }
        let has_text =
            |c: &Content<'_>| !c.text.is_empty() && matches!(c.status, "indexed" | "truncated");
        if items.iter().any(has_text) || self.attached.get() {
            self.ensure_content(true)?;
        }
        let attached = self.attached.get();
        let tx = self.conn.transaction()?;
        {
            let mut current = tx.prepare("SELECT version FROM files WHERE id=?")?;
            let mut status =
                tx.prepare("UPDATE files SET text_status=?,text_detail=? WHERE id=?")?;
            let mut content = if attached {
                Some((
                    tx.prepare("DELETE FROM content.links WHERE file_id=?")?,
                    tx.prepare("SELECT id FROM content.documents WHERE hash=?")?,
                    tx.prepare(
                        "INSERT INTO content.documents(hash,payload,plain_size) VALUES(?,?,?)",
                    )?,
                    tx.prepare("INSERT INTO content.fts(rowid,body) VALUES(?,?)")?,
                    tx.prepare("INSERT INTO content.links(file_id,version,doc_id) VALUES(?,?,?)")?,
                ))
            } else {
                None
            };
            for c in items {
                let version: String = current.query_row([c.id], |r| r.get(0))?;
                if version != c.version {
                    bail!("Dokument je izmenjen tokom indeksiranja");
                }
                if let Some((unlink, find, insert, index, link)) = content.as_mut() {
                    unlink.execute([c.id])?;
                    if has_text(c) {
                        let hash = hex::encode(Sha256::digest(c.text.as_bytes()));
                        let doc = match find.query_row([&hash], |r| r.get(0)).optional()? {
                            Some(doc) => doc,
                            None => {
                                let mut z = ZlibEncoder::new(Vec::new(), Compression::default());
                                z.write_all(c.text.as_bytes())?;
                                insert.execute(params![hash, z.finish()?, c.text.len() as i64])?;
                                let doc = tx.last_insert_rowid();
                                index.execute(params![doc, c.text])?;
                                doc
                            }
                        };
                        link.execute(params![c.id, c.version, doc])?;
                    }
                }
                status.execute(params![c.status, c.detail, c.id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
    pub fn finish_scan(&mut self, source: &str, scan: &str, complete: bool) -> Result<()> {
        self.finish_scans(&[source], scan, complete)
    }
    // Close several sources of one scan or import in a single transaction;
    // the catalog-wide content cleanup then runs once instead of per source.
    pub fn finish_scans(&mut self, sources: &[&str], scan: &str, complete: bool) -> Result<()> {
        self.check()?;
        self.ensure_content(false)?;
        let attached = self.attached.get();
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let tx = self.conn.transaction()?;
        for source in sources {
            if complete {
                if attached {
                    tx.execute("DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE source_id=? AND seen_scan!=?)",params![source,scan])?;
                }
                tx.execute(
                    "DELETE FROM files WHERE source_id=? AND seen_scan!=?",
                    params![source, scan],
                )?;
                tx.execute("DELETE FROM directories WHERE source_id=? AND id NOT IN(SELECT dir_id FROM files WHERE source_id=?)",params![source,source])?;
            }
            tx.execute(
                "UPDATE sources SET status=?,last_scan=? WHERE id=?",
                params![if complete { "ready" } else { "partial" }, now, source],
            )?;
        }
        if attached {
            tx.execute_batch("DELETE FROM content.links WHERE file_id IN(SELECT id FROM files WHERE text_status NOT IN ('indexed','truncated'));DELETE FROM content.fts WHERE rowid IN(SELECT id FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links));DELETE FROM content.documents WHERE id NOT IN(SELECT doc_id FROM content.links);")?;
        }
        tx.commit()?;
        Ok(())
    }
    #[cfg(test)]
    pub fn search(&mut self, o: SearchOptions) -> Result<SearchResult> {
        self.search_with_cancel(o, &|| false)
    }
    pub fn search_with_cancel(
        &mut self,
        mut o: SearchOptions,
        should_cancel: &dyn Fn() -> bool,
    ) -> Result<SearchResult> {
        let start = Instant::now();
        check_operation(should_cancel)?;
        self.check()?;
        if o.query.chars().take(1001).count() > 1000 {
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
        // Content-only searches never use name conditions; skip their probes.
        let mut terms = if o.mode == "content" {
            Vec::new()
        } else {
            query
                .replace('"', "")
                .split_whitespace()
                .map(fold)
                .collect::<Vec<_>>()
        };
        let mut distinct = HashSet::new();
        terms.retain(|term| distinct.insert(term.clone()));
        // Trigram lookups are fast for selective terms, but a broad term makes
        // SQLite verify and sort nearly every row. Probe each term and scan the
        // ordered name index instead when the trigram candidate set is large.
        let threshold = if terms.is_empty() {
            0
        } else {
            let files: i64 =
                self.conn
                    .query_row("SELECT files FROM catalog_stats WHERE id=1", [], |r| {
                        r.get(0)
                    })?;
            (files / 16).clamp(1000, 50_000)
        };
        for term in &terms {
            check_operation(should_cancel)?;
            let g = format!("*{}*", glob_literal(term));
            names.push(if self.selective(term, &g, threshold)? {
                "f.id IN(SELECT rowid FROM names WHERE search_name GLOB ? UNION SELECT pf.id FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))"
            } else {
                "(f.search_name GLOB ? OR f.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?))"
            });
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
            content_sql = "f.id IN(SELECT file_id FROM temp.available_content)";
            let tq = text_query(query);
            if !tq.is_empty() {
                content_sql = "f.id IN(SELECT l.file_id FROM content.links l JOIN temp.available_content a ON a.file_id=l.file_id WHERE l.doc_id IN(SELECT rowid FROM content.fts WHERE fts MATCH ?))";
                content_args.push(tq.into());
            }
        }
        let everything = name_sql == "1=1" && o.mode != "content";
        match o.mode.as_str() {
            "files" => {
                clauses.push(format!("({name_sql})"));
                args.extend(name_args);
            }
            "content" => {
                clauses.push(content_sql.into());
                args.extend(content_args);
            }
            // An empty name filter already matches every file.
            _ if name_sql == "1=1" => {}
            _ => {
                clauses.push(format!("(({name_sql}) OR ({content_sql}))"));
                args.extend(name_args);
                args.extend(content_args);
            }
        }
        let condition = format!(" WHERE {}", clauses.join(" AND "));
        let order = match o.sort.as_str() {
            "size" => "f.size DESC,f.id",
            "modified" => "f.modified DESC,f.id",
            _ => "f.search_name,f.id",
        };
        let (total, ids) = if o.source.is_empty() && o.extension.is_empty() && everything {
            // Page through the ordered index first and join metadata for the page only.
            let total =
                self.conn
                    .query_row("SELECT files FROM catalog_stats WHERE id=1", [], |r| {
                        r.get(0)
                    })?;
            let ids = self
                .conn
                .prepare(&format!(
                    "SELECT f.id FROM files f ORDER BY {order} LIMIT ? OFFSET ?"
                ))?
                .query_map(params![o.limit, o.offset], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            (total, ids)
        } else {
            // One ordered pass yields both the total and the requested page,
            // so the match conditions are evaluated once instead of twice.
            let mut statement = self.conn.prepare(&format!(
                "SELECT f.id FROM files f{condition} ORDER BY {order}"
            ))?;
            let mut rows = statement.query(params_from_iter(args.iter()))?;
            let (mut total, mut ids) = (0i64, Vec::new());
            while let Some(row) = rows.next()? {
                if total % 256 == 0 {
                    check_operation(should_cancel)?;
                }
                if total >= o.offset && total - o.offset < o.limit {
                    ids.push(row.get::<_, i64>(0)?);
                }
                total += 1;
            }
            (total, ids)
        };
        let mut results = if ids.is_empty() {
            Vec::new()
        } else {
            self.conn
                .prepare(&format!(
                    "SELECT {COLUMNS}{FROM} WHERE f.id IN({}) ORDER BY {order}",
                    vec!["?"; ids.len()].join(",")
                ))?
                .query_map(params_from_iter(ids.iter()), file_result)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        check_operation(should_cancel)?;
        let has_indexed = results
            .iter()
            .any(|f| matches!(f.text_status.as_str(), "indexed" | "truncated"));
        // Metadata remains useful when content is absent or corrupt. Only the
        // response changes; restoring the matching database restores access.
        let available = has_indexed && self.ensure_content(false).unwrap_or(false);
        let mut documents = HashMap::<i64, i64>::new();
        if available && !ids.is_empty() {
            documents = self.conn.prepare(&format!("SELECT l.file_id,l.doc_id FROM content.links l JOIN temp.available_content a ON a.file_id=l.file_id WHERE l.file_id IN({})", vec!["?"; ids.len()].join(",")))?
                .query_map(params_from_iter(ids.iter()), |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<HashMap<_, _>>>()?;
        }
        for f in &mut results {
            if matches!(f.text_status.as_str(), "indexed" | "truncated")
                && !documents.contains_key(&f.id)
            {
                f.text_status = "not_indexed".into();
                f.text_detail = "Document text is unavailable in this copy. Restore the matching content database or rescan the original media.".into();
            }
        }
        if o.mode != "files" && available {
            let tokenizer = Tokenizer::new(&self.conn)?;
            let terms = tokenizer.terms(query)?;
            let empty_terms = HashSet::new();
            let mut matched_documents: HashSet<i64> = documents.values().copied().collect();
            if o.mode == "all" && !terms.is_empty() && !matched_documents.is_empty() {
                let doc_ids = matched_documents.iter().copied().collect::<Vec<_>>();
                let mut bindings = vec![SqlValue::Text(text_query(query))];
                bindings.extend(doc_ids.iter().map(|id| SqlValue::Integer(*id)));
                matched_documents = self
                    .conn
                    .prepare(&format!(
                        "SELECT rowid FROM content.fts WHERE fts MATCH ? AND rowid IN({})",
                        vec!["?"; doc_ids.len()].join(",")
                    ))?
                    .query_map(params_from_iter(bindings.iter()), |row| row.get(0))?
                    .collect::<rusqlite::Result<HashSet<_>>>()?;
            }
            let mut cache: HashMap<i64, String> = HashMap::new();
            let mut payloads = self
                .conn
                .prepare("SELECT payload FROM content.documents WHERE id=?")?;
            for f in &mut results {
                check_operation(should_cancel)?;
                if let Some(&doc) = documents.get(&f.id) {
                    if let Some(s) = cache.get(&doc) {
                        f.snippet = s.clone();
                    } else {
                        let payload: Vec<u8> = payloads.query_row([doc], |r| r.get(0))?;
                        // A filename-only result needs just its opening preview;
                        // searching every byte for an absent term cannot help.
                        let selected_terms = if matched_documents.contains(&doc) {
                            &terms
                        } else {
                            &empty_terms
                        };
                        let s = snippet(&payload, selected_terms, &tokenizer, should_cancel)?;
                        f.snippet = s.clone();
                        cache.insert(doc, s);
                    }
                }
            }
        }
        check_operation(should_cancel)?;
        Ok(SearchResult {
            results,
            total,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.,
            offset: o.offset,
            limit: o.limit,
        })
    }
    // True when a term's trigram candidates stay below `limit`. Terms without
    // three literal characters cannot use the trigram index and always scan.
    fn selective(&self, term: &str, glob: &str, limit: i64) -> Result<bool> {
        if !term
            .split(['[', '*', '?'])
            .any(|run| run.chars().count() >= 3)
        {
            return Ok(false);
        }
        let candidates: i64 = self.conn.query_row(
            "SELECT (SELECT count(*) FROM (SELECT 1 FROM names WHERE search_name GLOB ?1 LIMIT ?2))+(SELECT count(*) FROM (SELECT 1 FROM files pf WHERE pf.dir_id IN(SELECT rowid FROM paths WHERE search_path GLOB ?1) LIMIT ?2))",
            params![glob, limit],
            |r| r.get(0),
        )?;
        Ok(candidates < limit)
    }
    #[cfg(test)]
    pub fn file(&mut self, id: i64, offset: usize, max_chars: usize) -> Result<Value> {
        self.file_with_cancel(id, offset, max_chars, &|| false)
    }
    pub fn file_with_cancel(
        &mut self,
        id: i64,
        offset: usize,
        max_chars: usize,
        should_cancel: &dyn Fn() -> bool,
    ) -> Result<Value> {
        check_operation(should_cancel)?;
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
            match self.ensure_content(false) {
                Ok(true) => {
                    doc=self.conn.query_row("SELECT l.doc_id FROM content.links l JOIN temp.available_content a ON a.file_id=l.file_id WHERE a.file_id=?",[id],|r|r.get::<_,i64>(0)).optional()?;
                    if doc.is_none() {
                        f.text_status = "not_indexed".into();
                        f.text_detail="Tekst dokumenta nije dostupan u ovoj kopiji baze sadržaja. Ponovo indeksirajte izvor.".into();
                    }
                }
                Ok(false) => {
                    f.text_status = "not_indexed".into();
                    f.text_detail = "Baza sadržaja nije prisutna u ovoj kopiji kataloga.".into();
                }
                Err(error) => {
                    check_operation(should_cancel)?;
                    f.text_status = "not_indexed".into();
                    f.text_detail = format!("Document text is unavailable: {error}");
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
                text_page(&payload, offset, max_chars, should_cancel)?
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
        check_operation(should_cancel)?;
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
        } else if self.attached.get() {
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
        if c == '"' || (c.is_whitespace() && !quoted) {
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
    let mut distinct = HashSet::new();
    words.retain(|word| distinct.insert(word.clone()));
    words.join(" AND ")
}
// Decode incrementally and use the same unicode61 token boundaries and byte
// offsets as the content index, including punctuation and combining marks.
fn snippet(
    payload: &[u8],
    terms: &HashSet<Vec<u8>>,
    tokenizer: &Tokenizer,
    should_cancel: &dyn Fn() -> bool,
) -> Result<String> {
    const BEFORE: usize = 90;
    const AFTER: usize = 200;
    if payload.len() > MAX_TEXT + 65536 {
        bail!("Stored document exceeds compressed size limit");
    }
    let mut decoder = ZlibDecoder::new(payload);
    let mut bytes = vec![0u8; if terms.is_empty() { 2048 } else { 65536 }];
    let (mut carry, mut text) = (Vec::new(), String::new());
    let (mut total_bytes, mut resume, mut found) = (0, 0, None::<usize>);
    let mut next_token_scan = 0;
    loop {
        check_operation(should_cancel)?;
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
        text.push_str(std::str::from_utf8(&carry[..valid])?);
        carry.drain(..valid);
        if found.is_none() && !terms.is_empty() && (n == 0 || text.len() >= next_token_scan) {
            let base = resume;
            let chunk = text
                .get(base..)
                .ok_or_else(|| anyhow::anyhow!("Invalid tokenizer offset"))?;
            resume = text.len();
            if terms.len() <= 4 && chunk.is_ascii() {
                // unicode61's default ASCII tokens contain only letters and
                // digits. Searching a few terms directly avoids one FFI
                // callback/hash lookup per word in long ASCII documents.
                // Defer the final token: the next chunk can extend it with an
                // ASCII letter, Unicode letter, or combining mark.
                let end = if n == 0 {
                    chunk.len()
                } else {
                    chunk
                        .bytes()
                        .rposition(|b| !b.is_ascii_alphanumeric())
                        .map_or(0, |i| i + 1)
                };
                resume = base + end;
                let normalized = chunk[..end].to_ascii_lowercase();
                for term in terms.iter().filter(|term| term.is_ascii()) {
                    let term = std::str::from_utf8(term)?;
                    for (position, _) in normalized.match_indices(term) {
                        let before = position.checked_sub(1).map(|i| normalized.as_bytes()[i]);
                        let after = normalized.as_bytes().get(position + term.len());
                        if before.is_none_or(|b| !b.is_ascii_alphanumeric())
                            && after.is_none_or(|b| !b.is_ascii_alphanumeric())
                        {
                            let position = base + position;
                            found = Some(found.map_or(position, |old| old.min(position)));
                            break;
                        }
                    }
                }
            } else {
                tokenizer.visit(chunk, &mut |token, start, end| {
                    // A token at the chunk boundary may continue in the next read.
                    if end == chunk.len() && n > 0 {
                        resume = base + start;
                    } else if terms.contains(token) {
                        found = Some(base + start);
                        return true;
                    }
                    false
                })?;
            }
            // A very long unbroken word must not be re-tokenized after every
            // 64 KiB read. Double its pending length before retrying, keeping
            // total tokenization linear while still checking cancellation on
            // every decode. EOF always processes the remaining tail.
            let pending = text.len() - resume;
            next_token_scan = if pending >= bytes.len() {
                text.len() + pending
            } else {
                0
            };
        }
        check_operation(should_cancel)?;
        let done = match found {
            Some(position) => text[position..].chars().nth(AFTER).is_some(),
            None => terms.is_empty() && text.chars().nth(AFTER).is_some(),
        };
        if n == 0 || done {
            break;
        }
    }
    let index = found.map_or(0, |position| text[..position].chars().count());
    let begin = index.saturating_sub(BEFORE);
    let mut characters = text.chars().skip(begin);
    let selected = characters
        .by_ref()
        .take(index + AFTER - begin)
        .collect::<String>();
    let has_more = characters.next().is_some();
    Ok(format!(
        "{}{}{}",
        if begin > 0 { "…" } else { "" },
        selected.split_whitespace().collect::<Vec<_>>().join(" "),
        if has_more { "…" } else { "" }
    ))
}
