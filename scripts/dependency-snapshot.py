"""Create a GitHub dependency snapshot from committed Cargo lockfiles.

No network requests or credentials are used. Every tracked Cargo.toml must have
its own Cargo.lock, as in this repository's application and isolated experiment.
The complete lock graph is submitted for both files because GitHub deduplicates
inventories by manifest path. Optional and other-target dependencies remain in
the inventory; runtime/development scope cannot be inferred from a lockfile.

Schema: https://docs.github.com/en/rest/dependency-graph/dependency-submission
"""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tomllib
from urllib.parse import quote, urlsplit

CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
NAME = r"[A-Za-z0-9_-]+"
VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?"
DEPENDENCY = re.compile(rf"({NAME})(?: ({VERSION}))?(?: \(([^)]+)\))?")
DETECTOR = "allfiles-cargo-lock"
CORRELATOR = "cargo-lockfiles"


def git(repo, *arguments):
    result = subprocess.run(
        ["git", "-C", str(repo), *arguments], capture_output=True, check=False
    )
    if result.returncode:
        raise ValueError("Git could not read the requested committed repository state")
    return result.stdout


def package_key(package):
    name, version = package.get("name"), package.get("version")
    if not isinstance(name, str) or not re.fullmatch(NAME, name):
        raise ValueError("Invalid Cargo package name")
    if not isinstance(version, str) or not re.fullmatch(VERSION, version):
        raise ValueError(f"Invalid Cargo version for {name}")
    return name, version, package.get("source")


def purl(key):
    return f"pkg:cargo/{quote(key[0], safe='')}@{quote(key[1], safe='')}"


def resolve_graph(manifest_text, lock_text):
    manifest, lock = tomllib.loads(manifest_text), tomllib.loads(lock_text)
    if lock.get("version") not in (3, 4):
        raise ValueError("Only Cargo lockfile versions 3 and 4 are supported")
    root_manifest = manifest.get("package")
    if not isinstance(root_manifest, dict):
        raise ValueError("Virtual or inherited workspaces need an explicit resolver")
    root = package_key(root_manifest)
    if root[2] is not None:
        raise ValueError("The manifest root cannot have an external source")
    packages = lock.get("package")
    if not isinstance(packages, list) or not packages:
        raise ValueError("Cargo lockfile has no packages")
    indexed = {}
    for package in packages:
        key = package_key(package)
        if key in indexed:
            raise ValueError(f"Duplicate locked package: {key[0]} {key[1]}")
        if key != root and key[2] != CRATES_IO:
            raise ValueError(f"Unsupported dependency source: {key[0]} {key[1]}")
        indexed[key] = package
    if root not in indexed:
        raise ValueError("Manifest package/version does not match the lockfile root")

    def dependencies(key):
        values = indexed[key].get("dependencies", [])
        if not isinstance(values, list):
            raise ValueError(f"Invalid dependency list for {key[0]}")
        children = set()
        for value in values:
            match = DEPENDENCY.fullmatch(value) if isinstance(value, str) else None
            if not match:
                raise ValueError(f"Invalid locked dependency identifier in {key[0]}")
            name, version, source = match.groups()
            candidates = [candidate for candidate in indexed
                          if candidate[0] == name
                          and (version is None or candidate[1] == version)
                          and (source is None or candidate[2] == source)]
            if len(candidates) != 1:
                raise ValueError(f"Missing or ambiguous locked dependency: {value}")
            child = candidates[0]
            if child == root or child == key:
                raise ValueError("A dependency cannot point at the application or itself")
            children.add(child)
        return children

    direct = dependencies(root)
    resolved = {}
    for key in sorted(indexed, key=lambda item: (item[0], item[1], item[2] or "")):
        if key == root:
            continue
        identifier = purl(key)
        if identifier in resolved:
            raise ValueError(f"Package URL collision: {identifier}")
        resolved[identifier] = {
            "package_url": identifier,
            "relationship": "direct" if key in direct else "indirect",
            "dependencies": sorted(purl(child) for child in dependencies(key)),
        }
    if any(child not in resolved for item in resolved.values()
           for child in item["dependencies"]):
        raise ValueError("Incomplete dependency graph")
    return resolved


