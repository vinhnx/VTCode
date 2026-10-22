#!/usr/bin/env python3
"""Regression checks for the crates.io publish order derivation."""

import contextlib
import importlib.util
import io
import json
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
PUBLISH_SCRIPT = SCRIPTS / "publish_extracted_crates.sh"

_spec = importlib.util.spec_from_file_location(
    "publish_order", SCRIPTS / "publish_order.py"
)
publish_order = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(publish_order)


def load_workspace_metadata() -> dict:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        cwd=ROOT,
        capture_output=True,
        text=True,
        check=True,
    )
    return json.loads(result.stdout)


def declared_publish_crates() -> list[str]:
    """Read the PUBLISH_CRATES list straight from the release script."""
    text = PUBLISH_SCRIPT.read_text()
    match = re.search(r"^PUBLISH_CRATES=\(\n(.*?)^\)", text, re.MULTILINE | re.DOTALL)
    if match is None:
        raise AssertionError(
            "PUBLISH_CRATES list not found in publish_extracted_crates.sh"
        )
    return re.findall(r"^\s*(vtcode[\w-]*)\s*$", match.group(1), re.MULTILINE)


class PublishOrderTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.metadata = load_workspace_metadata()
        cls.packages = publish_order.workspace_packages(cls.metadata)

    def test_declared_list_matches_derived_order(self):
        """The publish list must already be a valid topological order.

        This is the regression that aborted the 0.180.4 release: the list was
        hand-maintained and `vtcode-commons` gained a dependency on
        `vtcode-exec-events` without the sequence being updated.
        """
        crates = declared_publish_crates()
        self.assertTrue(crates)
        dependencies = {
            crate: publish_order.package_dependencies(
                self.packages[crate], self.packages
            )
            for crate in crates
        }
        derived = publish_order.topological_order(crates, dependencies)
        self.assertEqual(crates, derived)

    def test_derived_order_places_every_dependency_first(self):
        crates = declared_publish_crates()
        dependencies = {
            crate: publish_order.package_dependencies(
                self.packages[crate], self.packages
            )
            for crate in crates
        }
        order = publish_order.topological_order(crates, dependencies)
        position = {crate: index for index, crate in enumerate(order)}
        for crate, deps in dependencies.items():
            for dep in deps:
                if dep in position:
                    self.assertLess(
                        position[dep],
                        position[crate],
                        f"{dep} must be published before {crate}",
                    )

    def test_every_published_crate_is_a_workspace_member(self):
        for crate in declared_publish_crates():
            self.assertIn(crate, self.packages)

    def test_plan_rejects_unknown_crate(self):
        with contextlib.redirect_stderr(io.StringIO()):
            code = publish_order.main(["plan", "--crates", "vtcode-not-a-crate"])
        self.assertNotEqual(code, 0)

    def test_plan_rejects_dependency_outside_publish_list(self):
        with contextlib.redirect_stderr(io.StringIO()):
            code = publish_order.main(["plan", "--crates", "vtcode-commons"])
        self.assertNotEqual(code, 0)

    def test_plan_orders_dependency_before_dependent(self):
        # The list is given in a deliberately wrong order; the plan must
        # reorder it so every dependency precedes its dependents.
        with tempfile.TemporaryDirectory() as temp_dir:
            metadata_path = Path(temp_dir) / "metadata.json"
            metadata_path.write_text(json.dumps(self.metadata))
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPTS / "publish_order.py"),
                    "plan",
                    "--metadata",
                    str(metadata_path),
                    "--crates",
                    "vtcode-commons",
                    "vtcode-exec-events",
                    "vtcode-diff",
                ],
                capture_output=True,
                text=True,
                check=True,
            )
        order = result.stdout.split()
        self.assertLess(
            order.index("vtcode-exec-events"), order.index("vtcode-commons")
        )
        self.assertLess(order.index("vtcode-diff"), order.index("vtcode-commons"))

    def test_dependencies_are_reported_per_crate(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            metadata_path = Path(temp_dir) / "metadata.json"
            metadata_path.write_text(json.dumps(self.metadata))
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPTS / "publish_order.py"),
                    "deps",
                    "--metadata",
                    str(metadata_path),
                    "--crates",
                    "vtcode-commons",
                ],
                capture_output=True,
                text=True,
                check=True,
            )
        pairs = [
            tuple(line.split("\t"))
            for line in result.stdout.strip().splitlines()
            if line
        ]
        self.assertIn(("vtcode-commons", "vtcode-exec-events"), pairs)
        self.assertIn(("vtcode-commons", "vtcode-diff"), pairs)

    def test_cycle_is_reported(self):
        metadata = {
            "packages": [
                {"name": "a", "dependencies": [{"name": "b", "kind": None}]},
                {"name": "b", "dependencies": [{"name": "a", "kind": None}]},
            ]
        }
        with tempfile.TemporaryDirectory() as temp_dir:
            metadata_path = Path(temp_dir) / "metadata.json"
            metadata_path.write_text(json.dumps(metadata))
            result = subprocess.run(
                [
                    sys.executable,
                    str(SCRIPTS / "publish_order.py"),
                    "plan",
                    "--metadata",
                    str(metadata_path),
                    "--crates",
                    "a",
                    "b",
                ],
                capture_output=True,
                text=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("cycle", result.stderr)


if __name__ == "__main__":
    unittest.main()
