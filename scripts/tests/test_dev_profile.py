#!/usr/bin/env python3
"""Risk-first coverage for the dev-profile GPUI override gate.

The failure mode is silent drift after a Kit upgrade, not a panic: an
override can be dropped or pointed at a level that no longer exists, or a
crate can leave Cargo.lock while its override stays behind. Each synthetic
case below changes only one field, with an explicit finding oracle, and a
matching fixture must pass so an always-fail checker cannot hide in the
negative tests.
"""

import importlib.util
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location(
    "check_dev_profile", ROOT / "scripts" / "check_dev_profile.py"
)
gate = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(gate)


def manifest_text(levels: dict[str, int]) -> str:
    lines = "".join(
        f"{name} = {{ opt-level = {level} }}\n" for name, level in levels.items()
    )
    return (
        "[profile.dev]\n"
        "debug = 0\n\n"
        "[profile.dev.package]\n"
        + lines
        + '\n[profile.dev.package."*"]\nopt-level = 1\n'
    )


def lock_text(names: list[str]) -> str:
    return "version = 4\n\n" + "".join(
        f'[[package]]\nname = "{name}"\nversion = "1.0.0"\n\n' for name in names
    )


def matching_levels() -> dict[str, int]:
    return {name: gate.EXPECTED_OPT_LEVEL for name in gate.FRAMEWORK_PACKAGES}


class SyntheticCheckout:
    def __init__(self, manifest: str, lock: str):
        self._tmp = tempfile.TemporaryDirectory(prefix="dev-profile-gate-")
        self.root = Path(self._tmp.name)
        (self.root / "Cargo.toml").write_text(manifest, encoding="utf-8")
        (self.root / "Cargo.lock").write_text(lock, encoding="utf-8")

    def close(self):
        self._tmp.cleanup()


class CheckoutTests(unittest.TestCase):
    def test_checkout_keeps_the_pinned_framework_overrides(self):
        self.assertEqual(gate.check(ROOT), [])


class SyntheticTests(unittest.TestCase):
    def setUp(self):
        self.checkouts: list[SyntheticCheckout] = []

    def tearDown(self):
        for checkout in self.checkouts:
            checkout.close()

    def make(
        self, levels: dict[str, int] | None = None, names: list[str] | None = None
    ):
        checkout = SyntheticCheckout(
            manifest_text(matching_levels() if levels is None else levels),
            lock_text(list(gate.FRAMEWORK_PACKAGES) if names is None else names),
        )
        self.checkouts.append(checkout)
        return checkout

    def test_matching_profile_and_lock_pass(self):
        self.assertEqual(gate.check(self.make().root), [])

    def test_dropped_override_is_reported_for_that_package_only(self):
        levels = matching_levels()
        del levels["gpui-pre"]
        findings = gate.check(self.make(levels=levels).root)
        self.assertTrue(
            any("profile.dev.package.gpui-pre must" in finding for finding in findings),
            findings,
        )
        # gpui-pre-platform shares the prefix; only the dropped entry is named.
        self.assertFalse(
            any("gpui-pre-platform" in finding for finding in findings), findings
        )
        self.assertFalse(any("Cargo.lock" in finding for finding in findings), findings)

    def test_wrong_opt_level_is_reported_for_that_package_only(self):
        levels = matching_levels()
        levels["ttf-parser"] = 2
        findings = gate.check(self.make(levels=levels).root)
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("profile.dev.package.ttf-parser", findings[0])
        self.assertIn("opt-level = 3", findings[0])

    def test_stale_override_without_a_lock_entry_is_reported(self):
        names = [n for n in gate.FRAMEWORK_PACKAGES if n != "taffy"]
        findings = gate.check(self.make(names=names).root)
        self.assertEqual(len(findings), 1, findings)
        self.assertIn("Cargo.lock", findings[0])
        self.assertIn("taffy", findings[0])

    def test_extra_lock_package_does_not_create_findings(self):
        names = [*gate.FRAMEWORK_PACKAGES, "unrelated-dependency"]
        self.assertEqual(gate.check(self.make(names=names).root), [])

    def test_all_overrides_missing_reports_every_package(self):
        findings = gate.check(self.make(levels={}).root)
        self.assertEqual(len(findings), len(gate.FRAMEWORK_PACKAGES), findings)


if __name__ == "__main__":
    unittest.main()