def snapshot(repo, revision="HEAD", ref="refs/heads/main", job_id=None, job_url=None):
    if ref != "refs/heads/main":
        raise ValueError("Only the default refs/heads/main inventory may be submitted")
    if revision != "HEAD" and not re.fullmatch(r"[0-9a-fA-F]{40}|[0-9a-fA-F]{64}", revision):
        raise ValueError("--sha must be HEAD or a full commit SHA")
    sha = git(repo, "rev-parse", "--verify", "--end-of-options", revision + "^{commit}").decode().strip()
    if not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", sha):
        raise ValueError("Git returned an invalid commit SHA")
    files = {}
    for record in git(repo, "ls-tree", "-r", "-z", "--full-tree", sha).split(b"\0"):
        if not record:
            continue
        metadata, raw_path = record.split(b"\t", 1)
        path = raw_path.decode("utf-8")
        if PurePosixPath(path).name in ("Cargo.lock", "Cargo.toml"):
            mode, kind, _ = metadata.split()
            if mode not in (b"100644", b"100755") or kind != b"blob":
                raise ValueError("Cargo manifests and lockfiles must be regular tracked files")
            files[path] = git(repo, "show", f"{sha}:{path}").decode("utf-8")
    locks = {path for path in files if path.endswith("Cargo.lock")}
    manifests = {path for path in files if path.endswith("Cargo.toml")}
    expected = {str(PurePosixPath(path).with_name("Cargo.toml")) for path in locks}
    if not locks or manifests != expected:
        raise ValueError("Every tracked Cargo.toml/Cargo.lock must form an explicit pair")

    inventory = {}
    for lock_path in sorted(locks):
        manifest_path = str(PurePosixPath(lock_path).with_name("Cargo.toml"))
        graph = resolve_graph(files[manifest_path], files[lock_path])
        for path in (lock_path, manifest_path):
            inventory[path] = {
                "name": path,
                "file": {"source_location": path},
                "metadata": {"lockfile": lock_path, "all_locked_targets": True},
                "resolved": graph,
            }
    job = {"id": job_id or f"local:{sha}", "correlator": CORRELATOR}
    if job_url:
        parsed = urlsplit(job_url)
        if parsed.scheme != "https" or not parsed.netloc or parsed.username or parsed.password:
            raise ValueError("Job URL must be HTTPS without credentials")
        job["html_url"] = job_url
    return {
        "version": 0,
        "sha": sha,
        "ref": ref,
        "job": job,
        "detector": {
            "name": DETECTOR,
            "version": "1.0.0",
            "url": "https://github.com/vladimirperovic/allfiles/blob/main/scripts/dependency-snapshot.py",
        },
        "scanned": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "manifests": inventory,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sha", default="HEAD")
    parser.add_argument("--ref", default="refs/heads/main")
    parser.add_argument("--job-id")
    parser.add_argument("--job-url")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    job_id, job_url = args.job_id, args.job_url
    if os.environ.get("GITHUB_RUN_ID"):
        run = os.environ["GITHUB_RUN_ID"]
        job_id = job_id or ":".join((run, os.environ.get("GITHUB_RUN_ATTEMPT", "1"), os.environ.get("GITHUB_JOB", "inventory")))
        if os.environ.get("GITHUB_REPOSITORY"):
            job_url = job_url or f"{os.environ.get('GITHUB_SERVER_URL', 'https://github.com')}/{os.environ['GITHUB_REPOSITORY']}/actions/runs/{run}"
    try:
        result = snapshot(repo, args.sha, args.ref, job_id, job_url)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8", newline="\n")
    except (ValueError, OSError, tomllib.TOMLDecodeError) as error:
        parser.exit(1, f"Dependency snapshot failed: {error}\n")
    print(f"Snapshot {result['sha']}: {len(result['manifests'])} committed manifests -> {args.output}")


if __name__ == "__main__":
    main()
