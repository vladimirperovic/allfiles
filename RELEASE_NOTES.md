# AllFiles 0.1.5

Public MIT release of AllFiles, a portable local catalog for computers, external disks, Blu-ray discs and LTO archives. Search filenames and extracted document text even when the original media are offline.

[Try the dashboard demo](https://vladimirperovic.github.io/allfiles/) · [User guide](https://github.com/vladimirperovic/allfiles/blob/main/README.en.md) · [Full changelog](https://github.com/vladimirperovic/allfiles/blob/main/CHANGELOG.md)

## Downloads

| Computer | Archive |
|---|---|
| Windows Intel/AMD 64-bit | `allfiles-0.1.5-windows-amd64.zip` |
| Linux Intel/AMD 64-bit | `allfiles-0.1.5-linux-amd64.tar.gz` |
| Linux ARM64 | `allfiles-0.1.5-linux-arm64.tar.gz` |
| macOS Apple Silicon | `allfiles-0.1.5-darwin-arm64.tar.gz` |
| macOS Intel | `allfiles-0.1.5-darwin-amd64.tar.gz` |

Each archive contains one native executable, the English user guide, MIT and dependency licenses, documentation, and checksums. No separate runtime or database installation is needed. These packages are unsigned. macOS and Windows may require approval to run a downloaded application. Linux packages are built on Ubuntu 24.04 and require glibc 2.39 or newer.

Download `SHA256SUMS.txt` alongside your archive. Verify with `sha256sum --check --ignore-missing SHA256SUMS.txt` on Linux, `shasum -a 256 <archive>` on macOS, or `Get-FileHash <archive> -Algorithm SHA256` in PowerShell. Compare the complete hash to the matching filename in the checksum file before extraction.

Create an empty data directory, then run the extracted executable with `serve --data <directory>`. It opens the local browser interface. See the user guide for platform-specific commands, imports, encryption and MCP setup.

## Changes

- Compact Explorer-style file rows, searchable/filterable media lists, direct page navigation and improved keyboard/mobile navigation for large archives.
- Updated the ZIP dependency and enabled strict header-consistency checks for Office documents, with regression coverage for malformed archives and valid ZIP64/streamed documents.
- Corrected frontend/backend status races, restart recovery, completed-job refresh and stale file details.
- Bounded concurrent search work, cancellation on disconnect, and responsive status during exports.
- Corrected missing-content statistics, Unicode search/snippets, document extraction and catalog imports.
- Faster statistics, media aggregation, interleaved imports and filename-only previews.
- Public browser demo opening directly on a fictional dashboard with one LTO, three Blu-ray media and three disks.

This version deliberately uses **0.1.5** for the current Rust application; historical development tags 0.2.0 and 0.3.0 remain unchanged. Back up the entire existing data directory before upgrading. AllFiles catalogs files; it does not back up the originals. See [validation](https://github.com/vladimirperovic/allfiles/blob/main/docs/VALIDATION.md) and [security boundaries](https://github.com/vladimirperovic/allfiles/blob/main/SECURITY.md).
