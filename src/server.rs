use crate::{
    crypto,
    import::{self, ImportOptions},
    scanner::{self, Progress, SharedStore},
    store::{FileResult, MAX_TEXT, ScanOptions, SearchOptions, Store, VERSION},
};
use anyhow::{Result, bail};
use axum::{
    Router,
    body::{Body, to_bytes},
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
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Default)]
struct Jobs {
    progress: Option<Progress>,
    cancel: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    stopping: bool,
}
pub struct App {
    pub store: SharedStore,
    base: String,
    token: String,
    jobs: Mutex<Jobs>,
    stop_lock: Mutex<()>,
    shutdown: tokio::sync::Notify,
}
impl App {
    fn new(store: SharedStore, base: String) -> Arc<Self> {
        Arc::new(Self {
            store,
            base,
            token: crypto::random_id(),
            jobs: Mutex::new(Jobs::default()),
            stop_lock: Mutex::new(()),
            shutdown: tokio::sync::Notify::new(),
        })
    }
    fn start(
        self: &Arc<Self>,
        work: impl FnOnce(&SharedStore, &AtomicBool, &dyn Fn(Progress)) -> Result<()> + Send + 'static,
    ) -> Result<Progress> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.stopping {
            bail!("Aplikacija se zatvara");
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
        jobs.cancel = Arc::new(AtomicBool::new(false));
        let cancel = jobs.cancel.clone();
        let app = self.clone();
        let id = initial.id.clone();
        jobs.thread = Some(std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                work(&app.store, &cancel, &|mut p| {
                    p.id = id.clone();
                    p.status = "running".into();
                    app.jobs.lock().unwrap().progress = Some(p);
                })
            }))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Indexing worker failed")));
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
        }));
        Ok(initial)
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
                Ok(Ok(bytes)) => {
                    let task = tokio::task::spawn_blocking(move || {
                        api(&app, &method, &path, &query, &bytes)
                    })
                    .await;
                    match task {
                        Ok(Ok((code, v))) => response(code, v),
                        Ok(Err(e)) => {
                            let code = if e
                                .downcast_ref::<rusqlite::Error>()
                                .is_some_and(|e| matches!(e, rusqlite::Error::QueryReturnedNoRows))
                                || e.downcast_ref::<std::io::Error>()
                                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound)
                            {
                                404
                            } else {
                                400
                            };
                            response(code, json!({"error":e.to_string()}))
                        }
                        Err(_) => response(500, json!({"error":"Internal worker failure"})),
                    }
                }
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
        "/api/session" => json!({"token":app.token}),
        "/api/status" => {
            let job = app.jobs.lock().unwrap().progress.clone();
            let mut v = app.store.lock().unwrap().stats()?;
            v["job"] = serde_json::to_value(job)?;
            v
        }
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
            let mut s = app.store.lock().unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            s.conn
                .progress_handler(1000, Some(move || Instant::now() >= deadline))?;
            let result = s.search(o);
            s.conn.progress_handler(0, None::<fn() -> bool>)?;
            serde_json::to_value(result?)?
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
            app.store
                .lock()
                .unwrap()
                .file(number(&q, "id", 0)?, offset as usize, max as usize)?
        }
        "/api/scan" => {
            let o: ScanOptions = decode(bytes)?;
            if !Path::new(&o.root).is_absolute() {
                bail!("Unesite apsolutnu putanju foldera");
            }
            let p = app.start(move |s, c, r| scanner::scan(s, o, c, r))?;
            return Ok((202, json!({"job":p})));
        }
        "/api/import" => {
            let o: ImportOptions = decode(bytes)?;
            let p = app.start(move |s, c, r| import::import(s, o, c, r))?;
            return Ok((202, json!({"job":p})));
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
            let jobs = app.jobs.lock().unwrap();
            if jobs.stopping
                || jobs
                    .progress
                    .as_ref()
                    .is_some_and(|p| p.status == "running")
            {
                bail!("Sačekajte da se indeksiranje završi pre izvoza");
            }
            let mut s = app.store.lock().unwrap();
            let p = s.export(
                Path::new(&o.destination),
                o.include_content,
                if o.encrypt { Some(&password) } else { None },
            )?;
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
    let app = App::new(Arc::new(Mutex::new(store)), base);
    let shutdown_app = app.clone();
    let router = Router::new().fallback(handler).with_state(app.clone());
    axum::serve(listener,router).with_graceful_shutdown(async move{tokio::select!{_=shutdown_app.shutdown.notified()=>{},_=crate::shutdown_signal()=>{let a=shutdown_app.clone();let _=tokio::task::spawn_blocking(move||a.stop()).await;}}}).await?;
    tokio::task::spawn_blocking(move || app.stop()).await?;
    Ok(())
}
