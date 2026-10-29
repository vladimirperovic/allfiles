# Architecture

AllFiles is a Rust executable with an embedded HTML/CSS/JavaScript interface and native SQLite/FTS5. It binds a local HTTP service to `127.0.0.1`; the browser renders paged results rather than loading the whole catalog. The Go implementation at the repository root is retained for reference and format compatibility, and is not part of the Cargo executable.

## Components

| Component | Responsibility |
|---|---|
| `src/main.rs`, `src/cli.rs` | Commands, interactive password entry, process startup and shutdown signals |
| `src/server.rs` | Embedded assets, HTTP request checks, API, job lifecycle, query cancellation and export reservation |
| `src/store.rs`, `src/sql/` | SQLite schema, metadata/content operations, search, previews, statistics and export |
| `src/fts_tokens.rs` | SQLite's `unicode61` tokenizer for content snippet matching |
| `src/scanner.rs` | Filesystem traversal, incremental metadata/content batches and progress |
| `src/extract.rs` | Bounded text extraction from supported PDF, Office, OpenDocument and text files |
| `src/import.rs` | Validation and streaming CSV/TSV, EFU and recognized XML import |
| `src/crypto.rs`, `src/encrypted_vfs.rs` | Password-derived keys, encryption manifest and encrypted SQLite file access |
| `src/mcp.rs` | `stdio` JSON-RPC tools forwarded to the existing local HTTP service |
| `web/` | Shared Serbian/English interface, state recovery, document preview and optional particles |
| `src/tests.rs`, module tests, `scripts/` | Synthetic correctness, lifecycle, integration and performance checks |

The public browser demo is a separate static simulation. It must not start a service, open a real catalog or request local filesystem permissions.

## Data layout

```text
data/
  catalog.sqlite     media, directories, files, search indexes and counters
  content.sqlite     compressed text, document links and content FTS index
  encryption.json    required key-derivation metadata for encrypted catalogs
  .allfiles.lock     operating-system lock for this data directory
```

The content database is optional until needed. File records refer to source media and directories; content links bind a file version to an extracted document. Shared extracted text can use a shared document row. This storage optimization is not original-file duplicate detection.

Name/path lookup uses normalized fields and trigram indexes, with an ordered scan for short or common terms. Content lookup uses FTS5 `unicode61 remove_diacritics 2`; snippet matching uses that same tokenizer. Preview and MCP text pages count Unicode characters while enforcing byte limits on stored/decompressed text.

Transactional counters avoid resumming all files on every status poll. Connection-local availability tables/triggers describe which persisted content links are currently usable. When a content database is missing, mismatched or incomplete, metadata remains searchable and the response reports available content separately from stored extraction metadata. Opening an incomplete copy does not erase persisted extraction status.

One process owns a catalog directory. SQLite uses rollback journals, `synchronous=FULL`, foreign keys and in-memory temporary tables. Export uses SQLite backup into a new directory, optionally with new encryption keys; it never overwrites an existing destination. Use export for a running catalog and close the process before manual copies or dismounts.

## Work, requests and cancellation

The shared store is serialized by a mutex. Filesystem traversal and text extraction happen outside that lock where possible; updates use bounded batches. Scan/import jobs publish progress independently. Cancelling preserves committed work and avoids final stale-file cleanup for an incomplete scan.

`/api/status` carries an `instance_id` and monotonic `job_revision`. Accepted jobs return their initial job and revision together. The interface uses these values and local request epochs to reject late responses, identify restarts and avoid reconnecting after intentional shutdown.

Status uses cached counters when the store is busy and sets `stats_cached`. A job refreshes counters before publishing a terminal state; an idle poll refreshes them from the store. Export keeps the existing synchronous API, reserves the mutation slot briefly, then performs the copy without holding the job mutex. Status and cancellation can therefore remain responsive while export holds the store.

Search and file-preview requests share a limit of 16 active operations. Additional requests receive HTTP 503. Their 15-second deadline starts before waiting for the store. A dropped HTTP request signals cancellation; waiting checks, SQLite progress callbacks and text-decompression checks stop work cooperatively. This limits abandoned work but is not a hard real-time guarantee or a parser sandbox.

MCP talks to the running service rather than opening a second store. Its dedicated bounded output writer owns a JSON frame once it is queued, so aborting a tool request cannot truncate that frame. EOF drains accepted work; output failure closes the writer and releases waiting responses. Tool cancellation drops the HTTP operation and propagates to the server's request cancellation.

## Security boundaries and limits

The service checks Host/Origin and uses a session token for mutating requests. This is browser request protection, not authentication against other local processes. The embedded interface does not require a cloud service. An external MCP client can nevertheless forward requested results to a remote model.

Adiantum encrypts database storage through a VFS without an unencrypted working database. Manifest v1 uses Argon2id (3 passes, 64 MiB, parallelism 4), a random salt and HKDF-SHA256 keys for the two databases. Encryption protects confidentiality at rest, not authenticity. The manifest and password are both required; an encrypted copy does not remove the original unencrypted copy.

Input documents are limited to 256 MiB, extracted UTF-8 text to 8 MiB, expanded Office XML to 32 MiB per part/64 MiB total, and PDF traversal to 20,000 pages. Import has additional record/token and shared-buffer limits. PDF parsing still runs in-process, so these are not strict process memory/time limits. See the [user guide](../README.en.md) and [security policy](../SECURITY.md).

Independent read connections, cursor pagination, export progress/cancellation and isolated document parsing remain possible future changes. They must preserve encrypted-format compatibility, coherent job/status behavior and bounded resource use.
