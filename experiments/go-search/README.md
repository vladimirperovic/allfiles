# Isolated Go SQLite query comparator

This experimental command measures SQL **count plus an ordered page of file IDs** against the same unencrypted synthetic fixture used by `../rust-search`. It is not the AllFiles application and does not include scanning, document extraction, compression, snippets, full metadata, encryption, HTTP, the browser interface or MCP.

Two builds share the same Go executor:

- `go-pure`: the production `ncruces/go-sqlite3` version and Go SQLite engine, with FTS5 registered as in AllFiles; `CGO_ENABLED=0`.
- `go-native`: `mattn/go-sqlite3`, linked to the **same static SQLite 3.53.4 library** as the Rust comparator; CGO required, statement cache explicitly disabled. Both native executors use SQLite `NO_MUTEX` on one connection, with operations serialized by the executor.

The experiment has a separate `go.mod` and `go.sum`; native comparison dependencies do not enter the production application. Its tests are separate from the root module's `go test ./...`.

## Build and run

From the repository root, build the production-style Go variant:

```sh
mkdir -p .tmp
CGO_ENABLED=0 go -C experiments/go-search build -trimpath -o "$PWD/.tmp/go-pure" .
go -C experiments/go-search test ./...
```

On Linux, build the native variant against the shared, checksum-verified amalgamation. Sourcing the helper builds the library and exports its paths:

```sh
source experiments/rust-search/build-native-sqlite.sh
CGO_ENABLED=1 \
  CGO_CFLAGS="-I$SQLITE3_INCLUDE_DIR" \
  CGO_LDFLAGS="-L$SQLITE3_LIB_DIR -lm -ldl -lpthread" \
  go -C experiments/go-search build -trimpath \
    -tags 'native libsqlite3 sqlite_fts5' -o "$PWD/.tmp/go-native" .
```

Use those same environment variables and build tags with `go test` to test the native variant. The helper requires a C compiler, `ar`, `curl`, `unzip` and `sha256sum`; Go itself does not download the SQLite amalgamation.

Run either binary against an existing fixture and manifest:

```sh
.tmp/go-pure --data /absolute/path/to/fixture \
  --cases /absolute/path/to/cases.json --iterations 20 --warmup 3
```

The command needs `catalog.sqlite` and `content.sqlite`, opens both read-only, then enables SQLite `query_only`. An `encryption.json` manifest is rejected. It checks SQLite version 3.53.4, records the engine source ID and compile options, and exits on any query error or mismatch. JSON is written to standard output only after every case succeeds; diagnostics use standard error.

## Shared manifest

```json
{
  "format": 1,
  "cases": [{
    "name": "example",
    "count_sql": "SELECT COUNT(*) FROM files WHERE extension=?",
    "page_sql": "SELECT id FROM files WHERE extension=? ORDER BY id LIMIT 50",
    "args": ["pdf"],
    "expected_count": 2,
    "expected_ids": [1, 2]
  }]
}
```

Both statements receive the same string arguments. The count query returns one integer; the page query returns one integer ID per row in a deterministic order. Expected values must come from the shared fixture, not from this example.

## Timing and interpretation

Each sample prepares, executes and closes the count statement, then prepares, executes, reads and closes the ID-page statement. All of that is timed, including Go binding and result materialization overhead. Query preparation is not cached between samples, matching the current application's query behavior. Connection creation, FTS registration, manifest loading, warmup, correctness checks and JSON serialization are excluded.

Every warmup and measured sample is checked against the expected count and exact ordered IDs. The report contains median, nearest-rank p95 and minimum milliseconds. For even sample counts, median is the mean of the middle two sorted values; p95 is element `ceil(0.95 * n) - 1`.

Use the same machine, fixture, query order, warmup and sample counts for all variants. Repeated runs on a warm filesystem cache are not cold-start or disk benchmarks. A `go-pure` versus `rust-native` difference combines SQLite implementation, compiler, binding and language effects. Comparing `go-native` to `rust-native` helps isolate the binding/language part, while their shared native SQLite build keeps its source and compile flags aligned. Neither comparison establishes that rewriting the whole application in Rust would produce the same improvement.
