"""Require all five native packages and compute the public archive checksums."""
import argparse
import hashlib
from pathlib import Path

root = Path(__file__).resolve().parents[1]
version = (root / "VERSION").read_text(encoding="utf-8").strip()
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--directory", type=Path, required=True)
args = parser.parse_args()
expected = {f"allfiles-{version}-{platform}{suffix}" for platform, suffix in (
    ("windows-amd64", ".zip"), ("linux-amd64", ".tar.gz"), ("linux-arm64", ".tar.gz"),
    ("darwin-amd64", ".tar.gz"), ("darwin-arm64", ".tar.gz"))}
actual = {p.name for p in args.directory.iterdir() if p.suffix == ".zip" or p.name.endswith(".tar.gz")}
assert actual == expected, f"Release packages differ: missing={expected - actual}, unexpected={actual - expected}"
lines = []
for name in sorted(expected):
    path = args.directory / name
    assert path.is_file() and not path.is_symlink() and path.stat().st_size > 0
    lines.append(f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {name}\n")
(args.directory / "SHA256SUMS.txt").write_text("".join(lines), encoding="utf-8")
print(f"All {len(expected)} native release archives present; SHA256SUMS.txt written")
