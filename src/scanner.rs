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
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
// Most rows of one catalog transaction while walking (imports use twice as
// many). Every commit syncs a rollback journal that holds each page the batch
// touched, and rows land on scattered index pages, so larger batches write far
// less in total: a 120,000-file scan took ~15 s at 2048 rows and ~6 s at 8192.
pub const BATCH: usize = 8192;
/// Rows for the next transaction. A batch holds the store, and searches wait
/// for it, so the size follows the catalog's disk: it starts at a quarter of
/// the most, doubles while a batch takes under two seconds, and halves after
/// one that took more than five (a slow USB disk, a very large catalog).
pub struct Pace {
    rows: usize,
    most: usize,
}
impl Pace {
    pub fn new(most: usize) -> Self {
        Self {
            rows: most / 4,
            most,
        }
    }
    pub fn rows(&self) -> usize {
        self.rows
    }
    pub fn held(&mut self, time: Duration) {
        if time > Duration::from_secs(5) {
            self.rows = (self.rows / 2).max(self.most / 16);
        } else if time < Duration::from_secs(2) {
            self.rows = (self.rows * 2).min(self.most);
        }
    }
}
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
    // A blank label means "the folder name" for a new medium and "unchanged"
    // for an existing one; begin_scan resolves it together with the kind.
    if options.label.trim().is_empty() {
        options.label.clear();
    }
    let source = store.lock().unwrap().begin_scan(&options)?;
    let workers = extraction_workers(&source.kind);
    let mut state = Progress {
        id: crypto::random_id(),
        status: "running".into(),
        phase: "scanning".into(),
        ..Default::default()
    };
    report(state.clone());
    let result = (|| -> Result<()> {
        let mut batch = Vec::with_capacity(BATCH);
        let mut pace = Pace::new(BATCH);
        // Also written every few seconds, so a slow medium shows progress and a
        // cancelled scan keeps what it has walked.
        let mut flushed = Instant::now();
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
                extra: None,
            });
            state.current = relative;
            if batch.len() >= pace.rows() || flushed.elapsed() >= COMMIT_INTERVAL {
                flush(
                    store,
                    &source.id,
                    &root,
                    &mut batch,
                    &mut state,
                    &mut pace,
                    options.index_content,
                    workers,
                    cancel,
                    report,
                )?;
                flushed = Instant::now();
            }
        }
        flush(
            store,
            &source.id,
            &root,
            &mut batch,
            &mut state,
            &mut pace,
            options.index_content,
            workers,
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
// Text extraction is mostly CPU work (PDF and Office parsing), so disks and
// folders use several workers. Optical discs and tapes are read one file at a
// time: parallel reads would make the drive seek between files.
pub fn extraction_workers(kind: &str) -> usize {
    if matches!(kind, "bluray" | "lto") {
        return 1;
    }
    std::thread::available_parallelism().map_or(1, |n| n.get().clamp(1, 4))
}
enum Extraction {
    Started(String),
    Done(IndexedFile, Result<extract::Extracted>),
}
fn extract_current(
    root: &Path,
    item: &IndexedFile,
    cancel: &AtomicBool,
) -> Result<extract::Extracted> {
    let path: PathBuf = root.join(&item.relative);
    Ok(match fs::symlink_metadata(&path) {
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
    })
}
#[allow(clippy::too_many_arguments)] // A batch shares the scan's state and cancellation token.
fn flush(
    store: &SharedStore,
    source: &str,
    root: &Path,
    batch: &mut Vec<ScannedFile>,
    state: &mut Progress,
    pace: &mut Pace,
    index_content: bool,
    workers: usize,
    cancel: &AtomicBool,
    report: &dyn Fn(Progress),
) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }
    check(cancel)?;
    let (indexed, held) = {
        let mut store = store.lock().unwrap();
        let started = Instant::now();
        (store.upsert(source, &state.id, batch)?, started.elapsed())
    };
    pace.held(held);
    state.scanned += batch.len() as i64;
    batch.clear();
    if index_content {
        state.phase = "extracting".into();
        let todo = indexed
            .into_iter()
            .filter(|item| item.needs_text)
            .collect::<Vec<_>>();
        let workers = workers.clamp(1, todo.len().max(1));
        let queue = Mutex::new(todo.into_iter());
        let (sender, results) = mpsc::sync_channel::<Extraction>(workers * 2);
        // Extract without holding the store lock. Workers parse documents in
        // parallel; this thread reports progress and commits documents in
        // groups bounded by count, text size and time.
        std::thread::scope(|scope| -> Result<()> {
            // Owned here so an early return drops it and stops waiting workers.
            let results = results;
            for _ in 0..workers {
                let (sender, queue) = (sender.clone(), &queue);
                scope.spawn(move || {
                    while !cancel.load(Ordering::Relaxed) {
                        let Some(item) = queue.lock().unwrap().next() else {
                            break;
                        };
                        if sender
                            .send(Extraction::Started(item.relative.clone()))
                            .is_err()
                        {
                            break;
                        }
                        let extracted = extract_current(root, &item, cancel);
                        if sender.send(Extraction::Done(item, extracted)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(sender);
            let mut pending = Vec::new();
            let (mut bytes, mut since) = (0, Instant::now());
            for message in results.iter() {
                check(cancel)?;
                let (item, extracted) = match message {
                    Extraction::Started(relative) => {
                        state.current = relative;
                        report(state.clone());
                        continue;
                    }
                    Extraction::Done(item, extracted) => (item, extracted?),
                };
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
            check(cancel)?;
            record(store, &mut pending)
        })?;
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
    fn batch_size_follows_how_long_the_store_was_held() {
        let mut pace = Pace::new(8192);
        assert_eq!(pace.rows(), 2048);
        pace.held(Duration::from_millis(300));
        pace.held(Duration::from_millis(1900));
        pace.held(Duration::from_millis(10));
        assert_eq!(pace.rows(), 8192, "quick batches grow to the most only");
        pace.held(Duration::from_secs(3));
        assert_eq!(pace.rows(), 8192, "between the two limits the size stays");
        for _ in 0..10 {
            pace.held(Duration::from_secs(6));
        }
        assert_eq!(pace.rows(), 512, "slow batches shrink to a floor");
        pace.held(Duration::from_secs(1));
        assert_eq!(pace.rows(), 1024);
    }

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
