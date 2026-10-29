# Go, Rust and SQLite: measured comparison

Native SQLite accounted for most of the improvement in this test. For broad queries, Go with native SQLite was about **2.6–2.7 times faster** than the Go engine used by the application at the time. Rust with the same native SQLite reduced those times by a further **1–3%**. This does not establish that rewriting the whole application in Rust would produce the same improvement.

A separate Rust query comparator was built for this experiment, not a complete Rust version of AllFiles. The comparators are excluded from production packages. At the time of the experiment, the interface, scanning, import, MCP and encryption remained in the Go application. The subsequent production migration is described below.

## Measurement conditions

- Run: [GitHub Actions, 26 October 2026](https://github.com/vladimirperovic/allfiles/actions/runs/40311607185), source commit `d91db50a1e4223fb31ac3de69fd92589a324c4cb`.
- Linux x86-64, AMD EPYC 9V45, 2 available virtual CPUs; Go 1.26.10 and Rust 1.99.1.
- The same synthetic catalog: **100,050 files**, including 50 locations of one shared document of about 1 MiB. This is not a large collection of distinct documents.
- The same SQLite **3.53.4**, identical SQL queries and parameters. Native Go and Rust link exactly the same static SQLite library; the source ID and all compile options were checked.
- Two sequential rounds, with program order reversed in the second. Each query has 3 warmups and 20 measured runs per round with a warm cache. Values below are the **mean of the two round medians**, not a pooled median of all 40 samples.
- Every sample checks the same total count and exact ordered IDs. SHA-256 checks of both databases verify that the programs did not modify the input.

The measured work is SQL result counting and a page of up to 50 IDs, including query preparation, binding, reading and finalization. It **excludes** HTTP, full result fields, snippets, indexing, PDF/Office extraction, encryption and the browser. These are not complete user-request timings, cold-start measurements or battery measurements.

## Results

Lower times are better. "Reference Go" uses the historical production `ncruces/go-sqlite3` version with the generated Go SQLite engine. "Native" means SQLite compiled from C source.

| Query | Reference Go, ms | Go + native SQLite, ms | Rust + native SQLite, ms |
|---|---:|---:|---:|
| Rare filename, 1 match | 2.021 | 0.783 | 0.693 |
| Rare path, 1 match | 1.324 | 0.661 | 0.553 |
| First page, no query | 0.062 | 0.057 | 0.024 |
| Page at item 90,000, no query | 22.865 | 10.263 | 9.791 |
| Common filename, 99,999 matches | 347.189 | 129.085 | 125.488 |
| Same common filename, page at item 90,000 | 367.667 | 138.714 | 136.452 |
| Single letter, 100,000 matches | 267.762 | 101.508 | 100.197 |
| Content, rare term in a shared document | 0.196 | 0.137 | 0.069 |
| Content, common term in a shared document | 0.205 | 0.134 | 0.073 |

| Comparator resource | Reference Go | Go + native SQLite | Rust + native SQLite |
|---|---:|---:|---:|
| Executable, MiB | 8.72 | 4.08 | 1.70 |
| Maximum measured process RAM, MiB | 19.37 | 19.89 | 10.63 |
| Same two databases, MiB | 35.04 | 35.04 | 35.04 |

Executable size and RAM refer **only to the small comparator programs**, without the full application's features or the browser. RAM is the maximum peak RSS across the two processes, measured with GNU `time`; it is not Go allocation volume. The databases contain exactly 36,544,512 + 200,704 bytes. Changing language did not reduce database size. The small content database also reflects sharing the same synthetic text.

Rust has a larger relative gain on the shortest queries, but the absolute difference is hundredths or tenths of a millisecond. The small Rust/native-Go difference on broad queries does not demonstrate a reliable practical language advantage: Rust's p95 is higher in some cases. Detailed p95 values, results, engine options and both rounds are preserved in the [raw JSON report](results/2026-10-26-linux.json).

## State after migration

Historical version 0.3.0 introduced the complete Rust backend with native SQLite and a compatible Adiantum VFS. The current public release 0.1.5 retains that Rust implementation; historical version labels are unchanged. The tables above remain historical measurements of small SQL comparators, not the full application's size or speed. See the [main guide](../README.md#public-release-015-and-upgrading) for migration guidance and [application benchmark history](application-history.md) for the separate full-application measurements.

## Further experiments

**SQLite + FTS5** remains a reasonable fit for AllFiles: a portable local format without a separate server, plus a text-search index. See [SQLite's guidance on local applications](https://www.sqlite.org/whentouse.html) and the [FTS5 documentation](https://www.sqlite.org/fts5.html).

Before the Rust migration, this isolated test suggested native SQLite within the Go application as the lowest-risk next experiment. That was not a simple package replacement: encryption used a custom Go VFS, so a native variant needed compatible encryption, portable builds and renewed testing. This was the conclusion before the production Rust migration in 0.3.0. Further broad-query counting improvements and cursor pagination may help without changing language.

[Tantivy](https://github.com/quickwit-oss/tantivy) is a Rust full-text search library and a candidate for a separate large content index. It **was not benchmarked here**; no claim is made that it would be faster or smaller for your documents. It would require another index format, catalog synchronization and encryption support. A representative WinCatalog export and varied PDF/Office collection are needed for a more representative comparison.

## Reproduce

The manually dispatched **Compare Go and Rust search** workflow generates a synthetic catalog and runs all three variants. Local procedures are in the [Go guide](go-search/README.md) and [Rust guide](rust-search/README.md); the shared runner is `scripts/compare-search.py`. For separate production-application optimization measurements, see the [main guide](../README.md#historical-measurements-and-remaining-work) and [application benchmark history](application-history.md).
