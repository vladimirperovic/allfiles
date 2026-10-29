"""Regression tests for complete, commit-pinned GitHub dependency inventory."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("dependency_snapshot", Path(__file__).with_name("dependency-snapshot.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
SOURCE = MODULE.CRATES_IO
MANIFEST = '[package]\nname = "app"\nversion = "0.1.0"\n'


def lock(packages):
    result = "version = 4\n"
    for name, version, source, dependencies in packages:
        result += f"\n[[package]]\nname = {json.dumps(name)}\nversion = {json.dumps(version)}\n"
        if source is not None:
            result += f"source = {json.dumps(source)}\n"
        result += "dependencies = " + json.dumps(dependencies) + "\n"
    return result


def fixture():
    return [
        ("app", "0.1.0", None, ["foo 2.0.0"]),
        ("foo", "1.0.0", SOURCE, []),
        ("foo", "2.0.0", SOURCE, ["foo 1.0.0", "bar"]),
        ("bar", "3.0.0+build.1", SOURCE, []),
        # Lockfiles can retain optional/other-target packages not reachable
        # from the currently selected build. They must not disappear.
        ("optional-target", "1.0.0", SOURCE, []),
    ]


class GraphTests(unittest.TestCase):
    def test_duplicate_versions_complete_edges_and_all_locked_targets(self):
        graph = MODULE.resolve_graph(MANIFEST, lock(fixture()))
        self.assertEqual(len(graph), 4)
        self.assertNotIn("pkg:cargo/app@0.1.0", graph)
        self.assertEqual(graph["pkg:cargo/foo@2.0.0"]["relationship"], "direct")
        self.assertEqual(graph["pkg:cargo/foo@1.0.0"]["relationship"], "indirect")
        self.assertEqual(graph["pkg:cargo/foo@2.0.0"]["dependencies"], ["pkg:cargo/bar@3.0.0%2Bbuild.1", "pkg:cargo/foo@1.0.0"])
        self.assertIn("pkg:cargo/optional-target@1.0.0", graph)
        self.assertTrue(all("scope" not in package for package in graph.values()))

    def test_source_qualified_lock_reference(self):
        packages = fixture()
        packages[0] = ("app", "0.1.0", None, [f"foo 2.0.0 ({SOURCE})"])
        self.assertEqual(MODULE.resolve_graph(MANIFEST, lock(packages))["pkg:cargo/foo@2.0.0"]["relationship"], "direct")

    def test_ambiguous_and_missing_references_are_errors(self):
        for name in ("foo", "missing", "foo 9.0.0"):
            with self.subTest(name=name):
                packages = fixture()
                packages[0] = ("app", "0.1.0", None, [name])
                with self.assertRaisesRegex(ValueError, "Missing or ambiguous"):
                    MODULE.resolve_graph(MANIFEST, lock(packages))

    def test_unhandled_sources_are_errors_even_when_not_reachable(self):
        for source in (None, "git+https://example.invalid/repository#123", "registry+https://example.invalid/index"):
            with self.subTest(source=source):
                with self.assertRaisesRegex(ValueError, "Unsupported dependency source"):
                    MODULE.resolve_graph(MANIFEST, lock(fixture() + [("unhandled", "1.0.0", source, [])]))

    def test_wrong_root_duplicate_package_and_root_dependency_are_errors(self):
        with self.assertRaisesRegex(ValueError, "does not match"):
            MODULE.resolve_graph(MANIFEST.replace('"0.1.0"', '"0.2.0"'), lock(fixture()[1:]))
        with self.assertRaisesRegex(ValueError, "Duplicate locked package"):
            MODULE.resolve_graph(MANIFEST, lock(fixture() + [fixture()[-1]]))
        packages = fixture()
        packages[3] = ("bar", "3.0.0+build.1", SOURCE, ["app"])
        with self.assertRaisesRegex(ValueError, "application or itself"):
            MODULE.resolve_graph(MANIFEST, lock(packages))


class CommittedSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="allfiles-dependency-snapshot-")
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.run_git("init", "--quiet")
        self.run_git("config", "user.name", "Synthetic Test")
        self.run_git("config", "user.email", "test@example.invalid")
        self.run_git("config", "commit.gpgsign", "false")
        self.run_git("config", "core.hooksPath", str(self.repo / "no-hooks"))
        self.write_pair("", MANIFEST, lock(fixture()))
        self.write_pair("experiments/rust-search", MANIFEST.replace('"app"', '"experiment"'), lock([
            ("experiment", "0.1.0", None, ["bar"]), ("bar", "3.0.0", SOURCE, [])
        ]))
        self.commit()
        self.first = self.run_git("rev-parse", "HEAD").strip()

    def run_git(self, *arguments):
        return subprocess.run(["git", "-C", str(self.repo), *arguments], check=True, capture_output=True, text=True).stdout

    def write_pair(self, subdirectory, manifest, locked):
        directory = self.repo / subdirectory
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "Cargo.toml").write_text(manifest, encoding="utf-8")
        (directory / "Cargo.lock").write_text(locked, encoding="utf-8")

    def commit(self):
        self.run_git("add", ".")
        self.run_git("commit", "--quiet", "-m", "Synthetic fixture")

    def test_all_tracked_pairs_and_github_schema(self):
        result = MODULE.snapshot(self.repo, job_id="123:1:inventory", job_url="https://github.com/example/repo/actions/runs/123")
        self.assertEqual(set(result["manifests"]), {"Cargo.toml", "Cargo.lock", "experiments/rust-search/Cargo.toml", "experiments/rust-search/Cargo.lock"})
        self.assertEqual(result["sha"], self.first)
        self.assertEqual(result["ref"], "refs/heads/main")
        self.assertEqual(result["job"]["correlator"], "cargo-lockfiles")
        self.assertEqual(result["detector"]["name"], "allfiles-cargo-lock")
        self.assertEqual(result["version"], 0)
        self.assertRegex(result["scanned"], r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$")
        for path, manifest in result["manifests"].items():
            self.assertEqual(manifest["file"]["source_location"], path)
            self.assertEqual(manifest["name"], path)
            for key, value in manifest["resolved"].items():
                self.assertEqual(value["package_url"], key)
                self.assertTrue(set(value["dependencies"]) <= set(manifest["resolved"]))

    def test_dirty_untracked_and_newer_commit_never_contaminate_pinned_snapshot(self):
        original = MODULE.snapshot(self.repo, self.first)["manifests"]
        self.write_pair("", "invalid dirty manifest", "invalid dirty lock")
        self.write_pair("untracked", MANIFEST, lock(fixture()))
        self.assertEqual(MODULE.snapshot(self.repo, self.first)["manifests"], original)
        self.commit()
        self.assertEqual(MODULE.snapshot(self.repo, self.first)["manifests"], original)

    def test_missing_pair_fails(self):
        (self.repo / "experiments/rust-search/Cargo.lock").unlink()
        self.commit()
        with self.assertRaisesRegex(ValueError, "explicit pair"):
            MODULE.snapshot(self.repo)

    def test_branch_and_revision_validation(self):
        for ref in ("refs/heads/feature", "refs/pull/1/merge", "main"):
            with self.assertRaisesRegex(ValueError, "default"):
                MODULE.snapshot(self.repo, ref=ref)
        for revision in ("--all", "HEAD~1", "a\nb", "no-such-ref"):
            with self.assertRaisesRegex(ValueError, "full commit SHA"):
                MODULE.snapshot(self.repo, revision)
        with self.assertRaisesRegex(ValueError, "credentials"):
            MODULE.snapshot(self.repo, job_url="https://user:secret@example.invalid/run")


if __name__ == "__main__":
    unittest.main()
