"""Regression coverage for the vendored `block` patch (patches/block/).

Risk-first notes (agentic-testing): the bug-prone surface here is silent
regression, not behavior. Both failure modes below still compile and run while
reintroducing the `static_of_uninhabited_type` future-compat warning (deny by
default on current toolchains, softened to a warning only for registry
dependencies):

- the `[patch.crates-io]` wiring is dropped and the lock drifts back to the
  registry source (pinned via the resolver output, not our own manifests, so
  the oracle is independent of the wiring under test);
- the vendored sources are re-copied from upstream, losing the fix (pinned by
  compiling the vendored crate standalone, where the lint is a hard error).

Each test pins both sides of its boundary: the fixed state passes and the
reverted state fails. Wiring uses `cargo metadata --locked`, matching the
existing `test_publish_order.py` precedent; both tests are platform
independent (`block` only compiles on Apple targets, but the lint and the
resolve graph are checked everywhere).
"""

import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PATCH_MANIFEST = ROOT / "patches" / "block" / "Cargo.toml"


def run_cargo(*args: str) -> "subprocess.CompletedProcess[str]":
    return subprocess.run(
        ["cargo", *args],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )


class BlockPatchWiringTests(unittest.TestCase):
    def test_block_resolves_to_vendored_patch(self):
        proc = run_cargo("metadata", "--locked", "--format-version", "1")
        self.assertEqual(proc.returncode, 0, msg=proc.stderr[-2000:])
        blocks = [
            p for p in json.loads(proc.stdout)["packages"] if p["name"] == "block"
        ]
        self.assertGreater(len(blocks), 0, msg="`block` vanished from the resolve graph")
        for block in blocks:
            self.assertIsNone(
                block["source"],
                msg=f"block {block['version']} must come from the path patch, "
                f"not {block['source']}",
            )
            self.assertTrue(
                block["manifest_path"].endswith("patches/block/Cargo.toml"),
                msg=f"unexpected block manifest: {block['manifest_path']}",
            )


class VendoredBlockTests(unittest.TestCase):
    def test_vendored_block_checks_clean(self):
        # Hermetic: check a temp copy so cargo never writes Cargo.lock/target
        # residue under patches/block/ (`cargo check` has no --lockfile-path).
        with tempfile.TemporaryDirectory(prefix="block-patch-check") as tmp:
            mirror = Path(tmp) / "block"
            shutil.copytree(PATCH_MANIFEST.parent, mirror)
            proc = run_cargo(
                "check",
                "--manifest-path", str(mirror / "Cargo.toml"),
                "--offline",
            )
        self.assertEqual(proc.returncode, 0, msg=proc.stderr[-2000:])
        self.assertNotIn("warning", proc.stderr.lower())
        self.assertNotIn("error", proc.stderr.lower())


if __name__ == "__main__":
    unittest.main()
