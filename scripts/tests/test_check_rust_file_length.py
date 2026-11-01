"""Regression coverage for the Rust file-length governance scan.

Risk-first notes (agentic-testing): the bug-prone surface is the TOCTOU gap
between the `git ls-files` snapshot and file reads. A concurrent session can
delete or rename a file mid-scan, which used to crash the whole Zen step with
FileNotFoundError. These tests pin the fail-open behavior with literal
fixtures (never the production counter as oracle):

- a vanished path counts as skipped, never a crash;
- surviving files are still measured and budgeted;
- exactly `max_lines` passes while `max_lines + 1` fails in enforce mode.
"""

import contextlib
import importlib.util
import io
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "check_rust_file_length", SCRIPTS / "check_rust_file_length.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def write_lines(path: Path, count: int) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("content line\n" * count, encoding="utf-8")
    return path


class CountLinesTests(unittest.TestCase):
    def test_missing_path_returns_none(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(module.count_lines(Path(tmp) / "gone.rs"))

    def test_counts_literal_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_lines(Path(tmp) / "a.rs", 7)
            self.assertEqual(module.count_lines(path), 7)


class MissingFileRaceTests(unittest.TestCase):
    def run_main(self, files, *argv):
        stdout = io.StringIO()
        with (
            mock.patch.object(module, "list_tracked_rust_files", return_value=files),
            mock.patch.object(sys, "argv", ["check_rust_file_length.py", *argv]),
            contextlib.redirect_stdout(stdout),
        ):
            code = module.main()
        return code, stdout.getvalue()

    def test_vanished_file_skipped_without_crashing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            real = write_lines(root / "src" / "lib.rs", 10)
            ghost = root / "src" / "gone.rs"
            with mock.patch.object(module, "REPO_ROOT", root):
                code, out = self.run_main([real, ghost], "--mode", "warn")
        self.assertEqual(code, 0, out)
        self.assertIn("skipped_missing=1", out)
        self.assertIn("total=1", out)

    def test_max_lines_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exact = write_lines(root / "exact.rs", 500)
            over = write_lines(root / "over.rs", 501)
            with mock.patch.object(module, "REPO_ROOT", root):
                code, _ = self.run_main([exact], "--mode", "enforce", "--max-lines", "500")
                self.assertEqual(code, 0)
                code, _ = self.run_main([over], "--mode", "enforce", "--max-lines", "500")
                self.assertEqual(code, 1)


if __name__ == "__main__":
    unittest.main()
