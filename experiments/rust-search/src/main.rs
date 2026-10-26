use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const SQLITE_VERSION: &str = "3.53.4";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    count_sql: String,
    page_sql: String,
    args: Vec<String>,
    expected_count: i64,
    expected_ids: Vec<i64>,
}

struct Options {
    data: PathBuf,
    cases: PathBuf,
    iterations: usize,
    warmup: usize,
}

#[derive(Serialize)]
struct CaseResult {
    name: String,
    median_ms: f64,
    p95_ms: f64,
    min_ms: f64,
    count: i64,
    ids: Vec<i64>,
}

#[derive(Serialize)]
struct Report {
    implementation: &'static str,
    sqlite_version: String,
    sqlite_source_id: String,
    sqlite_compile_options: Vec<String>,
    iterations: usize,
    warmup: usize,
    results: Vec<CaseResult>,
}

fn options() -> Result<Options> {
    let mut data = None;
    let mut cases = None;
    let mut iterations = 20;
    let mut warmup = 3;
    let mut seen = HashSet::new();
    let mut arguments = env::args_os().skip(1);
    while let Some(argument) = arguments.next() {
        let flag = argument.to_str().ok_or("invalid command-line option")?;
        if flag == "--help" || flag == "-h" {
            println!("allfiles-rust-search --data DIR --cases FILE [--iterations 20] [--warmup 3]");
            std::process::exit(0);
        }
        if !seen.insert(argument.clone()) {
            return Err(format!("duplicate option: {flag}").into());
        }
        let value = arguments.next().ok_or_else(|| format!("missing value for {flag}"))?;
        match flag {
            "--data" => data = Some(PathBuf::from(value)),
            "--cases" => cases = Some(PathBuf::from(value)),
            "--iterations" => iterations = value.to_str().ok_or("invalid iterations")?.parse()?,
            "--warmup" => warmup = value.to_str().ok_or("invalid warmup")?.parse()?,
            _ => return Err(format!("unknown option: {flag}").into()),
        }
    }
    if !(1..=100_000).contains(&iterations) || warmup > 100_000 {
        return Err("iterations must be 1..100000 and warmup 0..100000".into());
    }
    Ok(Options {
        data: data.ok_or("--data is required")?,
        cases: cases.ok_or("--cases is required")?,
        iterations,
        warmup,
    })
}

// Percent-encode paths before appending URI parameters. A filename containing '?'
// or '#' must not turn into SQLite connection options. Input databases must exist.
fn readonly_uri(path: &Path) -> Result<String> {
    let canonical = fs::canonicalize(path)?;
    let raw = canonical.to_str().ok_or("database path is not valid UTF-8")?;
    #[cfg(windows)]
    let raw = raw.strip_prefix(r"\\?\").unwrap_or(raw).replace('\\', "/");
    let mut uri = String::from("file:");
    for byte in raw.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(byte) {
            uri.push(*byte as char);
        } else {
            use std::fmt::Write;
            write!(uri, "%{byte:02X}")?;
        }
    }
    uri.push_str("?mode=ro");
    Ok(uri)
}

fn open_catalog(data: &Path) -> Result<Connection> {
    if data.join("encryption.json").exists() {
        return Err("this benchmark only accepts unencrypted synthetic catalogs".into());
    }
    let main = readonly_uri(&data.join("catalog.sqlite"))?;
    let content = readonly_uri(&data.join("content.sqlite"))?;
    let connection = Connection::open_with_flags(
        main,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute("ATTACH DATABASE ?1 AS content", [&content])?;
    connection.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA trusted_schema=ON; PRAGMA query_only=ON;")?;
    let version: String = connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?;
    if version != SQLITE_VERSION {
        return Err(format!("expected SQLite {SQLITE_VERSION}, linked {version}; use build-native-sqlite.sh").into());
    }
    let fts5: i64 = connection.query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |row| row.get(0))?;
    if fts5 != 1 {
        return Err("native SQLite was built without FTS5".into());
    }
    Ok(connection)
}

// Both statements are freshly prepared, read and finalized inside every sample.
// No statement cache, JSON encoding, result validation or startup is in this timer.
fn query_pair(connection: &Connection, case: &Case) -> Result<(i64, Vec<i64>)> {
    let mut count_statement = connection.prepare(&case.count_sql)?;
    let count = count_statement.query_row(params_from_iter(case.args.iter()), |row| row.get(0))?;
    count_statement.finalize()?;

    let mut page_statement = connection.prepare(&case.page_sql)?;
    let mut ids = Vec::with_capacity(case.expected_ids.len());
    {
        let mut rows = page_statement.query(params_from_iter(case.args.iter()))?;
        while let Some(row) = rows.next()? {
            ids.push(row.get(0)?);
        }
    }
    page_statement.finalize()?;
    Ok((count, ids))
}

