# Public release 0.1.5

This release publishes the current Rust AllFiles application under the [MIT license](../LICENSE). The chosen public version is **0.1.5**, even though historical development tags include 0.2.0 and 0.3.0. Those tags, release artifacts and benchmark labels are not rewritten. Use the explicit [v0.1.5 release page](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.5) to select this public package.

## What is public

- Rust application sources, embedded interface and synthetic regression scripts.
- Go reference/compatibility sources and historical synthetic performance experiments.
- User/developer documentation and third-party license notices.
- Platform packages that are actually attached to the release, with SHA-256 checksums.
- A separate static [browser demo](https://vladimirperovic.github.io/allfiles/).

Personal databases, original documents, passwords, encryption manifests, local exports, browser profiles and machine-specific audit output must stay outside the public repository and archives. Published validation should describe sanitized workloads and results; it must not require a private catalog to reproduce.

## Browser demo

The demo opens directly on the dashboard with fictional files and **exactly 1 LTO tape, 3 Blu-ray media and 3 disks**: one computer and two external drives. It is a browser-only simulation, not a hosted catalog service. It has no connection to a visitor's local AllFiles instance and does not scan, import or unlock real data. Search/filter/preview interactions are demonstrations over synthetic records.

The production program still requires its local executable and existing `--data` directory. The static demo does not change that model or make a local catalog publicly accessible.

## Packages and validation

| Archive | Executable | Target |
|---|---|---|
| `allfiles-0.1.5-windows-amd64.zip` | `allfiles-windows-amd64.exe` | Windows Intel/AMD |
| `allfiles-0.1.5-linux-amd64.tar.gz` | `allfiles-linux-amd64` | Linux Intel/AMD, including Omarchy |
| `allfiles-0.1.5-linux-arm64.tar.gz` | `allfiles-linux-arm64` | Linux ARM64 |
| `allfiles-0.1.5-darwin-arm64.tar.gz` | `allfiles-darwin-arm64` | macOS Apple Silicon |
| `allfiles-0.1.5-darwin-amd64.tar.gz` | `allfiles-darwin-amd64` | macOS Intel |

This table describes target names, not a claim that every artifact is available or tested. Local Windows runtime/browser/integration checks are recorded in [VALIDATION.md](VALIDATION.md). The native CI matrix must be judged by its individual completed runs. A compiled artifact alone is not evidence of runtime behavior on another device. Current packages are unsigned.

Native CI Linux packages target Ubuntu 24.04 and glibc 2.39 or newer. This is the distribution target, not a portability claim for older Linux systems. Build from source for other environments and verify runtime behavior there.

Release archives include the appropriate executable, both user guides, license, changelog, contributor/security guidance, documentation, third-party notices, `VERSION` and an internal `SHA256SUMS` file. The release also provides `SHA256SUMS.txt` for the five complete archives. They must not include data directories or a populated personal catalog. Users should verify the published archive hash before extraction and follow the [user guide](../README.en.md).

## Package a tested build

Run packaging from the source repository with Python 3 after building and testing the executable on its target platform. The scripts use the Python standard library. Choose the platform matching the binary; packaging does not compile or independently prove its architecture.

```sh
python scripts/package-release.py --binary target/release/allfiles --platform linux-amd64 --output dist/release
```

For Windows, use `target/release/allfiles.exe` and `--platform windows-amd64`; for other native platforms use `linux-arm64`, `darwin-amd64` or `darwin-arm64`. Account for custom Cargo target directories. The script reads `VERSION`, uses an explicit public-file allowlist, creates a versioned archive, verifies its contents and prints its SHA-256 hash.

Collect all five native archives in one directory, then generate the release checksum file:

```sh
python scripts/prepare-release.py --directory dist/release
```

This checks the expected archive names and writes `SHA256SUMS.txt`; it does not replace runtime tests. Verify an archive with `sha256sum --check --ignore-missing SHA256SUMS.txt` on Linux, `shasum -a 256 <archive>` on macOS, or `Get-FileHash <archive> -Algorithm SHA256` in PowerShell. On macOS/Windows, compare the complete output hash with the matching filename in `SHA256SUMS.txt`.

The tag-triggered release workflow waits for the five native jobs, collects their archives and runs the preparation script before publishing. The Pages workflow separately builds and deploys the fictional dashboard demo. A workflow configuration or queued run does not mean either publication has succeeded.

## Maintainer release checks

1. Keep the release version consistent in application metadata, help/version output, package names and release notes. Preserve historical tags and benchmark identifiers.
2. Run formatting, Clippy, Rust tests, the release build and relevant synthetic HTTP/MCP/storage/extraction/frontend regressions against that executable. Record unavailable fixtures and skipped checks explicitly.
3. Verify frontend state recovery, both languages, bounded preview pages, export/encryption and shutdown. Use generated files and media only.
4. Review the exact staged files and archive contents for personal data, secrets, temporary paths and generated artifacts that do not belong in source control. Keep MIT and dependency notices in distributed packages.
5. Verify each package's platform and checksum. Record local validation separately from CI results; do not label pending matrix jobs as passed.
6. Check the deployed demo URL, direct dashboard entry, fictional labels, media counts and absence of calls to a real local backend. Check the download links after publication.
7. Publish the intended commit as `v0.1.5` without moving existing historical tags. Confirm the repository, release assets and demo are accessible publicly, and update the validation record with actual outcomes.

For a functional problem, request a small synthetic reproduction through [Issues](https://github.com/vladimirperovic/allfiles/issues). Security reports follow [SECURITY.md](../SECURITY.md); do not request private catalogs, documents or passwords.
