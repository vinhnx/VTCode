#!/usr/bin/env python3
"""Risk-first coverage for the vendored GPUI snapshot attribution gate.

The failure mode is silent drift in a hand-written notice, not a panic: every
case below uses asymmetric inputs (only one field changed at a time) with an
explicit finding oracle, plus a matching fixture that must pass so a checker
that always fails cannot hide behind the negative tests.
"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    "check_third_party_notices", ROOT / "scripts" / "check_third_party_notices.py"
)
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)

VENDORED_VERSION = "0.3.12"
VENDORED_REVISION = "d2a4fb6ded7c470be25d1541281da3882495b65b"


def manifest(version: str, revision: str) -> str:
    return (
        '[package]\nname = "gpui-pre-macos"\n'
        f'version = "{version}"\n'
        "[package.metadata.gpui-pre]\n"
        f'zed-rev = "{revision}"\n'
    )


def header(
    version: str, revision: str, source: str = "https://github.com/zed-industries/zed"
) -> str:
    return (
        "Zed GPUI macOS backend\n"
        f"  Source: {source}\n"
        f"  Snapshot: gpui-pre-macos {version}, zed@{revision}\n"
        "  License: Apache License, Version 2.0\n"
    )


class SyntheticCheckout:
    def __init__(
        self, manifest_text: str, header_text: str, notices_text: str | None = None
    ):
        self._tmp = tempfile.TemporaryDirectory(prefix="notices-gate-")
        self.root = Path(self._tmp.name)
        paths = {
            gate.MANIFEST_REL: manifest_text,
            gate.HEADER_REL: header_text,
            gate.NOTICES_REL: header_text if notices_text is None else notices_text,
        }
        for relative, text in paths.items():
            target = self.root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text, encoding="utf-8")

    def close(self):
        self._tmp.cleanup()


class CheckoutTests(unittest.TestCase):
    def test_checkout_notice_names_the_vendored_snapshot(self):
        # Direct regression: the real header said 0.3.11 / f816e1f... while the
        # vendored crate is 0.3.12 / d2a4fb6... . Fails before the fix.
        self.assertEqual(gate.check(ROOT), [])


class SyntheticTests(unittest.TestCase):
    def setUp(self):
        self.checkouts: list[SyntheticCheckout] = []

    def tearDown(self):
        for checkout in self.checkouts:
            checkout.close()

    def make(
        self, manifest_text: str, header_text: str, notices_text: str | None = None
    ):
        checkout = SyntheticCheckout(manifest_text, header_text, notices_text)
        self.checkouts.append(checkout)
        return checkout

    def test_matching_files_pass(self):
        checkout = self.make(
            manifest(VENDORED_VERSION, VENDORED_REVISION),
            header(VENDORED_VERSION, VENDORED_REVISION),
        )
        self.assertEqual(gate.check(checkout.root), [])

    def test_version_drift_is_reported_without_revision_finding(self):
        checkout = self.make(
            manifest(VENDORED_VERSION, VENDORED_REVISION),
            header("0.3.11", VENDORED_REVISION),
        )
        findings = gate.check(checkout.root)
        self.assertTrue(any("version" in finding for finding in findings), findings)
        self.assertFalse(any("revision" in finding for finding in findings), findings)

    def test_revision_drift_is_reported_without_version_finding(self):
        checkout = self.make(
            manifest(VENDORED_VERSION, VENDORED_REVISION),
            header(VENDORED_VERSION, "f816e1f9263e3a8ef63c4baaa02e1441ce1c6038"),
        )
        findings = gate.check(checkout.root)
        self.assertTrue(any("revision" in finding for finding in findings), findings)
        self.assertFalse(any("version" in finding for finding in findings), findings)

    def test_generated_notice_with_a_different_header_is_reported(self):
        # Snapshot line is correct in both files; only the surrounding manual
        # header differs, so only the prefix check can catch this.
        checkout = self.make(
            manifest(VENDORED_VERSION, VENDORED_REVISION),
            header(VENDORED_VERSION, VENDORED_REVISION),
            header(
                VENDORED_VERSION,
                VENDORED_REVISION,
                source="https://example.invalid/zed",
            ),
        )
        findings = gate.check(checkout.root)
        self.assertTrue(
            any("no longer starts" in finding for finding in findings), findings
        )
        self.assertFalse(any("version" in finding for finding in findings), findings)

    def test_missing_snapshot_line_is_reported(self):
        checkout = self.make(
            manifest(VENDORED_VERSION, VENDORED_REVISION),
            "Zed GPUI macOS backend\n  License: Apache License, Version 2.0\n",
        )
        findings = gate.check(checkout.root)
        self.assertTrue(any("exactly one" in finding for finding in findings), findings)


if __name__ == "__main__":
    unittest.main()
