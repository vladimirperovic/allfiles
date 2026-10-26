use crate::{
    crypto, extract,
    store::{ScanOptions, ScannedFile, Store},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::UNIX_EPOCH,
};
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub id: String,
    pub status: String,
    pub phase: String,
    pub current: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    pub scanned: i64,
    pub indexed: i64,
}
pub type SharedStore = Arc<Mutex<Store>>;
pub fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        bail!("Cancelled");
    }
    Ok(())
}
pub fn modified(meta: &fs::Metadata) -> Result<(i64, i64)> {
    let time = meta.modified()?;
    let ns = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_nanos())?,
        Err(e) => -i64::try_from(e.duration().as_nanos())?,
    };
    Ok((ns.div_euclid(1_000_000_000), ns))
}
pub fn scan(
    store: &SharedStore,
    mut options: ScanOptions,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    check(cancel)?;
    let raw = Path::new(&options.root);
    let meta = fs::symlink_metadata(raw)?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        bail!("scan root must be a real directory, not a symbolic link");
    }
    let root = dunce::canonicalize(raw)?;
    let data = store.lock().unwrap().dir.clone();
    if root.starts_with(&data) {
        bail!("the catalog data directory cannot be scanned");
    }
    options.root = root
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("Scan root is not UTF-8"))?
        .into();
    if options.label.trim().is_empty() {
        options.label = root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
    }
    if options.kind.is_empty() {
        options.kind = "computer".into();
    }
    let source = store.lock().unwrap().begin_scan(&options)?;
    let mut state = Progress {
        id: crypto::random_id(),
        status: "running".into(),
        phase: "scanning".into(),
        ..Default::default()
    };
    report(state.clone());
    let result = (|| -> Result<()> {
        let mut batch = Vec::with_capacity(256);
        let mut errors = Vec::new();
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| !e.path().starts_with(&data))
        {
            check(cancel)?;
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    if errors.len() < 5 {
                        errors.push(e.to_string());
                    }
                    continue;
                }
            };
            if !entry.file_type().is_file() {
                continue;
            }
            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(e) => {
                    if errors.len() < 5 {
                        errors.push(e.to_string());
                    }
                    continue;
                }
            };
            let rel = entry.path().strip_prefix(&root)?;
            let Some(relative) = rel.to_str() else {
                if errors.len() < 5 {
                    errors.push("Filename is not valid UTF-8".into());
                }
                continue;
            };
            let relative = relative.replace('\\', "/");
            let name = entry
                .file_name()
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("Filename is not UTF-8"))?
                .to_owned();
            let dir = relative
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or("")
                .to_owned();
            let extension = Path::new(&name)
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            let (seconds, nanos) = modified(&meta)?;
            batch.push(ScannedFile {
                relative: relative.clone(),
                name,
                dir,
                extension,
                size: i64::try_from(meta.len())?,
                modified: seconds,
                modified_ns: nanos,
            });
            state.current = relative;
            if batch.len() == 256 {
                flush(
                    store,
                    &source.id,
                    &root,
                    &mut batch,
                    &mut state,
                    options.index_content,
                    cancel,
                    report,
                )?;
            }
        }
        flush(
            store,
            &source.id,
            &root,
            &mut batch,
            &mut state,
            options.index_content,
            cancel,
            report,
        )?;
        check(cancel)?;
        if !errors.is_empty() {
            bail!(
                "scan incomplete; previous entries retained: {}",
                errors.join("; ")
            );
        }
        Ok(())
    })();
    let finish = store
        .lock()
        .unwrap()
        .finish_scan(&source.id, &state.id, result.is_ok());
    result.and(finish)
}
#[allow(clippy::too_many_arguments)] // A batch shares the scan's state and cancellation token.
fn flush(
    store: &SharedStore,
    source: &str,
    root: &Path,
    batch: &mut Vec<ScannedFile>,
    state: &mut Progress,
    index_content: bool,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    check(cancel)?;
    let indexed = store.lock().unwrap().upsert(source, &state.id, batch)?;
    state.scanned += batch.len() as i64;
    batch.clear();
    if index_content {
        state.phase = "extracting".into();
        for item in indexed {
            check(cancel)?;
            if !item.needs_text {
                continue;
            }
            state.current = item.relative.clone();
            report(state.clone());
            let path: PathBuf = root.join(&item.relative);
            let extracted = match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    let (_, ns) = modified(&meta)?;
                    if format!("{}:{ns}", meta.len()) != item.version {
                        extract::Extracted::error(
                            "File changed after metadata scanning; scan again to index its current contents.",
                        )
                    } else {
                        extract::extract(&path, cancel)
                    }
                }
                Err(e) => extract::Extracted::error(e.to_string()),
            };
            check(cancel)?;
            store.lock().unwrap().record_content(
                item.id,
                &item.version,
                &extracted.status,
                &extracted.text,
                &extracted.detail,
            )?;
            if matches!(extracted.status.as_str(), "indexed" | "truncated") {
                state.indexed += 1;
            }
            report(state.clone());
        }
    }
    state.phase = "scanning".into();
    report(state.clone());
    Ok(())
}
