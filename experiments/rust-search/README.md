# Rust search microbenchmark

This is a small read-only comparator, **not a Rust version of AllFiles**. It has no UI, scanning, document extraction, encryption, import, export, MCP or web server. Use unencrypted synthetic fixtures, not a working private catalog.

It runs the same count SQL and first-page ID SQL as the Go comparator against the same `catalog.sqlite` and attached `content.sqlite`. String arguments and expected counts/ordered IDs come from one shared format-1 JSON manifest. Every warmup and measured result is verified; any mismatch fails the process.

Each sample includes prepare, bind, execute, read and finalize of both statements. No statement cache is used. File opening, schema setup, JSON parsing/output and correctness checks are outside the timed sample. The single connection stays open across cases, so these are warm-cache query measurements, not cold startup or end-to-end application timings. Median averages the middle two samples for even counts; p95 uses nearest rank. Timing uses `Instant` and reports milliseconds.

## Build on the comparison Linux runner

Requires a Rust toolchain (edition 2024, Rust 1.89 or newer), a C compiler, `ar`, `curl`, `unzip` and `sha256sum`.

```bash
source experiments/rust-search/build-native-sqlite.sh
cargo build --release --locked --manifest-path experiments/rust-search/Cargo.toml
cargo test --locked --manifest-path experiments/rust-search/Cargo.toml
experiments/rust-search/target/release/allfiles-rust-search \
  --data /path/to/synthetic-catalog --cases /path/to/cases.json \
  --iterations 20 --warmup 3 > rust-results.json
```

The build helper downloads the official SQLite **3.53.4** amalgamation over HTTPS, verifies its pinned SHA-256, and builds a static native library with `-O2`, `SQLITE_THREADSAFE=1` and `SQLITE_ENABLE_FTS5`. Cargo links this library through `SQLITE3_LIB_DIR`, `SQLITE3_INCLUDE_DIR` and `SQLITE3_STATIC`. The CLI refuses a different SQLite version and reports the actual compile options. The normal bundled library shipped in `libsqlite3-sys` 0.38.2 is **3.53.2**, which is why this experiment deliberately uses the pinned external library.

CI can instead run `bash experiments/rust-search/build-native-sqlite.sh "$RUNNER_TEMP/sqlite-native"`. That produces `include/sqlite3.h`, `lib/libsqlite3.a` and `lib/pkgconfig/sqlite3.pc` under the chosen prefix. The pkg-config metadata prevents the Rust build script from selecting a system SQLite installation instead. Set `SQLITE3_INCLUDE_DIR` to its `include` directory, `SQLITE3_LIB_DIR` to its `lib` directory, and `SQLITE3_STATIC=1` for later Cargo steps. A unit test checks the actual linked SQLite version and FTS5 support. The Go-native comparator must link this same library.

Both database URI paths use `mode=ro`, and the connection enables `query_only`, a 5-second busy timeout, `temp_store=MEMORY` and `trusted_schema=ON`. Do not edit the fixture while a comparison runs. The case manifest is benchmark code and must be trusted; it is not an untrusted query API.

## Interpreting results

Run the Go and Rust comparators on the **same runner and fixture**, preferably in alternating order and multiple process runs. Report data size, case names, SQLite versions, compile options, toolchain versions and measurement spread. Native rusqlite and AllFiles' Go/WASM SQLite driver have different execution and FFI paths. Even with identical SQLite versions and SQL, any measured difference belongs to the whole tested stack; it does not isolate the Rust language from the Go language or establish that a full application rewrite will be faster.

The output is one JSON object on stdout. Errors go to stderr with a nonzero exit status. The result includes observed count and ordered IDs to make correctness reviewable alongside timings.

## Sources and licenses

- [rusqlite documentation](https://docs.rs/rusqlite/0.40.2/rusqlite/): native Rust SQLite wrapper, MIT.
- [libsqlite3-sys build/link configuration](https://github.com/rusqlite/rusqlite#notes-on-building-rusqlite-and-libsqlite3-sys): SQLite FFI, MIT.
- [SQLite amalgamation](https://www.sqlite.org/amalgamation.html) and [licensing](https://www.sqlite.org/copyright.html): SQLite is public domain.
- `serde`, `serde_derive`, `serde_json` and their locked transitive dependencies retain their upstream licenses; see [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Cargo.lock is committed so CI uses a reproducible dependency graph. This experiment is kept separate from production builds and release binaries.
