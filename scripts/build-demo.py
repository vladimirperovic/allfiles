#!/usr/bin/env python3
"""Build the GitHub Pages dashboard from the production UI and invented data only."""
import argparse
import hashlib
import json
import pathlib
import re
import shutil
import tomllib


def main():
    root = pathlib.Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=pathlib.Path, default=root / "dist" / "pages")
    parser.add_argument("--version", default=None)
    args = parser.parse_args()
    version = args.version or tomllib.loads((root / "Cargo.toml").read_text("utf-8"))["package"]["version"]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?", version):
        parser.error("Invalid version")
    destination = args.output.resolve()
    if destination == root or destination == root / "web" or destination == root / "demo":
        parser.error("Use a dedicated output directory, such as dist/pages")
    destination.mkdir(parents=True, exist_ok=True)
    assets = ["app.js", "i18n.js", "particles.js", "style.css", "logo.svg"]
    allowed = set(assets) | {"index.html", "demo.js", "demo.css", "demo-config.js", "demo-build.json", ".nojekyll"}
    unexpected = [item.name for item in destination.iterdir() if item.name not in allowed or not item.is_file() or item.is_symlink()]
    if unexpected:
        parser.error("Output directory contains unexpected files; use a fresh directory: " + ", ".join(sorted(unexpected)))
    for name in assets:
        shutil.copyfile(root / "web" / name, destination / name)
    shutil.copyfile(root / "demo" / "adapter.js", destination / "demo.js")
    shutil.copyfile(root / "demo" / "demo.css", destination / "demo.css")
    (destination / "demo-config.js").write_text(
        "window.AllFilesDemoConfig = Object.freeze(" + json.dumps({"version": version}) + ");\n", "utf-8"
    )
    html = (root / "web" / "index.html").read_text("utf-8")
    html = re.sub(r'(src|href)="/([^"/]+)"', r'\1="./\2"', html)
    html = html.replace("<title>AllFiles — Sve na jednom mestu</title>", "<title>AllFiles — Interactive demo</title>")
    html = html.replace('<meta name="theme-color"', '<meta name="description" content="Explore the AllFiles dashboard with fictional files on three drives, three Blu-ray discs and one LTO tape.">\n  <meta http-equiv="Content-Security-Policy" content="default-src \'self\'; script-src \'self\'; style-src \'self\'; img-src \'self\' data:; connect-src \'none\'; object-src \'none\'; base-uri \'self\'; form-action \'none\'">\n  <meta name="theme-color"')
    html = html.replace('<script src="./app.js" defer></script>', '<link rel="stylesheet" href="./demo.css">\n  <script src="./demo-config.js" defer></script>\n  <script src="./demo.js" defer></script>\n  <script src="./app.js" defer></script>')
    marker = '<div class="connection-error" id="connection-error" role="alert" hidden></div>'
    if marker not in html:
        raise RuntimeError("Dashboard layout changed: demo banner insertion point not found")
    html = html.replace(marker, marker + '\n      <aside class="demo-banner" aria-label="Demo"><div><strong id="demo-heading">DEMO · Fictional library</strong><p id="demo-description">Search files, open details and explore your media.</p></div><a id="demo-download" href="https://github.com/vladimirperovic/allfiles/releases/latest" target="_blank" rel="noopener noreferrer">Download AllFiles ↗</a></aside>')
    html = html.replace("</body>", '<dialog id="demo-dialog" class="modal"><header class="modal-header"><div><p class="eyebrow">ALLFILES DEMO</p><h2 id="demo-dialog-title">Explore the library</h2></div><button type="button" class="icon-button" id="demo-dialog-close" aria-label="Close">×</button></header><p class="modal-intro" id="demo-dialog-description"></p><footer class="modal-footer"><button type="button" class="button button-coral" id="demo-dialog-continue">Continue exploring</button></footer></dialog>\n</body>')
    (destination / "index.html").write_text(html, "utf-8")
    (destination / ".nojekyll").write_text("", "utf-8")
    manifest = {"version": version, "synthetic_data_only": True, "production_assets": {name: hashlib.sha256((root / "web" / name).read_bytes()).hexdigest() for name in assets}}
    (destination / "demo-build.json").write_text(json.dumps(manifest, indent=2) + "\n", "utf-8")
    print(f"AllFiles {version} demo built at {destination}")


if __name__ == "__main__":
    main()
