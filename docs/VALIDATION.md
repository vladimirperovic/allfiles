# Validation of 0.1.5

Recorded on 2026-10-29. Tests use generated files and catalogs; no personal catalog is required or published. Results below describe the final local Windows x64 Rust build. Native GitHub Actions results for Linux x64/ARM64, macOS Intel/Apple Silicon and Windows x64 are available in [Build and test](https://github.com/vladimirperovic/allfiles/actions/workflows/build.yml). Release publication requires all five native jobs to succeed.

## Local results

All 26 steps of the final validation run passed, including the release build, version consistency, formatting, Clippy with warnings denied, Go compatibility checks and the suites below.

| Check | Passed |
|---|---:|
| Rust unit/integration tests, including generated Go encryption fixture | 47 / 47 |
| Search, Unicode, sort/filter oracle and concurrent reads | 568 / 568 |
| Storage, export and consistency regression | 12 / 12 |
| Document extraction regression | 17 / 17 |
| Original frontend state regression | 6 / 6 |
| HTTP / MCP regression | 10 / 10 |
| Frontend async state, source reuse and shutdown regression | 25 / 25 |
| Live frontend/backend contract | 6 / 6 |
| Server cancellation, status, export and shutdown | 20 / 20 |
| Storage lifecycle and search/snippet checks | 67 / 67 |
| Browser end-to-end behavior | 13 / 13 |
| Browser startup/reconnection and missing-content recovery | 11 / 11 |
| Static dashboard demo | 17 / 17 |

Additional checks passed: HTTP/MCP smoke test, startup/statistics query inspection, interleaved import correctness, `go test ./...`, `go vet ./...`, `go test -race ./...`, tests and vet for the Go search experiment, and reading a Rust-encrypted export with the Go reference implementation. The demo had no JavaScript exceptions, failed HTTP responses, backend API calls or external requests. Desktop and narrow mobile screenshots were inspected.

Local toolchain: Windows 11 x64, Rust 1.99.1, bundled SQLite 3.54.1, Python 3.12.10, Node 26.5.1 and Go 1.26.10. Browser tests used headless Microsoft Edge. The local Rust build uses the GNU Windows toolchain; GitHub builds Windows with the native runner toolchain. Their executable checksums are expected to differ.

## Second review: corrected behavior

1. Status races: instance IDs and job revisions prevent older status/acceptance responses from replacing newer state. Job completion follows refreshed counters.
2. Fast jobs: completed jobs trigger a refresh even when polling never observed the running state or the file count did not change.
3. File details: refreshed catalogs invalidate stale details; removed files close the preview cleanly.
4. Service recovery: initial failures and service restarts retry the necessary state and acquire a fresh session token. Invalid searches do not retry indefinitely.
5. Cancellation: disconnected HTTP/MCP requests stop queued or executing search work. Admission is bounded and deadlines include lock wait time.
6. Unicode queries: whitespace outside quoted phrases is recognized and repeated terms are deduplicated.
7. Available content: document counts reflect readable linked content; incomplete copies produce a warning without erasing extraction metadata. Attach interruption rolls back cleanly.
8. Media filters: stale source responses are rejected and selection is reconciled after refresh.
9. Snippets: boundaries follow SQLite's Unicode tokenizer; the ASCII fast path retains matching semantics. Large unfinished tokens avoid repeated full rescanning.
10. Shutdown: late status responses cannot reopen the closed interface.

Grouped import transactions use bounded shared buffers. Media summaries use a covering index and correlated aggregates. Statistics avoid repeated full counts, unchanged media nodes are reused, filename-only previews avoid unnecessary content scans, and synchronous exports keep status/cancellation available.

## Performance observations

The baseline is the Rust executable immediately before the second set of ten corrections. The candidate is the final 0.1.5 Windows build. These are warm-cache synthetic measurements on one computer, not guarantees for other storage or catalogs.

| Workload | Before | 0.1.5 |
|---|---:|---:|
| Rare content search, pooled HTTP median, 100,050 files | 4.690 ms | 1.592 ms |
| Common content search, pooled HTTP median | 3.053 ms | 0.804 ms |
| Broad filename search, deep page | 7.872 ms | 8.145 ms |
| Import 100,000 rows, four alternating rounds | 3.502 s | 3.548 s |
| Reimport 100,000 rows | 1.279 s | 1.543 s |
| Content scan, 3,000 files | 0.912 s | 0.920 s |
| Unchanged rescan | 59.2 ms | 68.6 ms |

Content search improved about 3–4 times. Filename/path medians varied from 5.3% faster to 3.7% slower. Additional indexing and availability accounting have a write cost: the repeated import was 264 ms slower, and the unchanged rescan 9.4 ms slower. A first baseline scan was an outlier; no scan-speedup claim is based on it. Legacy field labels in `compare-http.py` are `go` and `rust`; both executables in this comparison were Rust.

Separate controlled workloads measured all 250 media summaries at 9.984 ms (previous 45.638 ms), the first 200 at 4.642 ms (previous 37.089 ms), and cached status at 0.352 ms. For 40 documents containing 21.12 MB of text, filename-only all-mode previews took 3.374 ms (previous 37.264 ms), while late content matches took 20.629 ms (previous 36.126 ms). An interleaved 20,000-row/100-media import took 1.524 s (previous 5.267 s) and produced the same metadata as grouped input. These separate workload medians are not the same experiment as the HTTP/write table above.

## Reproduce

Use Rust 1.99.1, Python 3.11 or newer with SQLite FTS5 support, and Node.js. From the repository root:

```sh
python scripts/check-version.py
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
python scripts/smoke.py target/release/allfiles
python scripts/audit_search.py target/release/allfiles
python scripts/audit_storage.py --binary target/release/allfiles --output audit/storage.json
python scripts/audit_extract.py target/release/allfiles
python scripts/audit-server-mcp.py target/release/allfiles
node scripts/audit_frontend.cjs
node scripts/audit2_frontend.cjs
python scripts/audit2_ui_contract.py target/release/allfiles audit/ui-contract.json
python scripts/audit2_server.py target/release/allfiles --output audit/server.json
python scripts/audit2_storage.py --binary target/release/allfiles --output audit/storage-lifecycle.json --bench
python scripts/audit2_import_order.py --binary target/release/allfiles --output audit/import-order.json
python scripts/build-demo.py --output dist/pages
```

On Windows append `.exe` to executable paths. Optional browser scripts document their Playwright/browser prerequisites in their source. The ignored Rust Go-fixture check requires `ALLFILES_GO_FIXTURE` pointing at a synthetic catalog generated by the Go reference; local validation included that fixture. Default CI does not claim that separately supplied cross-language fixture.

## Public release review

Before publication, all reachable Git history was scanned: 9 commits, 3 historical tags and 154 unique blobs. All 16 historical release assets were inspected in memory, and their published checksums matched. No recognized credentials, private keys, catalog databases or encryption manifests were found. Historical documentation contains a local home-directory example; current documentation uses generic examples. Pattern scanning cannot prove the absence of every possible secret.

Local audit logs, generated documents, fixtures, browser profiles and build output remain ignored. Release packaging uses an explicit allowlist and verifies archived bytes. Each archive includes checksums of its contents; the release adds `SHA256SUMS.txt` for all five archives. See [public release scope](PUBLIC_RELEASE.md) and [security boundaries](../SECURITY.md).
