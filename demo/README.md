# GitHub Pages dashboard demo

The demo opens the actual AllFiles dashboard in English with 420 invented files on seven media: one LTO tape, three Blu-ray discs, one computer and two external disks. Search, content previews, filters, sorting, pagination and English/Serbian switching run in the browser. A one-time migration sets existing demo sessions to English; later language choices remain saved.

Build with Python 3.11 or newer:

```sh
python scripts/build-demo.py --output dist/pages
```

The version comes from `Cargo.toml`; `--version 0.1.5` can override it. The builder copies the production assets without changing them, injects this directory's demo adapter and styles, and writes a source-hash manifest. It refuses unexpected files in the output directory so unrelated data cannot enter the Pages artifact. Generated output is ignored by Git.

`adapter.js` answers the dashboard's read requests with deterministic synthetic records in memory. It has no network fallback. Desktop operations display a short explanation, and writes are rejected. No real files, credentials, databases or user catalogs are included. A restrictive Content Security Policy also disables network connections and form submissions.

The browser regression mounts the generated site under `/allfiles/`, checks the dashboard interactions and verifies that no API or external network requests occur:

```sh
node scripts/audit_demo.mjs dist/pages audit/demo
```

The test uses `playwright-core` installed locally or in `.tmp/ui/node_modules`, and Microsoft Edge on Windows by default. Set `BROWSER_PATH` to use another Chromium executable. The Pages workflow builds with Python, runs these checks with Node.js and pinned Playwright Chromium, and deploys only after they pass.
