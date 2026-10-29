use crate::{
    crypto, extract,
    store::{Content, IndexedFile, ScanOptions, ScannedFile, Store, file_version},
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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
// Rows per catalog transaction while walking or importing. Larger batches mean
// fewer synchronous commits; each still holds the store lock only briefly.
pub const BATCH: usize = 2048;
const COMMIT_INTERVAL: Duration = Duration::from_secs(2);
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
    timestamp(meta.modified()?)
}
fn timestamp(time: SystemTime) -> Result<(i64, i64)> {
    let ns = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => i128::try_from(d.as_nanos())?,
        Err(e) => -i128::try_from(e.duration().as_nanos())?,
    };
    // SQLite stores two i64 values. Preserve the actual seconds for display,
    // and the nanoseconds modulo 2^64 for the version token. file_version
    // includes seconds for out-of-range dates so wrapped values cannot alias.
    Ok((i64::try_from(ns.div_euclid(1_000_000_000))?, ns as i64))
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
        let mut batch = Vec::with_capacity(BATCH);
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
            if batch.len() == BATCH {
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
        // Extract without holding the store lock and commit documents in groups,
        // bounded by count, text size and time so progress persists regularly.
        let mut pending = Vec::new();
        let (mut bytes, mut since) = (0, Instant::now());
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
                    let (seconds, ns) = modified(&meta)?;
                    if file_version(i64::try_from(meta.len())?, seconds, ns) != item.version {
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
            if matches!(extracted.status.as_str(), "indexed" | "truncated") {
                state.indexed += 1;
            }
            bytes += extracted.text.len();
            pending.push((item, extracted));
            if pending.len() >= 256 || bytes >= 16 << 20 || since.elapsed() >= COMMIT_INTERVAL {
                record(store, &mut pending)?;
                (bytes, since) = (0, Instant::now());
            }
            report(state.clone());
        }
        record(store, &mut pending)?;
    }
    state.phase = "scanning".into();
    report(state.clone());
    Ok(())
}
fn record(store: &SharedStore, pending: &mut Vec<(IndexedFile, extract::Extracted)>) -> Result<()> {
    if pending.is_empty() {
        return Ok(());
    }
    let items = pending
        .iter()
        .map(|(file, text)| Content {
            id: file.id,
            version: &file.version,
            status: &text.status,
            text: &text.text,
            detail: &text.detail,
        })
        .collect::<Vec<_>>();
    store.lock().unwrap().record_contents(&items)?;
    pending.clear();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_cover_filesystem_dates_and_preserve_version_precision() -> Result<()> {
        for seconds in [
            -11_612_937_600i64,
            -9_223_372_037,
            -1,
            0,
            1,
            9_223_372_037,
            32_503_680_000,
        ] {
            let base = if seconds < 0 {
                UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs()))
            } else {
                UNIX_EPOCH.checked_add(Duration::from_secs(seconds as u64))
            }
            .expect("filesystem date must fit SystemTime");
            // Windows SystemTime has 100 ns resolution.
            let time = base + Duration::from_nanos(123_400);
            let (actual_seconds, ns) = timestamp(time)?;
            assert_eq!(actual_seconds, seconds);
            let (_, next_ns) = timestamp(time + Duration::from_nanos(100))?;
            assert_ne!(
                file_version(20, seconds, ns),
                file_version(20, seconds, next_ns)
            );
            if i64::try_from(seconds as i128 * 1_000_000_000 + 123_400).is_ok() {
                assert_eq!(file_version(20, seconds, ns), format!("20:{ns}"));
            } else {
                assert_eq!(file_version(20, seconds, ns), format!("20:{seconds}:{ns}"));
                assert_ne!(
                    file_version(20, seconds, ns),
                    file_version(20, ns.div_euclid(1_000_000_000), ns)
                );
            }
        }
        assert_eq!(
            timestamp(UNIX_EPOCH - Duration::from_nanos(100))?,
            (-1, -100)
        );
        Ok(())
    }
}
