"""memstat.sh must pass commands through, with counters where perf exists."""

import subprocess
import unittest
from pathlib import Path

SCRIPT = Path(__file__).resolve().parents[1] / "perf/memstat.sh"


class MemstatTests(unittest.TestCase):
    def run_memstat(self, *args):
        return subprocess.run([str(SCRIPT), *args], capture_output=True, text=True)

    def test_no_args_prints_usage_and_exits_2(self):
        result = self.run_memstat()
        self.assertEqual(result.returncode, 2)
        self.assertIn("usage:", result.stderr)

    def test_stdout_passes_through(self):
        result = self.run_memstat("echo", "hello-memstat")
        self.assertEqual(result.returncode, 0)
        self.assertIn("hello-memstat", result.stdout)

    def test_double_dash_separator_is_accepted(self):
        result = self.run_memstat("--", "echo", "dash-ok")
        self.assertEqual(result.returncode, 0)
        self.assertIn("dash-ok", result.stdout)

    def test_failing_command_exit_propagates(self):
        result = self.run_memstat("sh", "-c", "exit 7")
        self.assertEqual(result.returncode, 7)


if __name__ == "__main__":
    unittest.main()
