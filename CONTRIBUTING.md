# Contributing

AllFiles is a local desktop application. Changes should preserve offline catalog access, bounded processing, the two-database format and the privacy boundaries described in [SECURITY.md](SECURITY.md). The production implementation is in Rust under `src/`; root-level Go files are retained for reference and compatibility checks.

## Report a reproducible issue

Use the bug-report or feature-request form in [GitHub Issues](https://github.com/vladimirperovic/allfiles/issues/new/choose). English and Serbian are welcome. For a bug, include:

- AllFiles version (`allfiles --version`), operating system and architecture.
- Exact steps, expected behavior and actual behavior.
- A minimal example generated from fictional names and text, plus sanitized error output.

Do not attach personal catalogs, private documents, passwords, tokens, `encryption.json`, browser profiles or logs/screenshots containing personal paths. Reproduce a problem in a new temporary directory. Use [private security reporting](SECURITY.md) for vulnerabilities.

## Build and run

Use Rust **1.99.1** and the committed `Cargo.lock`. Bundled SQLite requires a C compiler: `base-devel` on Arch/Omarchy, Xcode Command Line Tools on macOS, or Visual Studio C++ Build Tools / llvm-mingw on Windows. Use Python 3.11 or newer with SQLite FTS5 support for validation scripts, and Node.js for frontend regressions. Finished application packages do not require these development tools.

From the repository root:

```sh
python scripts/check-version.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python scripts/smoke.py target/release/allfiles
```

Use `target/release/allfiles.exe` on Windows, and account for a custom `CARGO_TARGET_DIR` if configured. Rebuild after changing `web/`: the interface is embedded at compile time. For a local demonstration, create a new empty directory and run the `demo` and `serve` commands from the [user guide](README.en.md#demo-without-your-data).

`sh scripts/build.sh` or `.\scripts\build.ps1` builds and copies the native executable and release documentation into `dist/`. These scripts do not run the full test suite or create a release on GitHub. Python 3 archive/checksum commands are documented in [package a tested build](docs/PUBLIC_RELEASE.md#package-a-tested-build). Cross-compiling requires the target's Rust standard library and a matching C linker. A successful cross-build is not a runtime test on the target device.

## Focused regression checks

Run checks appropriate to the change after building. Create `.tmp/` first for the reports (`mkdir -p .tmp` in a POSIX shell, or `New-Item -ItemType Directory -Path .tmp -Force` in PowerShell). These examples create synthetic temporary catalogs:

```sh
python scripts/audit_search.py target/release/allfiles
python scripts/audit_extract.py target/release/allfiles
python scripts/audit-server-mcp.py target/release/allfiles
python scripts/audit_storage.py --binary target/release/allfiles --output .tmp/storage.json
python scripts/audit2_storage.py --binary target/release/allfiles --output .tmp/storage-lifecycle.json
python scripts/audit2_ui_contract.py target/release/allfiles .tmp/ui-contract.json
node scripts/audit_frontend.cjs
node scripts/audit2_frontend.cjs
```

The broader concurrency and import-order probes are intentionally separate from the shortest feedback loop:

```sh
python scripts/audit2_server.py target/release/allfiles --output .tmp/server.json
python scripts/audit2_import_order.py --binary target/release/allfiles --output .tmp/import-order.json
```

Do not run competing heavy benchmarks simultaneously. Preserve result counts and ordered IDs when comparing search implementations; record the binary hash, workload, environment and raw samples. Timing alone is not evidence that a query returned the right results. Real browser interaction supplements the scripted DOM/state checks.

The Rust test `reads_and_updates_go_encryption_format` is ignored by default. It requires the specific synthetic encrypted Go fixture described in `src/tests.rs`, supplied through `ALLFILES_GO_FIXTURE`, and can then run with `cargo test --locked reads_and_updates_go_encryption_format -- --include-ignored`. This variable identifies a test fixture, not a password. Never substitute a personal catalog. Go reference checks use the version pinned by `go.mod`; run `go test ./...` when changing that implementation or compatibility fixtures.

## Submit a change

Keep changes focused and explain the concrete behavior before and after. Add a regression test for a fixed correctness or lifecycle bug, using synthetic data. Include the checks actually run and any platform or fixture that was unavailable. Keep public documentation in English and update README.md together with its README.en.md compatibility copy for user-visible changes. Preserve license notices. Avoid generated executables, personal exports, credentials and machine-specific paths in commits.

Use [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) to locate components and [docs/PUBLIC_RELEASE.md](docs/PUBLIC_RELEASE.md) for release preparation. Existing historical version tags and performance evidence must not be rewritten to match a new release number.

## Dependency updates

Dependabot is configured to check the production Cargo manifest and GitHub Actions monthly, with at most three Cargo and two Actions version-update pull requests open at once. This configuration does not enable automatic merging or prove that an update is compatible. Review changes to native SQLite, text parsing and encryption especially carefully; run the relevant compatibility/regression checks and refresh third-party notices when dependencies change. The Go reference and isolated experiments need separate deliberate updates.
