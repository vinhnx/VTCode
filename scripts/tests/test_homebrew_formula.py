#!/usr/bin/env python3
"""Fixture-only formula checks; release publication commands are stubbed."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TARGETS = (
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
CHECKSUMS = tuple(character * 64 for character in "abcd")


class HomebrewFormulaTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.workspace = Path(self.temp.name)
        self.formula = self.workspace / "homebrew" / "vtcode.rb"
        self.formula.parent.mkdir()
        self.original = (ROOT / "homebrew" / "vtcode.rb").read_text()
        self.formula.write_text(self.original)

    def transform(self, checksums=CHECKSUMS, version="1.2.3"):
        return subprocess.run(
            ["bash", "-c", 'source "$1"; shift; update_homebrew_formula_file "$@"',
             "fixture", str(ROOT / "scripts" / "homebrew-formula.sh"),
             str(self.formula), version, *checksums],
            cwd=self.workspace, capture_output=True, text=True,
        )

    def assert_updated(self):
        text = self.formula.read_text()
        self.assertIn('version "1.2.3"', text)
        for target, checksum in zip(TARGETS, CHECKSUMS):
            self.assertRegex(text, re.escape(target) + r'\.tar\.gz"\s+sha256 "' + checksum + '"')
        # Unrelated Ruby code and formatting remain byte-for-byte intact.
        normalize = lambda value: re.sub(r'sha256 "[^"]+"|version "[^"]+"', "VALUE", value)
        self.assertEqual(normalize(text), normalize(self.original))

    def test_updates_all_four_targets_and_is_idempotent(self):
        result = self.transform()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assert_updated()
        first = self.formula.read_bytes()
        self.assertEqual(self.transform().returncode, 0)
        self.assertEqual(self.formula.read_bytes(), first)

    def test_rejects_each_incomplete_or_invalid_checksum_before_writing(self):
        for index in range(4):
            for invalid in ("", "f" * 63, "g" * 64, "a" * 64 + " filename.tar.gz"):
                with self.subTest(index=index, invalid=invalid):
                    checksums = list(CHECKSUMS)
                    checksums[index] = invalid
                    self.assertNotEqual(self.transform(checksums).returncode, 0)
                    self.assertEqual(self.formula.read_text(), self.original)
        self.assertNotEqual(self.transform(CHECKSUMS[:3]).returncode, 0)
        self.assertEqual(self.formula.read_text(), self.original)

    def test_rejects_missing_or_duplicate_formula_entries_before_writing(self):
        for seed in (
            self.original.replace('x86_64-unknown-linux-gnu.tar.gz', 'unsupported.tar.gz'),
            self.original + '\nversion "9.9.9"\n',
            self.original + '\nurl "x86_64-unknown-linux-gnu.tar.gz"\nsha256 "old"\n',
        ):
            with self.subTest(seed=seed):
                self.formula.write_text(seed)
                self.assertNotEqual(self.transform().returncode, 0)
                self.assertEqual(self.formula.read_text(), seed)

    def test_rejects_version_injection_before_writing(self):
        self.assertNotEqual(self.transform(version='1.2.3"; system("bad")').returncode, 0)
        self.assertEqual(self.formula.read_text(), self.original)

    def caller_function(self, script, name):
        text = (ROOT / "scripts" / script).read_text()
        start = text.index(name + "() {")
        end = text.index("\n}\n", start) + 3
        return text[start:end]

    def test_callers_supply_all_targets_from_checksum_files(self):
        self.check_callers_with_checksum_files(remote_linux=False)

    def test_release_callers_fetch_when_only_intel_linux_checksum_is_missing(self):
        self.check_callers_with_checksum_files(remote_linux=True)

    def check_callers_with_checksum_files(self, remote_linux):
        dist = self.workspace / "dist"
        dist.mkdir()
        downloads = self.workspace / "downloads"
        downloads.mkdir()
        for target, checksum in zip(TARGETS, CHECKSUMS):
            filename = f"vtcode-1.2.3-{target}.sha256"
            (downloads / filename).write_text(f"{checksum}  archive.tar.gz\n")
            if not remote_linux or target != "x86_64-unknown-linux-gnu":
                (dist / filename).write_text(f"{checksum}  archive.tar.gz\n")
        scripts = ["release.sh", "release-lib.sh"]
        if not remote_linux:
            scripts.append("build-and-upload-binaries.sh")
        for script in scripts:
            with self.subTest(script=script):
                self.formula.write_text(self.original)
                preamble = '''
set -euo pipefail
source "$1"
print_info() { :; }
print_error() { :; }
print_success() { :; }
print_warning() { :; }
forbidden() { printf 'publication attempted\\n' >> forbidden; return 99; }
git() { if [[ "$1" == diff ]]; then return 0; fi; forbidden; }
gh() {
    if [[ "$REMOTE_LINUX" == true && "$1" == release && "$2" == download && "$4" == --dir ]]; then
        cp "$DOWNLOADS"/*.sha256 "$5/"
        printf 'fixture download\\n' >> downloads-used
    else
        forbidden
    fi
}
curl() { forbidden; }
cargo() { forbidden; }
'''
                if script == "build-and-upload-binaries.sh":
                    function = self.caller_function(script, "update_homebrew_formula")
                    preamble += '''
DRY_RUN=false
publish_homebrew_tap() {
    [[ "$#" == 5 && "$2" == "$MAC_INTEL" && "$3" == "$MAC_ARM" && "$4" == "$LINUX_ARM" && "$5" == "$LINUX_INTEL" ]]
}
'''
                    invocation = 'update_homebrew_formula "1.2.3"'
                else:
                    function = self.caller_function(script, "publish_homebrew_tap")
                    # Local transformation precedes the caller's missing-token error.
                    invocation = 'if publish_homebrew_tap "1.2.3"; then exit 98; fi'
                env = os.environ.copy()
                env.update(zip(("MAC_INTEL", "MAC_ARM", "LINUX_ARM", "LINUX_INTEL"), CHECKSUMS))
                env.update(HOMEBREW_TAP_TOKEN="", GITHUB_TOKEN="")
                env.update(REMOTE_LINUX=str(remote_linux).lower(), DOWNLOADS=str(downloads))
                result = subprocess.run(
                    ["bash", "-c", preamble + function + "\n" + invocation,
                     "fixture", str(ROOT / "scripts" / "homebrew-formula.sh")],
                    cwd=self.workspace, env=env, capture_output=True, text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assert_updated()
                self.assertFalse((self.workspace / "forbidden").exists())
        self.assertEqual((self.workspace / "downloads-used").exists(), remote_linux)


if __name__ == "__main__":
    unittest.main()
