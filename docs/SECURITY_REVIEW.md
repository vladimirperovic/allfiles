# Security review — 2026-10-29

This review covers the current Rust application, both tracked Rust lockfiles, both Go modules, GitHub configuration, and public history. Tests use synthetic inputs. The review found and corrected an XML processing issue in addition to the ZIP fix already included in 0.1.5. Upgrade to 0.1.6 for the XML corrections.

## GitHub dependency inventory

GitHub's dependency graph reported `zip` 4.6.1 from the root `Cargo.lock` and the old 4.x requirement from `Cargo.toml`, while the default branch and published 0.1.5 source contained 9.0.5. Both manifests were marked parseable and below the size limit. The alert's update timestamp still preceded the fix. No other tracked manifest contained the old ZIP dependency, and no earlier custom dependency submission was found.

This discrepancy was in GitHub's indexed inventory, not in the release lockfile. The API does not expose the internal reason its static index failed to refresh. The security workflow now submits exact resolved dependency snapshots from the committed manifests. It includes every locked registry dependency, both Rust projects, and dependency edges; it fails rather than silently skipping unsupported or ambiguous records. The snapshot is tied to a commit SHA and uses a stable detector/correlator so later submissions replace earlier ones. Pull requests cannot submit snapshots with a write token.

GitHub documents that submitted dependency inventories take precedence over static parsing and receive normal vulnerability alerts. See [dependency submission](https://docs.github.com/en/rest/dependency-graph/dependency-submission). Alerts are resolved by correcting their dependency evidence, not by dismissing them or excluding the vulnerable package.

GitHub now lists ZIP 9.0.5 and `quick-xml` 0.41.0. ZIP alert #1 changed to **fixed**, with no dismissal recorded, and the open Dependabot alert count is zero. Its fix timestamp (11:30:05 UTC) preceded the first confirmed explicit snapshot (11:30:31 UTC): the new manifest push had already refreshed the static index. Snapshot submission adds a verifiable inventory refresh for subsequent runs; it is not claimed as the cause of that earlier state change. The first security workflow completed successfully, including both Rust lockfiles, all three Go configurations and the inventory submission.

## Corrected XML processing

[`RUSTSEC-2026-0194`](https://rustsec.org/advisories/RUSTSEC-2026-0194.html) describes quadratic work when checking duplicate XML attributes. AllFiles reached that iterator in XML imports and XLSX extraction. The previous import limit counted normalized fields, so distinct prefixed attribute names could collapse to the same key and evade that limit.

Version 0.1.6 pins `quick-xml` 0.41.0 and additionally limits each processed element to 256 raw attributes, with cancellation checks during iteration. Tests accept the boundary, reject oversized lists and preserve duplicate-name rejection. The upgrade also fixes [`RUSTSEC-2026-0195`](https://rustsec.org/advisories/RUSTSEC-2026-0195.html); its namespace reader is not used by AllFiles.

ZIP remains pinned to 9.0.5 with `validate_header_consistency()` enabled before Office XML is read. The application does not extract ZIP entries to disk. The existing tests cover conflicting headers, duplicate/overlapping entries and valid ZIP64/streamed documents.

## Checks and maintenance warnings

RustSec is checked against both lockfiles. Go vulnerability analysis covers both modules and distinguishes modules present in the dependency graph from packages/symbols actually used. The security workflow does not ignore vulnerability IDs. GitHub CodeQL extended analysis and secret scanning provide separate checks; a successful workflow alone is not a statement that its alert list is empty.

CodeQL uses the extended query suite with both remote and local input sources. Findings are reviewed at their exact source locations and dataflow paths. Synthetic passwords inside Rust test-only modules are classified as test data; explicit developer-selected paths, test executables and benchmark SQL are assessed against their intended local interfaces. Runtime FFI findings are checked against the actual output writes, null/error guards and library lifetime contracts. These reviews do not add blanket query exclusions or weaken the application checks. The Go reference tests also run on Linux so symbolic-link containment is exercised even when Windows cannot create test links.

Two maintenance findings remain visible:

- [`RUSTSEC-2026-0192`](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) marks `ttf-parser` 0.25.1 as unmaintained, with no patched version. It is a nonoptional transitive dependency of the current PDF library. Its font-creation entry point is not called by AllFiles or its text-extraction library. This is a maintenance warning, not a reported exploitable flaw in this application; replacing the PDF stack is not treated as a routine security patch.
- [`GO-2026-5932`](https://pkg.go.dev/vuln/GO-2026-5932) concerns the unmaintained OpenPGP packages inside `golang.org/x/crypto`. The Go reference uses Argon2, not OpenPGP. Analysis found no affected imported package or reachable symbol. Updating that module cannot remove an advisory with no fixed version.

History scanning covered 13 commits, four tags and 251 unique blobs before this security update. No recognized credentials, private keys, catalog databases or encryption manifests were found; synthetic test literals were checked in context. Pattern scans cannot prove that every possible secret is absent.

Synthetic application probes cover hostile Host/Origin/Fetch-Site values, session-token enforcement, request-size limits, non-overwriting exports and literal rendering of injected filenames, media labels, paths and document content. Runtime results are recorded in [validation](VALIDATION.md).

## Limits

This is a scoped security review, not a guarantee that the software is vulnerability-free. PDF parsing remains in-process; encryption protects confidentiality without authenticating maliciously modified data; local programs can access an unlocked service; release packages remain unsigned. See [SECURITY.md](../SECURITY.md) for these boundaries and private reporting instructions.
