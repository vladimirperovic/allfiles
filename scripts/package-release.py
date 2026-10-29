"""Package one tested native executable using an explicit public-file allowlist."""
import argparse
import hashlib
import io
from pathlib import Path
import re
import tarfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ("windows-amd64", "linux-amd64", "linux-arm64", "darwin-amd64", "darwin-arm64")


def package(binary: Path, platform: str, output: Path) -> Path:
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("Invalid release version")
    if binary.is_symlink() or not binary.is_file():
        raise ValueError("Expected a regular native executable")
    executable = f"allfiles-{platform}" + (".exe" if platform.startswith("windows-") else "")
    files = {executable: (binary.read_bytes(), 0o755)}
    for name in ("README.md", "README.en.md", "LICENSE", "CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md", "THIRD_PARTY_NOTICES.txt", "VERSION"):
        path = ROOT / name
        if path.is_symlink() or not path.is_file():
            raise ValueError(f"Missing or linked public document: {name}")
        files[name] = (path.read_bytes(), 0o644)
    for path in sorted((ROOT / "docs").rglob("*.md")):
        if path.is_symlink() or not path.is_file():
            raise ValueError("Documentation must consist of regular files")
        files[path.relative_to(ROOT).as_posix()] = (path.read_bytes(), 0o644)
    sums = "".join(f"{hashlib.sha256(data).hexdigest()}  {name}\n" for name, (data, _) in sorted(files.items()))
    files["SHA256SUMS"] = (sums.encode("utf-8"), 0o644)
    output.mkdir(parents=True, exist_ok=True)
    suffix = ".zip" if platform.startswith("windows-") else ".tar.gz"
    archive = output / f"allfiles-{version}-{platform}{suffix}"
    if suffix == ".zip":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as target:
            for name, (data, mode) in sorted(files.items()):
                entry = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
                entry.create_system = 3
                entry.external_attr = (0o100000 | mode) << 16
                target.writestr(entry, data, compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
        with zipfile.ZipFile(archive) as check:
            assert check.testzip() is None
            assert set(check.namelist()) == set(files)
            assert all(check.read(name) == data for name, (data, _) in files.items())
    else:
        with tarfile.open(archive, "w:gz", compresslevel=9) as target:
            for name, (data, mode) in sorted(files.items()):
                entry = tarfile.TarInfo(name)
                entry.size, entry.mode, entry.mtime = len(data), mode, 0
                target.addfile(entry, io.BytesIO(data))
        with tarfile.open(archive) as check:
            assert set(check.getnames()) == set(files)
            assert all(check.extractfile(name).read() == data for name, (data, _) in files.items())
            assert check.getmember(executable).mode == 0o755
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    print(f"{digest}  {archive.name}")
    return archive


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--platform", required=True, choices=PLATFORMS)
    parser.add_argument("--output", default=Path("dist"), type=Path)
    args = parser.parse_args()
    package(args.binary, args.platform, args.output)
