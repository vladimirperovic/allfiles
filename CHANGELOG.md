# Changelog

## 0.1.5 — Public release — 2026-10-29

This is the public release of the current Rust application. The version was deliberately set to **0.1.5** for this release. Historical 0.2.0 and 0.3.0 tags, releases and benchmark labels remain unchanged; the smaller number does not indicate older application code or a return to Go.

### Application

- Portable local file catalog with offline filename/path search, document-content search and media/location labels.
- Rust production executable with bundled native SQLite and FTS5, embedded Serbian/English browser interface, and a local HTTP service bound to `127.0.0.1`.
- Text extraction from PDF, modern Office, OpenDocument and supported plain-text files; bounded CSV/TSV, EFU and recognized XML import.
- Consistent catalog export, optional Adiantum encryption, and six MCP tools through a local `stdio` bridge.
- MIT license, contributor/security documentation, and a browser-only demonstration with fictional data: 1 LTO tape, 3 Blu-ray media and 3 disks.

### Correctness and responsiveness

- Updated ZIP parsing to 9.0.5 and enabled full header-consistency validation before reading Office XML. Conflicting filenames, duplicate entries and overlapping entries are rejected; valid streamed and ZIP64 documents remain covered by tests. AllFiles reads archive text in memory and does not extract archives to disk.
- Status responses carry an instance identifier and monotonic job revision. The interface rejects stale job updates, handles service restarts and preserves the closed state after shutdown.
- HTTP disconnects and MCP cancellation stop pending/running search work. Search admission is bounded, and the request deadline includes waiting for the database. MCP output retains complete JSON frames when requests are cancelled and handles EOF or broken output pipes.
- Status and cancellation remain available during database work and synchronous export. Busy status polls may use cached counters; mutation jobs refresh counters before publishing completion.
- Missing or incomplete content databases are reported consistently without erasing persisted extraction metadata. Text previews use bounded Unicode character pages.
- Search handles Unicode whitespace and uses SQLite's content tokenizer for matching snippets, including punctuation and diacritics. Duplicate query terms are removed; unnecessary ASCII query allocation is reduced.
- Import validation preserves recognized hierarchical media/path information, handles XML field ordering, and bounds shared buffers. Interleaved media records avoid redundant work.
- Regression coverage includes document extraction, malformed input, storage consistency, search ordering, frontend state recovery, export, shutdown and real socket cancellation.

### Compatibility and limits

The two-database catalog layout and encryption manifest v1 remain compatible. Go source remains a compatibility/reference implementation; production builds use Cargo. Back up the full data directory before changing executables, and rescan available originals when corrected text extraction is needed.

OCR, direct tape-drive control, automatic cross-computer synchronization, original-file deduplication and signed binaries are not included. PDF processing has input/output limits but does not run in a separate resource-limited process. Local-only operation does not protect an unlocked catalog from other processes on the same computer. See [security boundaries](SECURITY.md).

### Validation

Local Windows checks and synthetic regression results are recorded in [docs/VALIDATION.md](docs/VALIDATION.md). The configured Linux/macOS/Windows matrix is a validation plan until its individual jobs finish successfully. Consult the [release page](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.5) for packages actually published; historical performance samples remain in [experiments/](experiments/README.md).
