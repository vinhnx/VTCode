"""Regression coverage for the unwrap/expect production scan.

Risk-first notes (agentic-testing): same TOCTOU gap as the file-length
scans — the `git ls-files` snapshot can name a file a concurrent session
deletes before it is read. The scan must skip the vanished path and keep
its findings, never crash the Zen step.
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
    "check_no_unwrap_expect_prod", SCRIPTS / "check_no_unwrap_expect_prod.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class MissingFileRaceTests(unittest.TestCase):
    def test_vanished_file_skipped_without_losing_findings(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            dirty = root / "src" / "lib.rs"
            dirty.parent.mkdir(parents=True, exist_ok=True)
            dirty.write_text('fn f() {\n    let x = foo.unwrap();\n}\n', encoding="utf-8")
            ghost = root / "src" / "gone.rs"
            stdout = io.StringIO()
            with (
                mock.patch.object(module, "REPO_ROOT", root),
                mock.patch.object(
                    module, "list_tracked_rust_files", return_value=[dirty, ghost]
                ),
                mock.patch.object(
                    sys,
                    "argv",
                    [
                        "check_no_unwrap_expect_prod.py",
                        "--mode",
                        "warn",
                        "--allowlist",
                        str(root / "no-allowlist.txt"),
                    ],
                ),
                contextlib.redirect_stdout(stdout),
            ):
                code = module.main()
        out = stdout.getvalue()
        self.assertEqual(code, 0, out)
        self.assertIn("skipped_missing=1", out)
        self.assertIn("scanned_files=1", out)
        self.assertIn("findings=1", out)
        self.assertIn("src/lib.rs:2", out)


if __name__ == "__main__":
    unittest.main()