fn summarize(mut samples: Vec<f64>) -> (f64, f64, f64) {
    samples.sort_by(f64::total_cmp);
    let count = samples.len();
    let median = if count % 2 == 0 {
        (samples[count / 2 - 1] + samples[count / 2]) / 2.0
    } else {
        samples[count / 2]
    };
    // Nearest-rank p95, matching the Go comparator exactly.
    let p95 = samples[(95 * count).div_ceil(100) - 1];
    (median, p95, samples[0])
}

fn run_case(connection: &Connection, case: &Case, iterations: usize, warmup: usize) -> Result<CaseResult> {
    let mut samples = Vec::with_capacity(iterations);
    let mut last_count = 0;
    let mut last_ids = Vec::new();
    for sample in 0..warmup + iterations {
        let started = Instant::now();
        let (count, ids) = query_pair(connection, case)?;
        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
        if count != case.expected_count || ids != case.expected_ids {
            return Err(format!("case {:?} sample {sample}: results differ from the shared manifest", case.name).into());
        }
        if sample >= warmup {
            samples.push(elapsed);
        }
        last_count = count;
        last_ids = ids;
    }
    let (median_ms, p95_ms, min_ms) = summarize(samples);
    Ok(CaseResult { name: case.name.clone(), median_ms, p95_ms, min_ms, count: last_count, ids: last_ids })
}

fn run() -> Result<()> {
    let options = options()?;
    if fs::metadata(&options.cases)?.len() > 32 * 1024 * 1024 {
        return Err("case manifest exceeds 32 MiB".into());
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(&options.cases)?)?;
    if manifest.format != 1 || manifest.cases.is_empty() || manifest.cases.len() > 1_000 {
        return Err("expected manifest format 1 with 1..1000 cases".into());
    }
    let mut names = HashSet::new();
    for case in &manifest.cases {
        if case.name.is_empty() || !names.insert(&case.name) || case.expected_count < 0 {
            return Err("case names must be nonempty and unique; expected counts must be nonnegative".into());
        }
    }
    let connection = open_catalog(&options.data)?;
    let mut compile_options = connection.prepare("PRAGMA compile_options")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    compile_options.sort();
    let results = manifest.cases.iter()
        .map(|case| run_case(&connection, case, options.iterations, options.warmup))
        .collect::<Result<Vec<_>>>()?;
    let report = Report {
        implementation: "rust-native",
        sqlite_version: connection.query_row("SELECT sqlite_version()", [], |row| row.get(0))?,
        sqlite_source_id: connection.query_row("SELECT sqlite_source_id()", [], |row| row.get(0))?,
        sqlite_compile_options: compile_options,
        iterations: options.iterations,
        warmup: options.warmup,
        results,
    };
    println!("{}", serde_json::to_string(&report)?);
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => { eprintln!("allfiles-rust-search: {error}"); ExitCode::FAILURE }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_uses_even_median_and_nearest_rank_p95() {
        assert_eq!(summarize(vec![4.0, 1.0, 3.0, 2.0]), (2.5, 4.0, 1.0));
        assert_eq!(summarize((1..=20).map(f64::from).collect()), (10.5, 19.0, 1.0));
        assert_eq!(summarize(vec![7.0]), (7.0, 7.0, 7.0));
    }

    #[test]
    fn query_pair_preserves_order_and_string_parameters() -> Result<()> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch("CREATE TABLE files(id INTEGER, name TEXT); INSERT INTO files VALUES (1,'a'),(2,'b'),(3,'a');")?;
        let case = Case {
            name: "fixture".into(), count_sql: "SELECT count(*) FROM files WHERE name=?1".into(),
            page_sql: "SELECT id FROM files WHERE name=?1 ORDER BY id DESC".into(), args: vec!["a".into()],
            expected_count: 2, expected_ids: vec![3, 1],
        };
        let result = run_case(&connection, &case, 2, 1)?;
        assert_eq!((result.count, result.ids), (2, vec![3, 1]));
        let wrong = Case { expected_count: 99, ..case };
        assert!(run_case(&connection, &wrong, 1, 0).is_err());
        Ok(())
    }
}
