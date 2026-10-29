# AllFiles 0.1.6

Security maintenance release for the portable local file catalog. Upgrade from 0.1.5 for safer handling of untrusted XML imports and XLSX documents. Catalog files and encryption remain compatible.

[Try the dashboard demo](https://vladimirperovic.github.io/allfiles/) · [User guide](https://github.com/vladimirperovic/allfiles/blob/main/README.md) · [Security review](https://github.com/vladimirperovic/allfiles/blob/main/docs/SECURITY_REVIEW.md)

## Security changes

- Updated `quick-xml` to 0.41.0, fixing RUSTSEC-2026-0194 and RUSTSEC-2026-0195. AllFiles reached the duplicate-attribute check affected by the first advisory; it does not use the namespace reader affected by the second.
- XML imports and XLSX cells now enforce a limit of 256 raw attributes per element and check cancellation during iteration. Namespace prefixes cannot bypass the limit by collapsing into one normalized field.
- Added regression coverage for oversized attribute lists, boundary acceptance, duplicate names and XML path compatibility.
- Retained ZIP 9.0.5 and explicit archive header-consistency validation.
- Added automated RustSec/Go checks, committed dependency-graph submissions and broader Dependabot coverage. Informational maintenance warnings remain documented in the security review.

## Downloads

| Computer | Archive |
|---|---|
| Windows Intel/AMD 64-bit | `allfiles-0.1.6-windows-amd64.zip` |
| Linux Intel/AMD 64-bit | `allfiles-0.1.6-linux-amd64.tar.gz` |
| Linux ARM64 | `allfiles-0.1.6-linux-arm64.tar.gz` |
| macOS Apple Silicon | `allfiles-0.1.6-darwin-arm64.tar.gz` |
| macOS Intel | `allfiles-0.1.6-darwin-amd64.tar.gz` |

Each archive contains a native executable, English documentation, MIT and dependency licenses, and internal checksums. Packages are unsigned. Linux builds target Ubuntu 24.04 and glibc 2.39 or newer. No separate runtime or database installation is required.

Download `SHA256SUMS.txt` alongside your archive. Verify with `sha256sum --check --ignore-missing SHA256SUMS.txt` on Linux, `shasum -a 256 <archive>` on macOS, or `Get-FileHash <archive> -Algorithm SHA256` in PowerShell. Compare the full hash with the matching filename before extraction.

Back up the complete existing data directory before upgrading. Create an empty data directory for a new catalog, then run the extracted executable with `serve --data <directory>`. AllFiles catalogs files; it does not back up the originals. Historical releases and tags remain unchanged. See [validation](https://github.com/vladimirperovic/allfiles/blob/main/docs/VALIDATION.md) and [security boundaries](https://github.com/vladimirperovic/allfiles/blob/main/SECURITY.md).
