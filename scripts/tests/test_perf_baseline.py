"""The performance harness must rebuild and stop when a verifier fails."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


class PerformanceBaselineTests(unittest.TestCase):
    def run_fixture(self, build_status, check_status=29, captures=1):
        with tempfile.TemporaryDirectory(prefix="vtcode-perf-test-") as directory:
            root = Path(directory)
            script = root / "scripts/perf/baseline.sh"
            script.parent.mkdir(parents=True)
            source = Path(__file__).resolve().parents[1] / "perf/baseline.sh"
            shutil.copy2(source, script)
            shutil.copy2(source.with_name("startup_env.py"), script.with_name("startup_env.py"))
            binary = root / "target/release/vtcode"
            binary.parent.mkdir(parents=True)
            if check_status:
                binary.write_text("#!/bin/sh\nexit 91\n")
            else:
                binary.write_text(
                    f"#!{sys.executable}\n"
                    "import os, pathlib, sys, time\n"
                    "assert 'OPENAI_API_KEY' not in os.environ\n"
                    "assert 'ANTHROPIC_API_KEY' not in os.environ\n"
                    "root = pathlib.Path(os.environ['TMPDIR']).parent\n"
                    "assert pathlib.Path.cwd() == root / 'workspace'\n"
                    "for key in ['HOME', 'VTCODE_CONFIG_PATH', 'XDG_DATA_HOME', 'CODEX_HOME']:\n"
                    "    assert pathlib.Path(os.environ[key]).is_relative_to(root)\n"
                    "assert pathlib.Path(os.environ['VTCODE_CONFIG_PATH']).read_text() == ''\n"
                    "assert os.environ['OLLAMA_BASE_URL'] == 'http://127.0.0.1:1'\n"
                    "if '--provider' in sys.argv:\n"
                    "    print('Type a request', flush=True)\n"
                    "    # Keep output pending when the parent stops the owned PTY child.\n"
                    "    for _ in range(1000):\n"
                    "        os.write(1, b'pending terminal output ' * 1000)\n"
                    "    time.sleep(30)\n"
                )
            binary.chmod(0o755)
            commands = root / "commands"
            commands.mkdir()
            cargo = commands / "cargo"
            cargo.write_text(
                '#!/bin/sh\nprintf "%s\\n" "$*" >> "$PERF_TEST_CALLS"\n'
                'if [ "$1" = build ]; then\n'
                f"  exit {build_status}\n"
                f"fi\nexit {check_status}\n"
            )
            cargo.chmod(0o755)
            calls = root / "calls"
            environment = os.environ.copy()
            environment.update(
                PATH=f"{commands}:{environment.get('PATH', '')}",
                PERF_TEST_CALLS=str(calls),
                PERF_RUN_BENCHMARKS="0",
                OPENAI_API_KEY="synthetic-must-not-propagate",
                ANTHROPIC_API_KEY="synthetic-must-not-propagate",
                XDG_DATA_HOME=str(root / "global-data"),
                CODEX_HOME=str(root / "global-codex"),
            )
            for _ in range(captures):
                result = subprocess.run(
                    ["bash", str(script), "fixture"],
                    env=environment,
                    capture_output=True,
                    text=True,
                    timeout=20,
                )
                if result.returncode == 0:
                    terminal_log = root / ".vtcode/perf/fixture-interactive_first_render.log"
                    self.assertEqual(terminal_log.read_bytes().count(b"Type a request"), 3)
            return result, calls.read_text().splitlines(), (root / ".vtcode/perf/fixture.json").exists()

    def test_existing_binary_does_not_skip_failed_rebuild(self):
        result, calls, summary_exists = self.run_fixture(23)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual(calls, ["build --release --locked --bin vtcode"])
        self.assertFalse(summary_exists)

    def test_check_failure_stops_measurement_after_successful_build(self):
        result, calls, summary_exists = self.run_fixture(0)
        self.assertEqual(result.returncode, 29, result.stderr)
        self.assertEqual(
            calls,
            ["build --release --locked --bin vtcode", "check --workspace --quiet --locked"],
        )
        self.assertFalse(summary_exists)

    def test_repeated_capture_isolates_routes_and_replaces_terminal_log(self):
        result, calls, summary_exists = self.run_fixture(0, 0, captures=2)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            calls,
            ["build --release --locked --bin vtcode", "check --workspace --quiet --locked"] * 2,
        )
        self.assertTrue(summary_exists)

    def test_comparison_distinguishes_build_cost_and_changed_isolation(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-perf-compare-") as directory:
            root = Path(directory)
            script = root / "scripts/perf/compare.sh"
            script.parent.mkdir(parents=True)
            shutil.copy2(Path(__file__).resolve().parents[1] / "perf/compare.sh", script)
            (root / ".vtcode/perf").mkdir(parents=True)
            baseline = root / "baseline.json"
            current = root / "current.json"
            baseline.write_text(json.dumps({"metrics": {"release_build_ms": 100, "warm_startup_ms": 10}}))
            current.write_text(json.dumps({
                "metrics": {"release_build_ms": 50, "warm_startup_ms": 8},
                "startup_environment": "isolated-workspace-v1",
            }))
            result = subprocess.run(
                ["bash", str(script), str(baseline), str(current)],
                capture_output=True,
                text=True,
                timeout=20,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(
                "| `release_build_ms` | 100 | 50 | -50.00% | command wall time (includes build/setup) |",
                result.stdout,
            )
            self.assertIn("| `warm_startup_ms` | 10 | 8 | -20.00% | faster |", result.stdout)
            self.assertIn("startup environments differ", result.stdout)


if __name__ == "__main__":
    unittest.main()
