"""Generate distribution notices from cargo metadata and an installed Rust toolchain.

Usage: python scripts/third-party-notices.py metadata.json /rust/toolchain [/llvm-mingw/LICENSE.TXT]
No network access. Cargo verifies the downloaded package checksums.
"""
import html.parser
import json
import pathlib
import sys

metadata = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8-sig"))
toolchain = pathlib.Path(sys.argv[2])
sections = ["ALLFILES — THIRD-PARTY NOTICES\n\nAllFiles uses Rust and native SQLite. AllFiles itself is licensed separately in LICENSE.\nThis conservative inventory includes all Cargo.lock packages, including build/test and platform-specific dependencies that may not be linked in a particular binary.\nSource license declarations and supplied notice texts follow.\n"]
shared_texts = {}
def shared(text):
    text="\n".join(line.rstrip() for line in text.strip().replace("\r\n", "\n").splitlines())
    if text not in shared_texts:
        shared_texts[text]=len(shared_texts)+1
    return f"[Shared notice text {shared_texts[text]} — reproduced below]"
def section(label, text):
    sections.append("\n" + "=" * 78 + "\n" + label + "\n" + "=" * 78 + "\n\n" + shared(text) + "\n")

class PlainHTML(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.text = []
        self.pre = None
    def handle_starttag(self, tag, attrs):
        if tag == "pre":
            self.pre=[]
    def handle_endtag(self, tag):
        if tag == "pre" and self.pre is not None:
            self.text.append(shared("".join(self.pre))+"\n")
            self.pre=None
    def handle_data(self, data):
        if self.pre is not None:
            self.pre.append(data)
        elif data.strip():
            self.text.append(data.strip()+"\n")

copyright_html = toolchain / "share/doc/rust/COPYRIGHT-library.html"
if copyright_html.exists():
    parser = PlainHTML()
    parser.feed(copyright_html.read_text(encoding="utf-8"))
    section("Rust 1.99.1 standard-library copyright inventory", "".join(parser.text))
for path in sorted((toolchain / "share/doc/rust/licenses").glob("*.txt")):
    section("Rust toolchain license text: " + path.name, path.read_text(encoding="utf-8"))
standard_mit = (toolchain / "share/doc/rust/licenses/MIT.txt").read_text(encoding="utf-8")
for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
    if package["source"] is None:
        continue
    root = pathlib.Path(package["manifest_path"]).parent
    label = f'{package["name"]} {package["version"]} — {package["license"]}\n{package["repository"] or package["source"]}'
    notice_files = []
    for path in root.rglob("*"):
        relative = path.relative_to(root)
        if not path.is_file() or len(relative.parts) > 3:
            continue
        if path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")) or any(part.lower() in ["licenses", "licences"] for part in relative.parts[:-1]):
            notice_files.append(path)
    if notice_files:
        for path in sorted(notice_files):
            section(label + "\nSource: " + str(path.relative_to(root)), path.read_text(encoding="utf-8", errors="replace"))
    else:
        # Some upstream crates declare a license but ship no LICENSE file.
        # Preserve their actual declaration/authors rather than inventing a copyright line.
        declaration = "Upstream Cargo.toml license declaration: " + str(package["license"]) + "\nAuthors: " + ", ".join(package["authors"])
        if "MIT" not in (package["license"] or ""):
            raise RuntimeError("Missing notice text for " + label)
        section(label, declaration + "\nNo separate license file is included in this crate. Standard MIT terms:\n\n" + standard_mit)
section("SQLite 3.54.1 — public-domain dedication (sqlite3.c header)", "All of the code and documentation in SQLite has been dedicated\nto the public domain by the authors.\nhttps://www.sqlite.org/copyright.html")
if len(sys.argv) > 3:
    section("llvm-mingw 20261020 — native Windows runtime/build distribution notices", pathlib.Path(sys.argv[3]).read_text(encoding="utf-8"))
for text,number in sorted(shared_texts.items(), key=lambda item:item[1]):
    sections.append(f"\n{'='*78}\nShared notice text {number}\n{'='*78}\n\n{text}\n")
pathlib.Path("THIRD_PARTY_NOTICES.txt").write_text("\n".join(sections), encoding="utf-8", newline="\n")
print("Wrote notices for", len(metadata["packages"])-1, "Cargo packages")
