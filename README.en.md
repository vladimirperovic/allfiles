# AllFiles

[Srpski](README.md) · English

A portable catalog for files on your computers, external drives, Blu-ray discs and LTO tapes. One executable opens a local web interface, and your catalog remains searchable when the original media are disconnected. AllFiles stores file metadata and extracted document text; **it does not back up the original files**.

The application is designed for local use, including Omarchy Linux with its data inside a mounted VeraCrypt container. Windows and macOS builds are also available. There is no subscription, required cloud server or bundled AI model. The executable needs no separate Go, Node, Python or SQLite installation.

## Start the application

Download the archive for your computer from [GitHub Releases](https://github.com/vladimirperovic/allfiles/releases), verify its SHA-256 checksum and extract it into your chosen folder. For Omarchy on an Intel/AMD computer, use `linux-amd64`. Mount your VeraCrypt container first if you use one.

From a terminal in the application's folder:

```sh
chmod +x ./allfiles-linux-amd64
mkdir -p data
./allfiles-linux-amd64 serve --data "$PWD/data"
```

AllFiles prints its local address and opens your browser. It chooses a free port by default. Add `--port 8765` for a fixed port, or `--no-browser` to open the address yourself. `serve` can be omitted. The `--data` directory **must already exist**; the application does not silently put your catalog in your home folder.

The **EN / SR** button switches the web interface between English and Serbian and remembers that choice in the current browser. It does not translate or change your filenames, paths, media labels or document contents. Terminal help, password prompts and some underlying diagnostic messages are still in Serbian.

On Windows, use PowerShell:

```powershell
New-Item -ItemType Directory -Path data -Force | Out-Null
.\allfiles-windows-amd64.exe serve --data "$PWD\data"
```

On an Apple Silicon MacBook Air, use `allfiles-darwin-arm64`; Intel Macs use `allfiles-darwin-amd64`. The Linux commands above also work on macOS after substituting the executable name. Initial builds are unsigned, so Windows or macOS may ask you to allow the downloaded application. Do not disable your system's protection globally.

Stop with `Ctrl+C` or the interface's close-application action, and wait for it to exit before disconnecting storage or dismounting a container. Only one AllFiles process can open a data directory at a time. While the interface is running, use its scan/import/export actions; close it before running the equivalent standalone CLI commands. A remaining `.allfiles.lock` file is normal: its operating-system lock is released when the process exits.

## Storage and portability

```text
allfiles-linux-amd64        executable with the web interface built in
data/
  catalog.sqlite           media, folders, filenames and metadata
  content.sqlite           extracted document text and its search index
  encryption.json          present and required for encrypted catalogs
```

The content database is created when needed. Keeping it separate lets you export a smaller metadata-only catalog; separation alone does not guarantee faster searches. Size depends on the number and length of paths, extracted text and search indexes.

This is a local executable with a browser interface, not a standalone `index.html` application. It listens only on `127.0.0.1`. Copy the appropriate executable and an exported data directory to USB to use them on another computer. The database format is shared across supported operating systems; paths to original files may still differ between computers.

## Add files and search

Choose **Add media** in the interface, set a label and physical location, and select a folder available on the current computer. Media kinds are `folder`, `computer`, `external`, `bluray` and `lto`. For example, after closing the running service:

```sh
./allfiles-linux-amd64 scan \
  --data "$PWD/data" \
  --root "/path/to/your/folder" \
  --label "HOME-SSD-01" \
  --kind external \
  --location "Home office shelf" \
  --content
```

Replace the sample path with a real one. Without `--content`, the CLI scan collects metadata only. With it, AllFiles also extracts text from supported documents. A completed rescan removes stale catalog entries for missing files; it never deletes original files. A cancelled or failed scan retains records already processed and does not perform that final stale-entry cleanup.

The folder picker lists up to 2,000 subfolders and shows a notice if more exist. Ordinary files do not consume this limit. Enter the full folder path directly when the desired folder is not listed.

Search filenames/paths or document content, then narrow results by media and file type. Results identify the original location even when its media are offline. Opening the original file requires the media to be available at the recorded path. Scans skip symbolic links and the application's own data directory.

| Document type | Content indexing |
|---|---|
| PDF with a text layer | Extractable text |
| DOCX, XLSX, PPTX | Text from modern Office XML files |
| ODT, ODS, ODP | OpenDocument text |
| TXT and supported plain-text formats | Text content |
| Image-only scanned PDF | Metadata only; OCR is not included |
| Legacy DOC, XLS, PPT | Metadata only in this version |

Extraction does not reproduce document layout, images or every embedded object. Protected, damaged and unsupported documents remain in the catalog even when text extraction fails. Processing is limited to 256 MiB per input document and 8 MiB of extracted text, with additional limits on expanded Office XML. File details show when text is truncated.

An LTO tape must be available as a mounted filesystem or through a supported catalog export. AllFiles does not control a tape drive or decode proprietary backup formats.

The interface uses a dark Graphite theme, translucent panels, readable table rows and small red accents. The padlock reflects encryption of the currently open catalog. **Animation** toggles the background particles; animation also pauses for reduced-motion preferences and when the page is hidden. These controls require no external services.

## Import an existing WinCatalog export

Start with a CSV report containing filenames, paths, sizes and, if applicable, media labels. Common CSV/TSV layouts, EFU file lists and recognized XML reports are supported:

```sh
./allfiles-linux-amd64 import \
  --data "$PWD/data" \
  --file "/path/to/wincatalog-export.csv" \
  --label "Blu-ray archive" \
  --kind bluray
```

**Compatibility with your particular WinCatalog export still needs validation on a real sample.** This is not a promise to read every CSV/XML layout or WinCatalog's native working database. Keep the original catalog. Metadata import does not reconstruct document text; content indexing needs access to the original documents.

## Export, back up and move a catalog

Use **Export for USB** while no scan/import is running, or close the service and run:

```sh
./allfiles-linux-amd64 export --data "$PWD/data" --out "$PWD/usb-copy"
```

The output must be a **new folder under an existing directory**. Export refuses to overwrite an existing destination and creates a checked, compact database copy. Copy the executable separately. Add `--catalog-only` to exclude document content; that copy can search file metadata but not the missing text. Accessible originals can be rescanned to build content again.

An ordinary export preserves the source's encryption state: plaintext stays plaintext; an encrypted catalog produces an encrypted copy with the same password and its required `encryption.json`. Keep both databases, when present, and the matching manifest together. Do not copy individual live SQLite files while the application is running; use export for a consistent copy. Export is currently synchronous and has no progress/cancel control.

## Optional catalog encryption

AllFiles can encrypt the database files independently of VeraCrypt. It uses the maintained **Adiantum SQLite VFS**, reading and writing encrypted data directly without a plaintext working copy.

In **Export for USB**, select **Encrypt the copy with a password**, choose a new folder and confirm your password. For an already encrypted catalog, ordinary export keeps its password; **New password for the copy** changes only the new copy. The equivalent CLI command is:

```sh
./allfiles-linux-amd64 encrypt --data "$PWD/data" --out "$PWD/data-protected"
./allfiles-linux-amd64 serve --data "$PWD/data-protected" --no-browser
```

The terminal prompts without echoing your password. New passwords need at least 12 characters and at most 1024 UTF-8 bytes; spaces are preserved. An encrypted source asks for its current password before the new copy's password. `--catalog-only` is also supported.

**Encryption creates a new copy and leaves the original unchanged.** A previously plaintext original remains plaintext. Continue using the new encrypted directory when that is your intent. There is no password recovery. Keep `encryption.json` with the correct database files: it contains public format/KDF parameters and a random salt, not the password, but is essential to unlock the catalog.

Format v1 uses Argon2id with 3 passes, 64 MiB of memory and parallelism 4, then HKDF-SHA256 to derive separate catalog and content keys. Creating a newly protected copy generates a fresh salt. Passwords are not accepted through CLI arguments, environment variables, redirected input or MCP; unlock the service interactively first.

**Adiantum provides confidentiality at rest, not cryptographic authenticity or tamper detection.** It is deterministic for the same key, location and plaintext. It does not protect data from programs that can access an unlocked application, nor does it encrypt the original documents. See the [upstream Adiantum VFS documentation](https://pkg.go.dev/github.com/ncruces/go-sqlite3/vfs/adiantum). VeraCrypt remains useful for other files, browser profiles and backups.

## Connect an AI client with MCP

AllFiles offers an MCP **stdio** bridge. Your AI client starts `allfiles mcp`, which talks to an already running local AllFiles service. It does not open a second database or include an AI model.

Start and, if needed, unlock the service on a fixed port:

```sh
./allfiles-linux-amd64 serve --data "$PWD/data" --port 8787 --no-browser
```

For clients that use an `mcpServers` JSON configuration:

```json
{
  "mcpServers": {
    "allfiles": {
      "command": "/absolute/path/to/allfiles-linux-amd64",
      "args": ["mcp", "--url", "http://127.0.0.1:8787"]
    }
  }
}
```

Replace `command` with the actual absolute executable path and select the binary for your OS. Windows JSON paths need `/` or escaped `\\`. Configuration location depends on your AI client. Keep the service running; if you change its port, update `--url` too. The bridge accepts only literal `http://127.0.0.1:PORT` URLs, with no remote hosts, redirects or environment HTTP proxies.

| Tool | Purpose and bounds |
|---|---|
| `allfiles_sources` | List media using `offset`/`limit`, up to 200 per call |
| `allfiles_search` | Search `files`, `content` or `all`; optional `source_id` and `extension`; up to 50 results |
| `allfiles_scan` | Rescan an existing `source_id`, or add `root` plus `label`; content indexing defaults to enabled |
| `allfiles_status` | Read catalog status and current job progress |
| `allfiles_cancel` | Cancel the current scan/import job |
| `allfiles_file` | Metadata and text by file `id`; `offset` and `max_chars`, default 4,000 and maximum 20,000 characters |

For example: “Find files with kitchen in the name,” “Search document text for walnut cabinetry,” or “I added files to HOME-SSD-01; rescan that media and check its progress.” Scans require the source folder on the machine running AllFiles and return a job immediately. Use `index_content: false` for metadata-only scans. `allfiles_file` with `max_chars: 0` returns metadata without reading document text and omits `total_chars`.

**A local catalog does not make a cloud AI conversation local.** Requested names, paths, snippets and document text become visible to your AI client and its configured model provider. Use a fully local client/model setup if that is required. Treat returned document contents as data, not instructions.

## VeraCrypt and privacy boundaries

AllFiles does not mount VeraCrypt or request its password. Place `--data` inside a mounted container; AllFiles puts its own temporary files under that directory. For confidential use, also consider system-disk, swap and hibernation encryption. Neither catalog encryption nor a container guarantees removal of information from RAM, the system clipboard or other programs.

A normal browser may leave data outside the container in its profile. You can use a dedicated profile, with the service started using `--no-browser`:

```sh
# First terminal, inside your container
./allfiles-linux-amd64 serve --data "$PWD/data" --port 8765 --no-browser

# Second terminal, in the same folder
chromium --user-data-dir="$PWD/data/browser-profile" http://127.0.0.1:8765
```

Use the actual Chromium executable installed on your system. Do not enable profile synchronization if you need it to stay local. Close AllFiles and that browser before dismounting. Clipboard history/synchronization is controlled by your OS.

The local HTTP service is intended for a personal desktop session. It is not a multi-user authorization boundary: other local processes can access an unlocked service. Do not forward its port to the internet. A permanently hosted web edition would need separate authentication, HTTPS and server-side data handling; it is not included.

## Demo and development

Preview the interface with clearly synthetic data in a separate directory:

```sh
mkdir -p demo-data
./allfiles-linux-amd64 demo --data "$PWD/demo-data"
./allfiles-linux-amd64 serve --data "$PWD/demo-data"
```

Do not use the demo directory for your real catalog. `demo` seeds it and exits; `serve` opens the interface.

To build from source, use the Go version specified in `go.mod` or a compatible newer release:

```sh
go test ./...
go vet ./...
go build -trimpath -ldflags="-s -w" -o allfiles .
```

The first build downloads dependencies; the finished executable works offline. SQLite uses `github.com/ncruces/go-sqlite3` with `CGO_ENABLED=0`, without a C toolchain. Rebuild after changing the embedded web interface.

Use `sh scripts/build.sh 0.2.0` or PowerShell `.\scripts\build.ps1 -Version 0.2.0` to build all five targets into `dist/`: Linux amd64/arm64, macOS amd64/arm64 and Windows amd64. Packages include both guides and `THIRD_PARTY_NOTICES.txt`. CI runs native tests on Linux, Windows and macOS before cross-building; it does not publish releases automatically. Actions artifact storage is optional through the manual `upload_artifacts` input.

Performance depends on the actual archive, disk, query and content. Short/common queries and large document collections need their own measurements; there is no guaranteed database size or search latency. Reproducible synthetic benchmarks include `BenchmarkCatalog100K` and `BenchmarkEncryptedCatalog100K` in the Go tests. OCR, automatic synchronization between computers, original-file duplicate detection, direct LTO control and signed binaries are not included in this version.

Version 0.2.0 adds transactional catalog counters, index-driven content candidates, one snippet calculation per shared document per result page, bounded HTTP/MCP text pages, and removes redundant per-document writes during metadata import. It also bounds CSV/XML record allocation and shared import buffers. Explicit regression benchmarks are in `review_benchmark_test.go`; the Serbian guide records the paired before/after measurements and their scope.

Remaining work includes cursor pagination for deep pages, cheaper counts for broad searches, export progress/cancellation, and separate-process PDF extraction with strict resource limits. The PDF library can build an entire page's text before our output limit is checked; the existing input/output limits do not constitute a parser sandbox.

The separate [Go](experiments/go-search/README.md) and [Rust](experiments/rust-search/README.md) experiments compare identical SQL workloads against unencrypted synthetic data. They are not replacement applications and are excluded from the production executable. The **Compare Go and Rust search** workflow verifies every count and ordered ID page and records runtime versions, timings, peak process memory and benchmark executable sizes.

The [measured comparison and recommendations](experiments/README.md) include all three variants and both raw rounds. Native SQLite accounted for most of the broad-query improvement in this experiment; the production engine is unchanged because native integration must also address encryption and portable builds. These SQL measurements exclude complete application behavior and encryption.
