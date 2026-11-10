# Changelog

## Unreleased

- Fixed the folder view hiding files of media imported with absolute paths (`/home/...`, `//server/share/...`, or a leading `\` in a report): such folders were missing from the top level, so their files could only be found by searching. They are now listed there under their full first name, and the path bar handles them.
- Importing from the interface no longer forces every medium to `bluray`. A WinCatalog database keeps the kind of each medium (optical discs `bluray`, others `external`), as the command line already did; reports without that information still default to `bluray`.
- The browser demo answers the folder view, so choosing a medium there works again instead of showing "search is unavailable". Its browser check opens folders, which also repairs the Pages workflow.
- The MCP tool `allfiles_file` no longer returns a file's thumbnail as base64 text (up to about 700 KB, sent twice); it reports `has_thumbnail` instead. Other details are unchanged.
- A rescan keeps the settings it does not name. Running `scan` again without `--kind` or `--location` used to reset the medium to `computer` and erase its location; both now stay, as does the label when a medium is rescanned by ID. A new medium without a kind is a `folder`, as in the interface and MCP (the command line used `computer`). A kind other than `folder`, `computer`, `external`, `bluray` or `lto` is rejected instead of being stored.
- Importing a WinCatalog database again replaces a file's details instead of only adding to them: tags, rating, comment, EXIF, audio tags or a thumbnail removed in WinCatalog disappear here too. Imports from reports, which carry no such details, still leave stored ones alone.
- Importing the same WinCatalog database again no longer rewrites every thumbnail. Each stored image is compared and written only when it differs; with 20,000 thumbnails (209 MB) on an SSD a repeated import took 1.6 s instead of 3.8 s.
- Error messages from the service are translated again where their wording had drifted from the interface's table (export, import and "open file" errors, and the notes about missing document text); entries only the former Go service produced were removed. The Serbian interface now also translates the import, extraction and search messages the service sends in English (87 of its messages have both languages, up from 40); rare internal errors remain English.
- Import shows "Checking the catalog before import" while it validates the file, and the Cancel button stays disabled once pressed instead of becoming active again at the next status update. Job states only the former Go service reported were removed from the interface.
- A new file without extractable text is stored as `unsupported` in one write instead of two.
- Audio details that consist only of a channel count or sample rate are kept.
- Removed dependency features nothing uses: `backup` of the SQLite binding (export uses `VACUUM INTO`), `blocking` of reqwest, `derive` of zeroize and `query` of axum. `futures-io`, `futures-sink` and `zeroize_derive` left `Cargo.lock` and the third-party notices. Also removed an unused icon.
- **Folder view.** With one medium selected and no search text, type filter or tag filter, the list shows that medium like a file manager: subfolders first (with file count and total size), then the files stored directly in the folder. Double-click, Enter or the arrow opens a folder; the `..` row, Backspace and the path above the table go back. Choosing a search, filter or another sort returns to the flat result list. New `GET /api/folder?source=&path=&offset=&limit=`.
- **File details from a source.** An import can now carry a thumbnail, EXIF (camera, lens, size, date, ISO, exposure, aperture, focal length, flash, GPS), audio tags (title, artist, album, year, track, genre, duration, bitrate), tags, a 1–5 star rating, a comment and a CRC32. They appear in the file view; tags are clickable and filter the list (`tag=` on `/api/search`). Small details are stored in new tables of `catalog.sqlite`; thumbnails (JPEG/PNG only, at most 512 KiB) are stored in `content.sqlite` and bound to the file version, so a catalog exported without content stays small and only loses thumbnails. Older versions open such catalogs and ignore the new tables. The WinCatalog reader fills all of these.
- `import` reads a native WinCatalog database (`.w3cat`) directly, read-only and without writing any file beside it. Each scanned medium becomes an AllFiles medium (optical discs as `bluray`, other media as `external`), folder paths and file dates are reconstructed, day-first `DD/MM/YYYY` dates are understood, and archive contents are skipped. Accepted file size for `.w3cat` is up to 64 GiB.
- Search in "all" mode now returns filename and path matches when `content.sqlite` is damaged or belongs to another catalog, instead of failing the whole request. Content-only search still reports the problem.
- Presentations, workbooks and documents with ten or more numbered parts are indexed in document order (slide 2 before slide 10). Rescan affected media to reorder text that was already extracted.
- A request whose content database could not be attached keeps its 15-second deadline and disconnect cancellation for the rest of its work.
- The media list and "open file" requests no longer wait indefinitely for the catalog while an export or scan batch holds it; they share the search deadline.
- `scan`/`import` in the terminal print progress at most once per second (and on each phase change) instead of two lines per extracted document.
- Removed the redundant `files_source` index, already covered by `files_source_size`. On a synthetic 100,000-file catalog this makes the catalog 10% smaller (46.6 → 42.0 MB) and the first import about 14% faster (4.09 → 3.51 s); search times are unchanged. Older versions recreate the index when they open the catalog.
- Document text is extracted on up to four cores for folders, computers and external disks; Blu-ray and LTO media are still read one file at a time. On four cores, 240 multi-page PDFs indexed in 1.9 s instead of 4.5 s with identical text.
- Removed the Go reference implementation, its Go modules and the historical `experiments/` comparisons from the repository; they remain in earlier tags such as v0.1.6 and v0.2.0. The Rust executable keeps opening and updating catalogs written by the Go versions, including encrypted ones (checked with an encrypted catalog created by the v0.2.0 Go build). CI, Dependabot and the security workflow no longer build or audit Go code.
- Fixed filename and folder search for names stored in decomposed Unicode (NFD), which macOS and Mac-formatted media commonly use. Such names containing `č`, `ć`, `š` or `ž` were not found by either `račun` or `racun`; both now match. Existing catalogs refresh their stored search names once when opened, and remain readable by 0.1.6.

## 0.1.6 — Security maintenance — 2026-10-29

- Updated `quick-xml` to 0.41.0, fixing RUSTSEC-2026-0194 and RUSTSEC-2026-0195. AllFiles uses the plain reader; its attribute iteration was affected by the first advisory.
- XML imports and XLSX cells now reject more than 256 raw attributes per element and check cancellation during iteration. Different namespace prefixes can no longer bypass the import limit by collapsing into one normalized field name. Duplicate-attribute validation remains enabled.
- Added regression tests for oversized attribute lists, the accepted boundary, duplicate attributes and XML path compatibility. Catalog formats and encryption remain compatible.
- Added automated RustSec/Go vulnerability checks and committed dependency snapshots for GitHub, expanded Dependabot to the experimental manifests, and pinned the remaining comparison workflow actions.
- Retained ZIP 9.0.5 and explicit archive header-consistency validation. See the [security review](docs/SECURITY_REVIEW.md) for findings, scope and remaining maintenance warnings.

## 0.1.5 — Public release — 2026-10-29

This is the public release of the current Rust application. The version was deliberately set to **0.1.5** for this release. Historical 0.2.0 and 0.3.0 tags, releases and benchmark labels remain unchanged; the smaller number does not indicate older application code or a return to Go.

### Application

- Portable local file catalog with offline filename/path search, document-content search and media/location labels.
- Rust production executable with bundled native SQLite and FTS5, embedded Serbian/English browser interface, and a local HTTP service bound to `127.0.0.1`.
- Text extraction from PDF, modern Office, OpenDocument and supported plain-text files; bounded CSV/TSV, EFU and recognized XML import.
- Consistent catalog export, optional Adiantum encryption, and six MCP tools through a local `stdio` bridge.
- MIT license, contributor/security documentation, and a browser-only demonstration with fictional data: 1 LTO tape, 3 Blu-ray media and 3 disks.

### Correctness and responsiveness

- Compact Explorer-style file rows, searchable media lists with type filters, and first/last/page-number navigation make large archives easier to browse. Comfortable rows and media cards remain available.
- Navigation remains visible on long media lists, explicit page/query changes reset the results scroll position, and mobile detail panels manage keyboard focus and restore it when closed.
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

Local Windows checks and synthetic regression results are recorded in [docs/VALIDATION.md](docs/VALIDATION.md). Release publication requires successful native Linux/macOS/Windows jobs. Consult the [release page](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.5) for published packages; historical performance samples remain in [experiments/](https://github.com/vladimirperovic/allfiles/blob/v0.1.6/experiments/README.md).
