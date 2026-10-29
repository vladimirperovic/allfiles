# Historical full-application measurements

These measurements were previously recorded in the Serbian main README and are preserved here in English. Version labels, workloads, results and reproduction commands retain their historical meaning. They are not measurements of every feature or a guarantee for public release 0.1.5. Current release evidence is in [validation](../docs/VALIDATION.md).

## Go 0.2.0 and Rust 0.3.0 HTTP comparison

Measured on the same Windows computer with the same unencrypted synthetic catalog of **100,050 files**: two sequential rounds with reversed program order, three warmups and 15 measured requests per query. Values are the mean of the two round medians. Timing covers the complete HTTP response over a persistent local connection, including metadata and snippets; it excludes the browser and indexing.

| HTTP search | Go 0.2.0 | Rust 0.3.0 |
|---|---:|---:|
| Rare filename | 1.716 ms | 1.440 ms |
| First page, no query | 0.510 ms | 0.507 ms |
| Page at item 90,000 | 31.712 ms | 15.736 ms |
| Common filename, 99,999 matches | 319.589 ms | 129.772 ms |
| Common filename, page at item 90,000 | 384.653 ms | 167.324 ms |
| Single letter, 100,000 matches | 241.739 ms | 118.776 ms |
| Content: 50 locations of one document | 6.929 ms | 4.960 ms |

Windows executable size changed from **14.24 MiB to 6.18 MiB**, about 57% smaller. The catalog format stayed the same; changing language alone did not reduce database size. The comparison also changed the SQLite engine/version (Go 3.53.4, native Rust 3.54.1), so it does not isolate language effects. The content workload used 50 file locations sharing one text of about 1 MiB, not a large varied document collection. Cold storage, encryption, RAM and battery use were not measured.

[Raw samples](results/2026-10-26-rust-app-windows.json) contain every observation. `scripts/compare-http.py` checks the total and exact ordered IDs for each response and does not modify the source fixture databases. The subsequent Rust optimization table remains in the [main guide](../README.md#historical-rust-030-optimization-measurements-26-october-2026), with [raw optimization samples](results/2026-10-26-rust-optimizations-windows.json).

## Historical Go 0.2.0 optimizations

The 0.2.0 review removed repeated full-file statistics aggregation, repeated decompression of the same document per result, and a correlated subquery that repeated content search for every file. Counters update in the same transaction as data; existing catalogs are counted once during upgrade. Preview/MCP responses return requested text pages, and metadata import avoids redundant per-document writes.

Paired synthetic measurements on the same Windows computer, Go 1.26.10, approximately 100,000 files and a warm cache:

| Operation | Before optimization | After |
|---|---:|---:|
| Catalog statistics | 29.72 ms | 0.169 ms |
| First page, no query | 27.28 ms | 0.202 ms |
| Common filename term (`drawing`) | 644.28 ms | 341.33 ms |
| Content: 50 locations of one approximately 1 MiB document | 1,078.82 ms | 6.92 ms |
| Total memory allocations for the preceding search | 724 MiB | 4.41 MiB |
| Import 10,000 PDF entries, metadata only | 65.95 s | 0.875 s |

The final row measures importing a CSV export, not extracting text from 10,000 PDF documents. Allocation volume is not peak RAM. These are specific test workloads, not guarantees for a real archive. The historical `v0.2.0:review_benchmark_test.go` contains explicit benchmarks that do not run in ordinary tests.

## Earlier 0.1.0 catalog sample

On Windows with an Intel i7-14700KF, **100,000 synthetic files** in approximately 7,000 folders occupied **34.96 MiB** in an unencrypted metadata catalog without document text. Import took **10.45 s**. Two selective name/path queries averaged **2.0–2.1 ms** over 30 warm-cache repetitions. Short/common terms, long paths, different disks, encryption and document content can produce different results.

Reproduction command from historical tag `v0.2.0`:

```sh
go test -v -run '^$' -bench BenchmarkCatalog100K -benchtime 30x -count 1
```

An encrypted copy of the same synthetic catalog also returned **2.0–2.1 ms** for those queries over 30 warm-cache repetitions. This does not measure first unlock, cold-disk reads, scanning or write overhead. Reproduction command from historical tag `v0.2.0`:

```sh
go test -v -run '^$' -bench BenchmarkEncryptedCatalog100K -benchtime 30x -count 1
```
