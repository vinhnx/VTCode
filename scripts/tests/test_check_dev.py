"""The development gate must preserve its lockfile and fail on missing test evidence."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SCRIPTS = Path(__file__).resolve().parents[1]


class CheckDevTests(unittest.TestCase):
    def run_fixture(self, options=(), quick=True, changed_paths=("src/lib.rs",), **scenario):
        with tempfile.TemporaryDirectory(prefix="vtcode-check-dev-") as directory:
            root = Path(directory)
            scripts = root / "scripts"
            scripts.mkdir()
            for name in ("check-dev.sh", "common.sh"):
                shutil.copy2(SCRIPTS / name, scripts / name)
            if quick:
                config = root / ".config/nextest.toml"
                config.parent.mkdir()
                config.write_text("[profile.quick]\nfail-fast = true\n")
            lockfile = root / "Cargo.lock"
            lockfile.write_text("fixture lockfile\n")
            commands = root / "commands"
            commands.mkdir()
            cargo = commands / "cargo"
            cargo.write_text(
                f"#!{sys.executable}\n"
                "import json, os, pathlib, sys\n"
                "args = sys.argv[1:]\n"
                "scenario = json.loads(os.environ['CHECK_DEV_SCENARIO'])\n"
                "with open(os.environ['CHECK_DEV_CALLS'], 'a') as log:\n"
                "    log.write(json.dumps(args) + '\\n')\n"
                "if args == ['nextest', '--version']:\n"
                "    sys.exit(0 if scenario.get('nextest_available', True) else 1)\n"
                "is_verifier = '--version' not in args and (args[0] in ('check', 'clippy', 'metadata') or args[:2] == ['nextest', 'run'])\n"
                "if is_verifier and '--locked' not in args:\n"
                "    pathlib.Path('Cargo.lock').write_text('unlocked resolution changed the lockfile\\n')\n"
                "if args[0] == 'metadata':\n"
                "    print(json.dumps({'packages': [{'name': 'vtcode', 'manifest_path': str(pathlib.Path.cwd() / 'Cargo.toml')}]}))\n"
                "    sys.exit(scenario.get('metadata_status', 0))\n"
                "if args[:2] == ['nextest', 'run']:\n"
                "    if scenario.get('empty_suite') and '--no-tests=fail' in args:\n"
                "        sys.exit(4)\n"
                "    sys.exit(scenario.get('nextest_status', 0))\n"
                "if is_verifier:\n"
                "    sys.exit(scenario.get(args[0] + '_status', 0))\n"
            )
            cargo.chmod(0o755)
            git = commands / "git"
            git.write_text(
                f"#!{sys.executable}\n"
                "import json, os, sys\n"
                "if sys.argv[1] == 'diff':\n"
                "    print('\\n'.join(json.loads(os.environ['CHECK_DEV_CHANGED_PATHS'])))\n"
                "elif sys.argv[1] == 'rev-parse':\n"
                "    print('.git')\n"
                "elif sys.argv[1] != 'ls-files':\n"
                "    sys.exit(91)\n"
            )
            git.chmod(0o755)
            calls_file = root / "calls.jsonl"
            environment = {
                **os.environ,
                "PATH": f"{commands}:{os.environ.get('PATH', '')}",
                "CHECK_DEV_CALLS": str(calls_file),
                "CHECK_DEV_SCENARIO": json.dumps(scenario),
                "CHECK_DEV_CHANGED_PATHS": json.dumps(changed_paths),
            }
            result = subprocess.run(
                ["bash", str(scripts / "check-dev.sh"), *options],
                cwd=root, env=environment, capture_output=True, text=True, timeout=20,
            )
            calls = [json.loads(line) for line in calls_file.read_text().splitlines()]
            return result, calls, lockfile.read_text()

    def assert_success(self, result):
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("All checks passed!", result.stdout)

    def assert_failure(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertNotIn("All checks passed!", result.stdout)

    def test_default_and_workspace_checks_preserve_lockfile(self):
        for options in ((), ("--workspace", "--quiet")):
            with self.subTest(options=options):
                result, calls, lockfile = self.run_fixture(options)
                self.assert_success(result)
                self.assertEqual(lockfile, "fixture lockfile\n")
                for command in ("clippy", "check"):
                    call = next(call for call in calls if call[0] == command and "--version" not in call)
                    self.assertIn("--locked", call)
                    self.assertEqual("--workspace" in call, "--workspace" in options)
                self.assertFalse(any(call[0] == "test" or call[:2] == ["nextest", "run"] for call in calls))

    def test_tests_use_locked_nextest_with_quick_or_default_profile(self):
        for quick in (True, False):
            with self.subTest(quick=quick):
                result, calls, lockfile = self.run_fixture(("--test", "--quiet"), quick=quick)
                self.assert_success(result)
                self.assertEqual(lockfile, "fixture lockfile\n")
                call = next(call for call in calls if call[:2] == ["nextest", "run"])
                self.assertIn("--locked", call)
                self.assertIn("--no-tests=fail", call)
                self.assertEqual(call[call.index("--profile") + 1], "quick" if quick else "default")

    def test_changed_selection_uses_locked_metadata_and_package_filter(self):
        result, calls, lockfile = self.run_fixture(("--changed",))
        self.assert_success(result)
        self.assertEqual(lockfile, "fixture lockfile\n")
        self.assertIn("--locked", next(call for call in calls if call[0] == "metadata"))
        call = next(call for call in calls if call[:2] == ["nextest", "run"])
        self.assertEqual(call[call.index("-p") + 1], "vtcode")

    def test_metadata_failure_is_not_treated_as_successful_selection(self):
        # Emit valid JSON before failing: only the pipeline exit status exposes the failure.
        result, calls, _ = self.run_fixture(("--changed",), metadata_status=23)
        self.assert_failure(result)
        self.assertIn("Failed to detect changed packages", result.stdout)
        self.assertFalse(any(call[:2] == ["nextest", "run"] for call in calls))

    def test_unchanged_scope_explicitly_skips_tests(self):
        for options in (("--changed",), ("--changed", "--quiet")):
            with self.subTest(options=options):
                result, calls, _ = self.run_fixture(options, changed_paths=())
                self.assert_success(result)
                self.assertIn("No changed packages in the selected test scope; skipping tests.", result.stdout)
                self.assertFalse(any(call[:2] == ["nextest", "run"] for call in calls))

    def test_missing_nextest_fails_without_using_another_runner(self):
        result, calls, _ = self.run_fixture(("--test",), nextest_available=False)
        self.assert_failure(result)
        self.assertIn("cargo-nextest is required", result.stdout)
        self.assertFalse(any(call[0] == "test" or call[:2] == ["nextest", "run"] for call in calls))

    def test_empty_suite_and_failed_tests_fail_the_gate(self):
        for scenario in ({"empty_suite": True}, {"nextest_status": 29}):
            with self.subTest(scenario=scenario):
                result, _, _ = self.run_fixture(("--test",), **scenario)
                self.assert_failure(result)
                self.assertNotIn("All tests passed!", result.stdout)

    def test_failed_locked_check_or_clippy_fails_the_gate(self):
        for scenario in ({"check_status": 31}, {"clippy_status": 32}):
            with self.subTest(scenario=scenario):
                result, _, lockfile = self.run_fixture(**scenario)
                self.assert_failure(result)
                self.assertEqual(lockfile, "fixture lockfile\n")


if __name__ == "__main__":
    unittest.main()
