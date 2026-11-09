#!/usr/bin/env python3
"""Fixture-only Homebrew cask checks; release publication is not exercised."""

import os
import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKSUM = "a" * 64


class HomebrewCaskTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.workspace = Path(self.temp.name)
        self.cask = self.workspace / "homebrew" / "vtcode-desktop.rb"
        self.cask.parent.mkdir()
        self.original = (ROOT / "homebrew" / "vtcode-desktop.rb").read_text()
        self.cask.write_text(self.original)

    def transform(self, checksum=CHECKSUM, version="1.2.3"):
        return subprocess.run(
            [
                "bash",
                "-c",
                'source "$1"; shift; update_homebrew_cask_file "$@"',
                "fixture",
                str(ROOT / "scripts" / "homebrew-formula.sh"),
                str(self.cask),
                version,
                checksum,
            ],
            cwd=self.workspace,
            capture_output=True,
            text=True,
        )

    def assert_updated(self):
        text = self.cask.read_text()
        self.assertIn('version "1.2.3"', text)
        self.assertRegex(text, r'(?m)^\s*sha256 "' + CHECKSUM + '"$')
        normalize = lambda value: re.sub(
            r'sha256 "[^"]+"|version "[^"]+"', "VALUE", value
        )
        self.assertEqual(normalize(text), normalize(self.original))

    def test_updates_version_and_checksum_and_is_idempotent(self):
        result = self.transform()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_updated()
        first = self.cask.read_bytes()
        self.assertEqual(self.transform().returncode, 0)
        self.assertEqual(self.cask.read_bytes(), first)

    def test_rejects_invalid_checksum_before_writing(self):
        for invalid in ("", "f" * 63, "g" * 64, "a" * 64 + " archive.zip"):
            with self.subTest(invalid=invalid):
                self.assertNotEqual(self.transform(checksum=invalid).returncode, 0)
                self.assertEqual(self.cask.read_text(), self.original)

    def test_rejects_missing_or_duplicate_entries_before_writing(self):
        for seed in (
            self.original.replace('sha256 "', 'checksum "', 1),
            self.original + '\nversion "9.9.9"\n',
            self.original + '\nsha256 "' + "b" * 64 + '"\n',
        ):
            with self.subTest(seed=seed):
                self.cask.write_text(seed)
                self.assertNotEqual(self.transform().returncode, 0)
                self.assertEqual(self.cask.read_text(), seed)

    def test_rejects_version_injection_before_writing(self):
        self.assertNotEqual(
            self.transform(version='1.2.3"; system("bad")').returncode, 0
        )
        self.assertEqual(self.cask.read_text(), self.original)

    @unittest.skipUnless(shutil.which("ruby"), "ruby is not installed")
    def test_template_is_valid_ruby(self):
        result = subprocess.run(
            ["ruby", "-c", str(self.cask)],
            capture_output=True,
            text=True,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_release_updates_cask_from_the_desktop_sidecar(self):
        """The release helper must read the desktop .sha256 and write the cask."""
        formula = self.workspace / "homebrew" / "vtcode.rb"
        formula.write_text((ROOT / "homebrew" / "vtcode.rb").read_text())
        dist = self.workspace / "dist"
        dist.mkdir()
        checksums = tuple(character * 64 for character in "abcd")
        targets = (
            "x86_64-apple-darwin",
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
        )
        for target, checksum in zip(targets, checksums):
            (dist / f"vtcode-1.2.3-{target}.sha256").write_text(
                f"{checksum}  archive\n"
            )
        desktop_checksum = "e" * 64
        (dist / "vtcode-desktop-1.2.3-aarch64-apple-darwin.zip.sha256").write_text(
            f"{desktop_checksum}  archive.zip\n"
        )

        text = (ROOT / "scripts" / "homebrew-tap.sh").read_text()
        start = text.index("publish_homebrew_tap() {")
        end = text.index("\n}\n", start) + 3
        function = text[start:end]
        preamble = """
set -euo pipefail
source "$1"
print_info() { :; }
print_error() { :; }
print_success() { :; }
print_warning() { :; }
forbidden() { printf 'publication attempted\\n' >> forbidden; return 99; }
git() {
    case "$1" in
        diff) return 1 ;;
        add|commit|push) return 0 ;;
        *) forbidden ;;
    esac
}
gh() { forbidden; }
curl() { forbidden; }
cargo() { forbidden; }
"""
        result = subprocess.run(
            [
                "bash",
                "-c",
                preamble + function + '\npublish_homebrew_tap "1.2.3" || true',
                "fixture",
                str(ROOT / "scripts" / "homebrew-formula.sh"),
            ],
            cwd=self.workspace,
            env={**os.environ, "HOMEBREW_TAP_TOKEN": "", "GITHUB_TOKEN": ""},
            capture_output=True,
            text=True,
        )
        # The missing tap token stops publication after the local update.
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('version "1.2.3"', self.cask.read_text())
        self.assertIn(f'sha256 "{desktop_checksum}"', self.cask.read_text())
        self.assertIn("Casks/vtcode-desktop.rb", function)
        self.assertFalse((self.workspace / "forbidden").exists())


if __name__ == "__main__":
    unittest.main()
