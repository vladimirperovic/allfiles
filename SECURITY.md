# Security

## Reporting a vulnerability

Please use [GitHub's private vulnerability reporting](https://github.com/vladimirperovic/allfiles/security/advisories/new) when it is available. Include the affected version, platform, impact and a minimal synthetic reproduction. If that channel is unavailable, open an issue requesting a private reporting channel **without exploit details or sensitive data**. Do not post a vulnerability reproduction publicly before coordination with the maintainer.

Never send personal catalogs or original private documents, passwords, tokens, `encryption.json`, browser profiles or unredacted paths/logs. A generated example with fictional data is usually sufficient. No response deadline or security certification is promised. Fixes target the current public release and `main`; historical reference releases are not maintained as separate security branches.

## Local service boundary

AllFiles binds its HTTP service to `127.0.0.1`. The service checks browser request origins/hosts and requires a session token for mutating API requests. These checks reduce unwanted browser requests; they do not authenticate other local programs or users. Another process able to reach the unlocked local service can read catalog data and obtain a session token.

Do not expose the service with port forwarding, a public reverse proxy or a network tunnel. The application does not provide hosted multi-user authentication or HTTPS. Its static public demo uses fictional data and is separate from the local service.

## Stored data and encryption

- `catalog.sqlite` contains names, paths and metadata; `content.sqlite` can contain private extracted document text. A catalog is not a backup of the original files.
- Password protection uses Adiantum with Argon2id/HKDF-derived keys. It protects confidentiality at rest, **not cryptographic authenticity or resistance to malicious modification**.
- A protected copy requires the matching `encryption.json` and password. There is no forgotten-password recovery. Creating an encrypted copy does not encrypt or erase a previously unencrypted source directory.
- While unlocked, data exists in process memory and can be shown in the browser or returned through MCP. Disk encryption does not isolate the application from malware, debuggers, another privileged user, swap/hibernation or clipboard history.
- Export to a new protected destination, and keep all required files together. Close the application before manually copying databases or dismounting storage. Do not assume that copying only the main `.sqlite` file while it is open produces a complete backup.

VeraCrypt is optional and managed outside AllFiles. Protect browser profiles, backups and the operating system as appropriate for your data. The application does not guarantee secure erasure of old copies or memory.

## MCP and external clients

The MCP bridge accepts only an explicit `http://127.0.0.1:PORT` service address and does not unlock a catalog itself. It returns requested names, paths, metadata and document excerpts to the connected client. If that client uses a remote model, it may send those results to its provider. A local database alone does not make the AI conversation local. Only connect clients you trust and understand.

## Untrusted documents and imports

Text extraction and catalog import enforce input, output and allocation limits. These limits do not constitute a sandbox: the PDF parser runs in the application process and may process a page before checking output/cancellation limits. Do not use AllFiles as an isolation boundary for actively hostile documents. Symlinks and the application's own data directory are skipped during scanning, but filesystem contents can change while a scan runs.

The supported formats and exact limits are in the [user guide](README.en.md#add-files-and-search). Automated tests and local audit results are regression evidence, not a guarantee that the application has no vulnerabilities.

## Downloads

The [security review](docs/SECURITY_REVIEW.md) records the 0.1.6 XML fixes and the scope of dependency, code and application checks. The security workflow checks both Rust lockfiles and both Go modules and refreshes GitHub's dependency inventory from the committed source. Informational maintenance warnings remain visible; they are not silently ignored.

Use assets linked from the [official repository releases](https://github.com/vladimirperovic/allfiles/releases/tag/v0.1.6) and verify the published SHA-256 checksums. Checksums detect a mismatched download; they are not a substitute for a trusted source or a digital signature. Current packages are unsigned. Dependency licenses are listed in [THIRD_PARTY_NOTICES.txt](THIRD_PARTY_NOTICES.txt).
