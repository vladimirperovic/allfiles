use crate::{crypto, demo, import, mcp, scanner, server, shutdown_signal, store};
use anyhow::{Result, bail};
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;
fn password(prompt: &str) -> Result<Zeroizing<String>> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        bail!("Password entry requires an interactive terminal");
    }
    Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
}
pub fn run() -> Result<()> {
    let mut args = std::env::args().skip(1).peekable();
    let command = if args.peek().is_some_and(|s| !s.starts_with('-')) {
        args.next().unwrap()
    } else {
        "serve".into()
    };
    let mut options = HashMap::new();
    while let Some(arg) = args.next() {
        let (key, value) = arg
            .trim_start_matches('-')
            .split_once('=')
            .map(|(a, b)| (a.to_owned(), Some(b.to_owned())))
            .unwrap_or((arg.trim_start_matches('-').to_owned(), None));
        if !arg.starts_with('-') {
            bail!("Unexpected argument: {arg}");
        }
        let boolean = matches!(
            key.as_str(),
            "version" | "help" | "h" | "no-browser" | "content" | "catalog-only"
        );
        if !boolean
            && !matches!(
                key.as_str(),
                "data" | "port" | "root" | "label" | "kind" | "location" | "file" | "out" | "url"
            )
        {
            bail!("Unknown option: {key}");
        }
        let value = match value {
            Some(v) => v,
            None if boolean => "true".into(),
            None => args
                .next()
                .ok_or_else(|| anyhow::anyhow!("Missing value for {key}"))?,
        };
        if boolean && !matches!(value.as_str(), "true" | "false") {
            bail!("Invalid boolean option: {key}");
        }
        options.insert(key, value);
    }
    let get = |key: &str| options.get(key).cloned().unwrap_or_default();
    let flag = |key: &str| options.get(key).is_some_and(|s| s == "true");
    if flag("version") {
        println!(
            "AllFiles {} (Rust, SQLite {})",
            store::VERSION,
            rusqlite::version()
        );
        return Ok(());
    }
    if flag("help") || flag("h") {
        println!(
            "AllFiles — private portable catalog (Rust)\nCommands: serve, scan, import, export, encrypt, demo, mcp\nallfiles serve --data /existing/catalog --port 8787 --no-browser\nallfiles scan --data /existing/catalog --root /media/disk --label Archive --content\nallfiles import --data /existing/catalog --file export.csv\nallfiles export --data /existing/catalog --out /new/copy [--catalog-only]\nallfiles encrypt --data /existing/catalog --out /new/encrypted-copy\nallfiles mcp --url http://127.0.0.1:8787\nPasswords are entered only in an interactive terminal."
        );
        return Ok(());
    }
    if !matches!(
        command.as_str(),
        "serve" | "scan" | "import" | "export" | "encrypt" | "demo" | "mcp"
    ) {
        bail!("Unknown command: {command}");
    }
    if command == "mcp" {
        if !get("data").is_empty() {
            bail!("MCP uses --url, not --data");
        }
        let url = if get("url").is_empty() {
            "http://127.0.0.1:8787".into()
        } else {
            get("url")
        };
        return runtime()?.block_on(mcp::run(&url));
    }
    let data = get("data");
    if data.is_empty() {
        bail!("Specify --data with an existing catalog directory");
    }
    let unlock = if crypto::manifest(Path::new(&data))?.is_some() {
        password("Catalog password: ")?
    } else {
        Zeroizing::new(String::new())
    };
    let mut s = store::Store::open(Path::new(&data), &unlock)?;
    drop(unlock);
    match command.as_str() {
        "serve" => {
            let port = if get("port").is_empty() {
                0
            } else {
                get("port").parse::<u16>()?
            };
            runtime()?.block_on(server::serve(s, port, flag("no-browser")))
        }
        "demo" => demo::seed(&mut s),
        "export" | "encrypt" => {
            let out = get("out");
            if out.is_empty() {
                bail!("Specify --out");
            }
            let new = if command == "encrypt" {
                let first = password("New password (at least 12 characters): ")?;
                let second = password("Repeat password: ")?;
                if *first != *second {
                    bail!("Passwords do not match");
                }
                Some(first)
            } else {
                None
            };
            let path = s.export(
                Path::new(&out),
                !flag("catalog-only"),
                new.as_ref().map(|p| p.as_str()),
            )?;
            println!("Export: {}", path.display());
            Ok(())
        }
        _ => {
            let shared = Arc::new(Mutex::new(s));
            let cancel = Arc::new(AtomicBool::new(false));
            let signal = cancel.clone();
            let rt = runtime()?;
            rt.spawn(async move {
                shutdown_signal().await;
                signal.store(true, Ordering::Relaxed);
            });
            let report = |p: scanner::Progress| {
                if p.scanned % 256 == 0 {
                    eprintln!("{}: {} files, {} documents", p.phase, p.scanned, p.indexed);
                }
            };
            if command == "scan" {
                if get("root").is_empty() {
                    bail!("Specify --root");
                }
                scanner::scan(
                    &shared,
                    store::ScanOptions {
                        root: get("root"),
                        label: get("label"),
                        kind: get("kind"),
                        location: get("location"),
                        index_content: flag("content"),
                        ..Default::default()
                    },
                    &cancel,
                    &report,
                )?;
            } else {
                if get("file").is_empty() {
                    bail!("Specify --file");
                }
                import::import(
                    &shared,
                    import::ImportOptions {
                        path: get("file"),
                        label: get("label"),
                        kind: get("kind"),
                        location: get("location"),
                    },
                    &cancel,
                    &report,
                )?;
            }
            println!("{}", shared.lock().unwrap().stats()?);
            Ok(())
        }
    }
}
fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?)
}
