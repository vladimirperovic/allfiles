use crate::{
    crypto,
    import::{self, ImportOptions},
    scanner::{self, Progress, SharedStore},
    store::{FileResult, MAX_TEXT, ScanOptions, SearchOptions, Store, VERSION},
};
use anyhow::{Result, bail};
use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

const MAX_QUERY_REQUESTS: usize = 16;

#[derive(Default)]
struct Jobs {
    progress: Option<Progress>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    stopping: bool,
    exporting: bool,
    revision: u64,
}
pub struct App {
    pub store: SharedStore,
    base: String,
    token: String,
    instance_id: String,
    stats_cache: Mutex<Value>,
    query_slots: Arc<tokio::sync::Semaphore>,
    jobs: Mutex<Jobs>,
    stop_lock: Mutex<()>,
    shutdown: tokio::sync::Notify,
}
impl App {
    fn new(store: SharedStore, base: String) -> Result<Arc<Self>> {
        let stats = store.lock().unwrap().stats()?;
        Ok(Arc::new(Self {
            store,
            base,
            token: crypto::random_id(),
            instance_id: crypto::random_id(),
            stats_cache: Mutex::new(stats),
            query_slots: Arc::new(tokio::sync::Semaphore::new(MAX_QUERY_REQUESTS)),
            jobs: Mutex::new(Jobs::default()),
            stop_lock: Mutex::new(()),
            shutdown: tokio::sync::Notify::new(),
        }))
    }
    fn refresh_stats(&self) -> Result<()> {
        let store = self
            .store
            .lock()
            .map_err(|_| anyhow::anyhow!("Store is unavailable"))?;
        let stats = store.stats()?;
        *self.stats_cache.lock().unwrap() = stats;
        Ok(())
    }
    fn refresh_status_cache(&self) -> Result<bool> {
        // A long query/export must not hide job progress or cancellation. The
        // counters are a recent snapshot while the store is busy, refreshed
        // before a job reports completion and whenever an idle poll can read.
        match self.store.try_lock() {
            Ok(store) => {
                let stats = store.stats()?;
                *self.stats_cache.lock().unwrap() = stats;
                Ok(false)
            }
            Err(TryLockError::WouldBlock) => Ok(true),
            Err(TryLockError::Poisoned(_)) => bail!("Store is unavailable"),
        }
    }
    fn status_snapshot(&self, cached: bool) -> Result<Value> {
        let jobs = self.jobs.lock().unwrap();
        // A worker can finish after refresh_status_cache releases the store.
        // Its counters are published before its terminal job revision, so read
        // the latest cache only after taking the job snapshot lock.
        let mut stats = self.stats_cache.lock().unwrap().clone();
        stats["job"] = serde_json::to_value(&jobs.progress)?;
        stats["job_revision"] = jobs.revision.into();
        stats["instance_id"] = self.instance_id.clone().into();
        stats["stats_cached"] = cached.into();
        stats["exporting"] = jobs.exporting.into();
        Ok(stats)
    }
    fn status(&self) -> Result<Value> {
        self.status_snapshot(self.refresh_status_cache()?)
    }
    fn reserve_export(self: &Arc<Self>) -> Result<ExportReservation> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.stopping
            || jobs.exporting
            || jobs
                .progress
                .as_ref()
                .is_some_and(|p| p.status == "running")
        {
            bail!("Sačekajte da se indeksiranje ili izvoz završi pre izvoza");
        }
        jobs.exporting = true;
        Ok(ExportReservation(self.clone()))
    }
    fn start(
        self: &Arc<Self>,
        work: impl FnOnce(&SharedStore, &AtomicBool, &dyn Fn(Progress)) -> Result<()> + Send + 'static,
    ) -> Result<(Progress, u64)> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.stopping {
            bail!("Aplikacija se zatvara");
        }
        if jobs.exporting {
            bail!("Sačekajte da se izvoz završi pre indeksiranja");
        }
        if jobs
            .progress
            .as_ref()
            .is_some_and(|p| p.status == "running")
        {
            bail!("Indeksiranje je već u toku");
        }
        if let Some(old) = jobs.thread.take() {
            old.join()
                .map_err(|_| anyhow::anyhow!("Previous worker failed"))?;
        }
        let initial = Progress {
            id: crypto::random_id(),
            status: "running".into(),
            phase: "Priprema".into(),
            ..Default::default()
        };
        jobs.progress = Some(initial.clone());
        jobs.revision += 1;
        let revision = jobs.revision;
        jobs.cancel = Arc::new(AtomicBool::new(false));
        let cancel = jobs.cancel.clone();
        let app = self.clone();
        let id = initial.id.clone();
        jobs.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work(&app.store, &cancel, &|mut p| {
                    p.id = id.clone();
                    p.status = "running".into();
                    let mut jobs = app.jobs.lock().unwrap();
                    jobs.progress = Some(p);
                    jobs.revision += 1;
                })
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Indexing worker failed")));
            let result = result.and(app.refresh_stats());
            let mut jobs = app.jobs.lock().unwrap();
            let p = jobs.progress.as_mut().unwrap();
            p.current.clear();
            if cancel.load(Ordering::Relaxed) {
                p.status = "cancelled".into();
                p.phase = "Zaustavljeno".into();
            } else if let Err(e) = result {
                p.status = "failed".into();
                p.phase = "Nije završeno".into();
                p.error = e.to_string();
            } else {
                p.status = "completed".into();
                p.phase = "Završeno".into();
            }
            jobs.revision += 1;
        }));
        Ok((initial, revision))
    }
    fn stop(&self) {
        let _stop_guard = self.stop_lock.lock().unwrap();
        let worker = {
            let mut j = self.jobs.lock().unwrap();
            j.stopping = true;
            j.cancel.store(true, Ordering::Relaxed);
            j.thread.take()
        };
        if let Some(t) = worker {
            let _ = t.join();
        }
    }
}
struct ExportReservation(Arc<App>);
impl Drop for ExportReservation {
    fn drop(&mut self) {
        if let Ok(mut jobs) = self.0.jobs.lock() {
            jobs.exporting = false;
        }
    }
}
#[derive(Clone)]
struct RequestControl {
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl RequestControl {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() + Duration::from_secs(15),
        }
    }
    fn stopped(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed) || Instant::now() >= self.deadline
    }
    fn check(&self) -> Result<()> {
        if self.stopped() {
            bail!("Operation cancelled or timed out");
        }
        Ok(())
    }
    fn lock_store<'a>(&self, store: &'a SharedStore) -> Result<MutexGuard<'a, Store>> {
        loop {
            self.check()?;
            match store.try_lock() {
                Ok(guard) => return Ok(guard),
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(2)),
                Err(TryLockError::Poisoned(_)) => bail!("Store is unavailable"),
            }
        }
    }
}
struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}
fn query_store<T>(
    app: &App,
    control: &RequestControl,
    query: impl FnOnce(&mut Store) -> Result<T>,
) -> Result<T> {
    let mut store = control.lock_store(&app.store)?;
    control.check()?;
    let progress = control.clone();
    store
        .conn
        .progress_handler(1000, Some(move || progress.stopped()))?;
    let result = query(&mut store);
    store.conn.progress_handler(0, None::<fn() -> bool>)?;
    control.check()?;
    result
}
async fn run_api(
    app: Arc<App>,
    method: String,
    path: String,
    query: String,
    bytes: Bytes,
) -> Response {
    let control = RequestControl::new();
    // Permit ownership follows the blocking work, not the client connection.
    // A disconnected client cancels that work before its permit is released.
    let limited_query = matches!(path.as_str(), "/api/search" | "/api/file");
    let permit = if limited_query {
        match app.query_slots.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                return response(
                    503,
                    json!({"error":"Too many active searches; retry shortly"}),
                );
            }
        }
    } else {
        None
    };
    let _cancel = CancelOnDrop(control.cancelled.clone());
    let work_control = control.clone();
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        api(&app, &method, &path, &query, &bytes, &work_control)
    })
    .await;
    match task {
        Ok(Ok((code, value))) => response(code, value),
        Ok(Err(error)) => {
            let code = if limited_query && control.stopped() {
                408
            } else if error
                .downcast_ref::<rusqlite::Error>()
                .is_some_and(|e| matches!(e, rusqlite::Error::QueryReturnedNoRows))
                || error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            {
                404
            } else {
                400
            };
            response(code, json!({"error":error.to_string()}))
        }
        Err(_) => response(500, json!({"error":"Internal worker failure"})),
    }
}
fn response(code: u16, v: Value) -> Response {
    (StatusCode::from_u16(code).unwrap(), axum::Json(v)).into_response()
}
async fn handler(State(app): State<Arc<App>>, request: Request) -> Response {
    let headers = request.headers();
    let host = headers
        .get(header::HOST)
        .and_then(|x| x.to_str().ok())
        .unwrap_or("");
    let origin = headers.get(header::ORIGIN).and_then(|x| x.to_str().ok());
    let denied = host != app.base.trim_start_matches("http://")
        || origin.is_some_and(|s| s != app.base)
        || headers
            .get("sec-fetch-site")
            .is_some_and(|s| s == "cross-site");
    let path = request.uri().path().to_owned();
    let method = request.method().as_str().to_owned();
    let mut result = if denied {
        response(
            403,
            json!({"error":"Pristup dozvoljen samo lokalnoj aplikaciji"}),
        )
    } else if path.starts_with("/api/") {
        if method != "GET" && method != "POST" {
            response(405, json!({"error":"Method not allowed"}))
        } else if method == "POST"
            && headers
                .get("x-allfiles-token")
                .and_then(|s| s.to_str().ok())
                != Some(app.token.as_str())
        {
            response(
                403,
                json!({"error":"Sesija nije važeća. Osvežite stranicu."}),
            )
        } else {
            let query = request.uri().query().unwrap_or("").to_owned();
            match tokio::time::timeout(
                Duration::from_secs(30),
                to_bytes(request.into_body(), 1 << 20),
            )
            .await
            {
                Ok(Ok(bytes)) => run_api(app, method, path, query, bytes).await,
                _ => response(413, json!({"error":"Request exceeded size or time limit"})),
            }
        }
    } else if method == "GET" || method == "HEAD" {
        asset(&path, method == "HEAD")
    } else {
        response(405, json!({"error":"Method not allowed"}))
    };
    for (key, val) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("cross-origin-resource-policy", "same-origin"),
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; object-src 'none'; base-uri 'none'",
        ),
    ] {
        result.headers_mut().insert(
            header::HeaderName::from_static(key),
            header::HeaderValue::from_static(val),
        );
    }
    result
}
fn asset(path: &str, head: bool) -> Response {
    let (data, mime): (&'static [u8], &str) = match path {
        "/" | "/index.html" => (
            include_bytes!("../web/index.html"),
            "text/html; charset=utf-8",
        ),
        "/app.js" => (
            include_bytes!("../web/app.js"),
            "text/javascript; charset=utf-8",
        ),
        "/i18n.js" => (
            include_bytes!("../web/i18n.js"),
            "text/javascript; charset=utf-8",
        ),
        "/particles.js" => (
            include_bytes!("../web/particles.js"),
            "text/javascript; charset=utf-8",
        ),
        "/style.css" => (
            include_bytes!("../web/style.css"),
            "text/css; charset=utf-8",
        ),
        "/logo.svg" => (include_bytes!("../web/logo.svg"), "image/svg+xml"),
        _ => return response(404, json!({"error":"Not found"})),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, data.len())
        .body(if head {
            Body::empty()
        } else {
            Body::from(data)
        })
        .unwrap()
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    Ok(serde_json::from_slice(bytes)?)
}
fn number(q: &HashMap<String, String>, key: &str, default: i64) -> Result<i64> {
    q.get(key)
        .map(|s| s.parse().map_err(Into::into))
        .unwrap_or(Ok(default))
}
fn api(
    app: &Arc<App>,
    method: &str,
    path: &str,
    query: &str,
    bytes: &[u8],
    control: &RequestControl,
) -> Result<(u16, Value)> {
    let post = matches!(
        path,
        "/api/scan" | "/api/import" | "/api/cancel" | "/api/export" | "/api/open" | "/api/shutdown"
    );
    if (post && method != "POST") || (!post && method != "GET") {
        return Ok((405, json!({"error":"Method not allowed"})));
    }
    let q: HashMap<String, String> = url::form_urlencoded::parse(query.as_bytes())
        .into_owned()
        .collect();
    let val = |key: &str| q.get(key).cloned().unwrap_or_default();
    let v = match path {
        "/api/session" => json!({"token":app.token,"instance_id":app.instance_id}),
        "/api/status" => app.status()?,
        "/api/sources" => {
            let s = app.store.lock().unwrap();
            if q.contains_key("id") || q.contains_key("offset") || q.contains_key("limit") {
                let offset = number(&q, "offset", 0)?;
                let limit = number(&q, "limit", 100)?;
                let (sources, total) = s.sources(&val("id"), offset, limit)?;
                json!({"has_more":offset+(sources.len()as i64)<total,"sources":sources,"total":total,"offset":offset,"limit":limit})
            } else {
                json!({"sources":s.all_sources()?})
            }
        }
        "/api/search" => {
            let o = SearchOptions {
                query: val("q"),
                mode: val("mode"),
                source: val("source"),
                extension: val("ext"),
                sort: val("sort"),
                offset: number(&q, "offset", 0)?,
                limit: number(&q, "limit", 0)?,
            };
            serde_json::to_value(query_store(app, control, |s| {
                s.search_with_cancel(o, &|| control.stopped())
            })?)?
        }
        "/api/file" => {
            let offset = number(&q, "text_offset", 0)?;
            let paged = q.contains_key("text_offset") || q.contains_key("max_chars");
            let max = number(&q, "max_chars", if paged { 4000 } else { MAX_TEXT as i64 })?;
            if !(0..=MAX_TEXT as i64).contains(&offset)
                || max < 0
                || max > if paged { 20000 } else { MAX_TEXT as i64 }
            {
                bail!("Invalid text page");
            }
            let id = number(&q, "id", 0)?;
            query_store(app, control, |s| {
                s.file_with_cancel(id, offset as usize, max as usize, &|| control.stopped())
            })?
        }
        "/api/scan" => {
            let o: ScanOptions = decode(bytes)?;
            if !Path::new(&o.root).is_absolute() {
                bail!("Unesite apsolutnu putanju foldera");
            }
            let (p, revision) = app.start(move |s, c, r| scanner::scan(s, o, c, r))?;
            return Ok((
                202,
                json!({"job":p,"job_revision":revision,"instance_id":app.instance_id}),
            ));
        }
        "/api/import" => {
            let o: ImportOptions = decode(bytes)?;
            let (p, revision) = app.start(move |s, c, r| import::import(s, o, c, r))?;
            return Ok((
                202,
                json!({"job":p,"job_revision":revision,"instance_id":app.instance_id}),
            ));
        }
        "/api/cancel" => {
            app.jobs
                .lock()
                .unwrap()
                .cancel
                .store(true, Ordering::Relaxed);
            json!({"ok":true})
        }
        "/api/export" => {
            #[derive(Deserialize, Default)]
            #[serde(default, deny_unknown_fields)]
            struct Export {
                destination: String,
                include_content: bool,
                encrypt: bool,
                password: String,
            }
            let mut o: Export = decode(bytes)?;
            let password = Zeroizing::new(std::mem::take(&mut o.password));
            if o.destination.trim().is_empty() {
                bail!("Specify a new export directory");
            }
            if !o.encrypt && !password.is_empty() {
                bail!("Enable encryption to set a new password");
            }
            let _export = app.reserve_export()?;
            let mut s = app.store.lock().unwrap();
            let p = s.export(
                Path::new(&o.destination),
                o.include_content,
                if o.encrypt { Some(&password) } else { None },
            )?;
            *app.stats_cache.lock().unwrap() = s.stats()?;
            json!({"path":p,"encrypted":o.encrypt||s.encryption.is_some()})
        }
        "/api/browse" => browse(&val("path"))?,
        "/api/open" => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Open {
                id: i64,
                #[serde(default)]
                folder: bool,
            }
            let o: Open = decode(bytes)?;
            let f = app.store.lock().unwrap().metadata(o.id)?;
            let target = validate_open(&f, o.folder)?;
            open_local(
                target
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Invalid path"))?,
            )?;
            json!({"ok":true})
        }
        "/api/shutdown" => {
            let a = app.clone();
            std::thread::spawn(move || {
                a.stop();
                a.shutdown.notify_one();
            });
            json!({"ok":true})
        }
        _ => return Ok((404, json!({"error":"Nepoznata ruta"}))),
    };
    Ok((200, v))
}
pub fn validate_open(f: &FileResult, folder: bool) -> Result<PathBuf> {
    if f.root.starts_with("demo://") || f.root.starts_with("import://") {
        bail!("Ovaj medij nije priključen. Pronađite ga prema oznaci i lokaciji.");
    }
    let root = Path::new(&f.root);
    let file = Path::new(&f.path);
    if !root.is_absolute() || !file.is_absolute() {
        bail!("Putanja nije dostupna na ovom računaru. Priključite originalni medij.");
    }
    let target = if folder {
        file.parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid file path"))?
    } else {
        file
    };
    if target
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
        || !target.starts_with(root)
    {
        bail!("Putanja nije unutar izabranog izvora");
    }
    let resolved_root = dunce::canonicalize(root)?;
    let resolved = dunce::canonicalize(target)?;
    if !resolved_root.is_dir() || !resolved.starts_with(&resolved_root) {
        bail!("Putanja vodi izvan izabranog izvora");
    }
    let meta = resolved.metadata()?;
    if (folder && !meta.is_dir()) || (!meta.is_dir() && !meta.is_file()) {
        bail!("Invalid file type");
    }
    Ok(resolved)
}
fn browse(raw: &str) -> Result<Value> {
    let mut entries = Vec::new();
    if cfg!(windows) && raw.is_empty() {
        for c in b'A'..=b'Z' {
            let p = format!("{}:\\", c as char);
            if Path::new(&p).is_dir() {
                entries.push(json!({"name":p,"path":p,"is_dir":true}));
            }
        }
        return Ok(json!({"path":"","parent":"","entries":entries,"truncated":false}));
    }
    let path = Path::new(if raw.is_empty() { "/" } else { raw });
    if !path.is_absolute() {
        bail!("Putanja mora biti apsolutna");
    }
    let mut truncated = false;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        if entries.len() == 2000 {
            truncated = true;
            break;
        }
        entries.push(
            json!({"name":entry.file_name().to_string_lossy(),"path":entry.path(),"is_dir":true}),
        );
    }
    entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(
        json!({"path":path,"parent":path.parent().unwrap_or(Path::new("")),"entries":entries,"truncated":truncated}),
    )
}
pub fn open_local(target: &str) -> Result<()> {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    let mut child = cmd.arg(target).spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
pub async fn serve(store: Store, port: u16, no_browser: bool) -> Result<()> {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    let base = format!("http://{}", listener.local_addr()?);
    println!(
        "AllFiles {VERSION} (Rust)\nLocal: {base}\nPodaci: {}\nZatvorite aplikaciju pre odmontiranja kontejnera.",
        store.dir.display()
    );
    if !no_browser {
        let _ = open_local(&base);
    }
    let app = App::new(Arc::new(Mutex::new(store)), base)?;
    let shutdown_app = app.clone();
    let router = Router::new().fallback(handler).with_state(app.clone());
    axum::serve(listener,router).with_graceful_shutdown(async move{tokio::select!{_=shutdown_app.shutdown.notified()=>{},_=crate::shutdown_signal()=>{let a=shutdown_app.clone();let _=tokio::task::spawn_blocking(move||a.stop()).await;}}}).await?;
    tokio::task::spawn_blocking(move || app.stop()).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::ScannedFile;
    use tokio::io::AsyncWriteExt;

    fn app() -> Result<(tempfile::TempDir, Arc<App>)> {
        let temp = tempfile::tempdir()?;
        let store = Arc::new(Mutex::new(Store::open(temp.path(), "")?));
        let app = App::new(store, "http://127.0.0.1:1234".into())?;
        Ok((temp, app))
    }

    #[test]
    fn pending_query_is_cancelled_without_waiting_for_store() -> Result<()> {
        let (_temp, app) = app()?;
        let store = app.store.lock().unwrap();
        let control = RequestControl::new();
        let worker_control = control.clone();
        let shared = app.store.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let (finished, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished
                .send(worker_control.lock_store(&shared).is_err())
                .unwrap();
        });
        ready.recv_timeout(Duration::from_secs(2))?;
        control.cancelled.store(true, Ordering::Relaxed);
        let cancelled = result.recv_timeout(Duration::from_secs(2));
        drop(store);
        worker.join().unwrap();
        assert!(cancelled?);
        Ok(())
    }

    #[test]
    fn cancelled_running_sql_releases_store_and_clears_progress_hook() -> Result<()> {
        let (_temp, app) = app()?;
        let control = RequestControl::new();
        let other_control = control.clone();
        let other = app.clone();
        let (started, ready) = std::sync::mpsc::channel();
        let (finished, received) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = query_store(&other, &other_control, |store| {
                started.send(())?;
                let total: i64 = store.conn.query_row(
                    "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<1000000000) SELECT sum(n) FROM numbers",
                    [], |r| r.get(0),
                )?;
                Ok(total)
            });
            finished.send(result.is_err()).unwrap();
        });
        ready.recv_timeout(Duration::from_secs(2))?;
        control.cancelled.store(true, Ordering::Relaxed);
        let cancelled = received.recv_timeout(Duration::from_secs(2));
        worker.join().unwrap();
        assert!(cancelled?);
        let store = app.store.lock().unwrap();
        let total: i64 = store.conn.query_row(
            "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<10000) SELECT sum(n) FROM numbers",
            [], |r| r.get(0),
        )?;
        assert_eq!(
            total, 50005000,
            "a cancelled request must not interrupt later users of the connection"
        );
        Ok(())
    }

    #[test]
    fn query_deadline_includes_queue_wait() -> Result<()> {
        let (_temp, app) = app()?;
        let _store = app.store.lock().unwrap();
        let expired = RequestControl {
            cancelled: Arc::new(AtomicBool::new(false)),
            deadline: Instant::now() - Duration::from_secs(1),
        };
        assert!(expired.lock_store(&app.store).is_err());
        Ok(())
    }

    #[test]
    fn status_revision_and_cached_counts_follow_completed_job() -> Result<()> {
        let (_temp, app) = app()?;
        let (release, wait) = std::sync::mpsc::channel();
        let (initial, revision) = app.start(move |shared, _, report| {
            wait.recv()?;
            let mut store = shared.lock().unwrap();
            let source = store.begin_scan(&ScanOptions {
                root: "/synthetic".into(),
                label: "Unit test".into(),
                ..Default::default()
            })?;
            store.upsert(
                &source.id,
                "unit",
                &[ScannedFile {
                    relative: "one.bin".into(),
                    name: "one.bin".into(),
                    dir: "".into(),
                    extension: "bin".into(),
                    size: 7,
                    modified: 1,
                    modified_ns: 1_000_000_000,
                }],
            )?;
            store.finish_scan(&source.id, "unit", true)?;
            drop(store);
            report(Progress {
                scanned: 1,
                ..Default::default()
            });
            Ok(())
        })?;
        let store = app.store.lock().unwrap();
        let status_app = app.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || sent.send(status_app.status()).unwrap());
        let status = received.recv_timeout(Duration::from_secs(2));
        drop(store);
        reader.join().unwrap();
        let status = status??;
        assert_eq!(status["job"]["id"], initial.id);
        assert_eq!(status["job_revision"], revision);
        assert_eq!(status["stats_cached"], true);
        let earlier_cache_read = app.refresh_status_cache()?;
        assert_eq!(app.stats_cache.lock().unwrap()["files"], 0);
        release.send(())?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while app.jobs.lock().unwrap().progress.as_ref().unwrap().status == "running" {
            assert!(Instant::now() < deadline, "job did not complete");
            std::thread::sleep(Duration::from_millis(2));
        }
        let store = app.store.lock().unwrap();
        // Inspect the snapshot directly while the store is unavailable: the
        // worker must refresh counters before publishing its completed state.
        let envelope = app.status_snapshot(earlier_cache_read)?;
        assert_eq!(envelope["job"]["status"], "completed");
        assert_eq!(
            envelope["files"], 1,
            "a completed revision must use the completed counters even if cache refresh began earlier"
        );
        let stats = app.stats_cache.lock().unwrap().clone();
        assert_eq!(stats["files"], 1);
        assert_eq!(stats["bytes"], 7);
        assert_eq!(stats["sources"], 1);
        let jobs = app.jobs.lock().unwrap();
        assert_eq!(jobs.progress.as_ref().unwrap().status, "completed");
        assert!(jobs.revision > revision);
        drop(jobs);
        drop(store);
        app.stop();
        Ok(())
    }

    #[test]
    fn export_reservation_does_not_block_status_or_cancel() -> Result<()> {
        let (_temp, app) = app()?;
        let export = app.reserve_export()?;
        assert!(app.reserve_export().is_err());
        assert!(app.start(|_, _, _| Ok(())).is_err());
        let store = app.store.lock().unwrap();
        let other = app.clone();
        let (sent, received) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let status = other.status();
            let cancel = api(
                &other,
                "POST",
                "/api/cancel",
                "",
                b"{}",
                &RequestControl::new(),
            );
            sent.send((status, cancel)).unwrap();
        });
        let result = received.recv_timeout(Duration::from_secs(2));
        drop(store);
        reader.join().unwrap();
        let (status, cancel) = result?;
        assert_eq!(status?["exporting"], true);
        assert_eq!(cancel?.0, 200);
        drop(export);
        assert!(!app.jobs.lock().unwrap().exporting);
        Ok(())
    }

    #[tokio::test]
    async fn query_admission_is_bounded_without_blocking_status() -> Result<()> {
        let (_temp, app) = app()?;
        let _permits = app
            .query_slots
            .clone()
            .acquire_many_owned(MAX_QUERY_REQUESTS as u32)
            .await?;
        let rejected = run_api(
            app.clone(),
            "GET".into(),
            "/api/search".into(),
            "".into(),
            Bytes::new(),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
        let status = run_api(
            app,
            "GET".into(),
            "/api/status".into(),
            "".into(),
            Bytes::new(),
        )
        .await;
        assert_eq!(status.status(), StatusCode::OK);
        Ok(())
    }

    #[tokio::test]
    async fn real_socket_disconnect_cancels_query_waiting_on_store() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let app = App::new(
            Arc::new(Mutex::new(Store::open(temp.path(), "")?)),
            format!("http://{address}"),
        )?;
        let router = Router::new().fallback(handler).with_state(app.clone());
        let (shutdown, closed) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = closed.await;
                })
                .await
        });
        let shared = app.store.clone();
        let (locked, ready) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let owner = std::thread::spawn(move || {
            let _store = shared.lock().unwrap();
            locked.send(()).unwrap();
            let _ = held.recv();
        });
        ready.recv_timeout(Duration::from_secs(2))?;
        let mut client = tokio::net::TcpStream::connect(address).await?;
        client
            .write_all(
                format!("GET /api/search?q=waiting HTTP/1.1\r\nHost: {address}\r\n\r\n").as_bytes(),
            )
            .await?;
        tokio::time::timeout(Duration::from_secs(2), async {
            while app.query_slots.available_permits() == MAX_QUERY_REQUESTS {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        drop(client);
        let cancelled = tokio::time::timeout(Duration::from_secs(2), async {
            while app.query_slots.available_permits() != MAX_QUERY_REQUESTS {
                tokio::task::yield_now().await;
            }
        })
        .await;
        release.send(())?;
        owner.join().unwrap();
        let _ = shutdown.send(());
        server.await??;
        cancelled?;
        Ok(())
    }
}
