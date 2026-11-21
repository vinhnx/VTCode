#!/usr/bin/env python3
"""Regression checks for the diff-reachable test selection closure."""

import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"

_spec = importlib.util.spec_from_file_location("select_tests", SCRIPTS / "select_tests.py")
select_tests = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(select_tests)


def make_package(name: str, root: str) -> dict:
    return {
        "name": name,
        "manifest_path": str(Path(root) / name / "Cargo.toml"),
        "dependencies": [],
    }


class SyntheticWorkspace:
    """A tiny synthetic package graph rooted in a temp dir."""

    def __init__(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.root = self._tmp.name
        self.packages = []

    def add(self, name: str, dependencies: list[str]) -> None:
        package = make_package(name, self.root)
        package["dependencies"] = [{"name": dep, "kind": None} for dep in dependencies]
        self.packages.append(package)

    def add_root_package(self, name: str, dependencies: list[str]) -> None:
        """A package whose manifest sits at the workspace root (like the real repo)."""
        package = {
            "name": name,
            "manifest_path": str(Path(self.root) / "Cargo.toml"),
            "dependencies": [{"name": dep, "kind": None} for dep in dependencies],
        }
        self.packages.append(package)

    def close(self) -> None:
        self._tmp.cleanup()


class ReverseGraphTests(unittest.TestCase):
    def test_dev_and_build_edges_count_as_dependents(self):
        packages = [make_package("core", "/r")]
        for name, kinds in (("bin", ["normal"]), ("testsuite", ["dev"]), ("macros-user", ["build"])):
            package = make_package(name, "/r")
            package["dependencies"] = [{"name": "core", "kind": kind} for kind in kinds]
            packages.append(package)

        graph = select_tests.build_reverse_graph(packages)
        dependents = select_tests.reachable_from({"core"}, graph)

        # Every edge kind must pull its dependent into the closure: dev edges
        # drive integration tests, build edges drive proc-macro rebuilds.
        self.assertEqual(dependents, {"core", "bin", "testsuite", "macros-user"})

    def test_closure_is_transitive_and_acyclic_safe(self):
        packages = [
            make_package("leaf", "/r"),
            make_package("middle", "/r"),
            make_package("top", "/r"),
        ]
        packages[1]["dependencies"] = [{"name": "leaf", "kind": None}]
        packages[2]["dependencies"] = [{"name": "middle", "kind": None}]

        graph = select_tests.build_reverse_graph(packages)
        self.assertEqual(select_tests.reachable_from({"leaf"}, graph), {"leaf", "middle", "top"})
        self.assertEqual(select_tests.reachable_from({"middle"}, graph), {"middle", "top"})
        self.assertEqual(select_tests.reachable_from(set(), graph), set())


class SelectForFilesTests(unittest.TestCase):
    def setUp(self):
        self.ws = SyntheticWorkspace()
        self.ws.add("vtcode-core", [])
        self.ws.add("vtcode-ui", ["vtcode-core"])
        self.ws.add("vtcode", ["vtcode-core", "vtcode-ui"])
        self.ws.add("docs-only", [])

    def tearDown(self):
        self.ws.close()

    def test_source_change_selects_changed_package_and_dependents(self):
        selected, reason = select_tests.select_for_files(
            ["vtcode-core/src/lib.rs"], self.ws.packages, self.ws.root
        )
        self.assertEqual(selected, {"vtcode-core", "vtcode-ui", "vtcode"})
        self.assertIsNone(reason)

    def test_leaf_change_selects_only_dependents(self):
        # vtcode-ui has exactly one dependent (the root binary): selecting
        # vtcode-core here would burn minutes on tests the diff cannot affect.
        selected, _ = select_tests.select_for_files(
            ["vtcode-ui/src/tui.rs"], self.ws.packages, self.ws.root
        )
        self.assertEqual(selected, {"vtcode-ui", "vtcode"})

    def test_manifest_change_reruns_everything_with_reason(self):
        selected, reason = select_tests.select_for_files(
            ["vtcode-core/Cargo.toml"], self.ws.packages, self.ws.root
        )
        self.assertEqual(selected, {p["name"] for p in self.ws.packages})
        self.assertIn("manifest changed", reason)

    def test_lockfile_and_toolchain_rerun_everything(self):
        for path in ("Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", ".config/nextest.toml"):
            selected, reason = select_tests.select_for_files([path], self.ws.packages, self.ws.root)
            self.assertEqual(selected, {p["name"] for p in self.ws.packages}, path)
            self.assertIsNotNone(reason, path)

    def test_workflow_change_reruns_everything(self):
        selected, reason = select_tests.select_for_files(
            [".github/workflows/ci.yml"], self.ws.packages, self.ws.root
        )
        self.assertEqual(selected, {p["name"] for p in self.ws.packages})
        self.assertIn("ci.yml", reason)

    def test_docs_and_scripts_select_nothing(self):
        selected, reason = select_tests.select_for_files(
            ["docs/development/ci-cd.md", "scripts/check-dev.sh"],
            self.ws.packages,
            self.ws.root,
        )
        self.assertEqual(selected, set())
        self.assertIsNone(reason)

    def test_root_package_loose_files_do_not_select(self):
        # Real workspaces: the workspace root is itself a package. Without the
        # source-dir boundary, a loose root file (README.md) would map to the
        # root package and docs-only PRs would run its heavy test binaries.
        root = tempfile.TemporaryDirectory()
        try:
            ws = SyntheticWorkspace()
            ws.root = root.name
            ws.add_root_package("rootpkg", [])
            selected, _ = select_tests.select_for_files(["README.md"], ws.packages, ws.root)
            self.assertEqual(selected, set())
            selected, _ = select_tests.select_for_files(["src/main.rs"], ws.packages, ws.root)
            self.assertEqual(selected, {"rootpkg"})
            selected, _ = select_tests.select_for_files([".github/workflows/x.yml"], ws.packages, ws.root)
            self.assertEqual(selected, set())
            selected, _ = select_tests.select_for_files(["build.rs"], ws.packages, ws.root)
            self.assertEqual(selected, {"rootpkg"})
        finally:
            root.cleanup()

    def test_nested_file_maps_to_nearest_manifest(self):
        selected, _ = select_tests.select_for_files(
            ["vtcode-ui/src/tui/pane.rs"], self.ws.packages, self.ws.root
        )
        self.assertEqual(selected, {"vtcode-ui", "vtcode"})


class RealWorkspaceSmokeTests(unittest.TestCase):
    """The synthetic graph above checks the algorithm; this checks the repo."""

    @classmethod
    def setUpClass(cls):
        metadata = json.loads(
            subprocess.check_output(
                ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
                cwd=ROOT,
                text=True,
            )
        )
        cls.packages = metadata["packages"]
        cls.root = str(ROOT)

    def test_core_change_reaches_root_binary(self):
        selected, _ = select_tests.select_for_files(
            ["crates/codegen/vtcode-core/src/lib.rs"], self.packages, self.root
        )
        self.assertIn("vtcode-core", selected)
        self.assertIn("vtcode", selected)

    def test_git_paths_are_repo_root_relative_regardless_of_cwd(self):
        # changed_files_for_base anchors git at repo_root, so invoking it from
        # a different working directory yields identical, root-relative paths.
        from_root = select_tests.changed_files_for_base("HEAD~1", self.root)
        previous = os.getcwd()
        try:
            os.chdir(ROOT / "docs")
            from_docs = select_tests.changed_files_for_base("HEAD~1", self.root)
        finally:
            os.chdir(previous)
        self.assertEqual(from_docs, from_root)
        self.assertTrue(from_root, "HEAD~1 diff must touch at least one file for this smoke test")

    def test_ui_change_reaches_desktop_consumers(self):
        selected, _ = select_tests.select_for_files(
            ["crates/codegen/vtcode-ui/src/lib.rs"], self.packages, self.root
        )
        self.assertIn("vtcode-ui", selected)
        # Whatever consumes vtcode-ui must retest; vtcode-core does not depend
        # on it, so its tests stay unselected for a UI-only diff.
        self.assertNotIn("vtcode-core", selected)
        self.assertTrue({"vtcode-desktop"} & selected, "vtcode-ui dependents missing from closure")

    def test_output_args_are_sorted_and_deduplicated(self):
        # Pins the exact nextest argument string CI interpolates unquoted:
        # sorted for stable logs, one `-p` per package, empty for no selection.
        self.assertEqual(
            select_tests.format_package_args({"vtcode-ui", "vtcode", "vtcode-core"}),
            "-p vtcode -p vtcode-core -p vtcode-ui",
        )
        self.assertEqual(select_tests.format_package_args(set()), "")


if __name__ == "__main__":
    unittest.main()
