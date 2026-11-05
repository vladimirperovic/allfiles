"""Check the release version without fetching dependencies or printing secrets."""
import os
from pathlib import Path
import re
import tomllib

root = Path(__file__).resolve().parents[1]
version = (root / "VERSION").read_text(encoding="utf-8").strip()
assert re.fullmatch(r"\d+\.\d+\.\d+", version), "VERSION must be a numeric semantic version"
package = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]
lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
assert package["version"] == version
assert next(p["version"] for p in lock["package"] if p["name"] == "allfiles") == version
if os.environ.get("RELEASE_TAG"):
    assert os.environ["RELEASE_TAG"] == f"v{version}", "Release tag must match VERSION"
print(f"AllFiles {version}: Cargo manifest, lockfile and release tag agree")
