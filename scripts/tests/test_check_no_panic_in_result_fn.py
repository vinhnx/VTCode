"""Regression coverage for the panic-in-Result production scan.

Risk-first notes (agentic-testing): every fixture mixes a positive and a
negative case so a scanner that flags everything (or nothing) cannot pass.
The `git ls-files` snapshot can also name a file a concurrent session deletes
before it is read; the scan must skip that path without losing its findings.
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
sys.path.insert(0, str(SCRIPTS))

unwrap_spec = importlib.util.spec_from_file_location(
    "check_no_unwrap_expect_prod", SCRIPTS / "check_no_unwrap_expect_prod.py"
)
unwrap_module = importlib.util.module_from_spec(unwrap_spec)
sys.modules[unwrap_spec.name] = unwrap_module
unwrap_spec.loader.exec_module(unwrap_module)

spec = importlib.util.spec_from_file_location(
    "check_no_panic_in_result_fn", SCRIPTS / "check_no_panic_in_result_fn.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class PanicInResultScanTests(unittest.TestCase):
    def run_scan(
        self, root: Path, source: str, allowlist: str = ""
    ) -> tuple[int, str, list]:
        src = root / "src" / "lib.rs"
        src.parent.mkdir(parents=True, exist_ok=True)
        src.write_text(source, encoding="utf-8")
        allow = root / "allow.txt"
        allow.write_text(allowlist, encoding="utf-8")

        stdout = io.StringIO()
        with (
            mock.patch.object(module, "REPO_ROOT", root),
            mock.patch.object(unwrap_module, "REPO_ROOT", root),
            mock.patch.object(module, "list_tracked_rust_files", return_value=[src]),
            mock.patch.object(
                sys,
                "argv",
                [
                    "check_no_panic_in_result_fn.py",
                    "--mode",
                    "enforce",
                    "--allowlist",
                    str(allow),
                ],
            ),
            contextlib.redirect_stdout(stdout),
            contextlib.redirect_stderr(io.StringIO()),
        ):
            code = module.main()
            findings = module.scan_file(src, [])
        return code, stdout.getvalue(), findings

    def test_flags_result_fn_but_not_plain_fn(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn fallible() -> std::io::Result<()> {\n"
                "    assert_eq!(1, 1);\n"
                "    Ok(())\n"
                "}\n"
                "fn infallible() {\n"
                "    assert_eq!(2, 2);\n"
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertIn("findings=1", out)
            self.assertEqual([line for _, line, _ in findings], [2])

    def test_multiline_signature_is_tracked_and_cfg_test_is_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn fallible(\n"
                "    value: u8,\n"
                ") -> anyhow::Result<()> {\n"
                '    unreachable!("impossible: {value}");\n'
                "}\n"
                "#[cfg(test)]\n"
                "mod tests {\n"
                "    fn helper() -> Result<()> {\n"
                "        assert!(true);\n"
                "        Ok(())\n"
                "    }\n"
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertIn("findings=1", out)
            self.assertEqual([line for _, line, _ in findings], [4])

    def test_trait_declaration_does_not_swallow_following_function(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "trait T {\n"
                "    fn declared(&self) -> Result<()>;\n"
                "}\n"
                "fn plain() {\n"
                "    assert!(true);\n"
                "}\n"
                "fn fallible() -> Result<()> {\n"
                '    panic!("boom");\n'
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertEqual([line for _, line, _ in findings], [8])

    def test_one_line_result_body_is_scanned(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                'fn tight() -> Result<()> { panic!("one-line"); Ok(()) }\n'
                "trait T {\n"
                "    fn declared(&self) -> Result<()>;\n"
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertEqual([line for _, line, _ in findings], [1])

    def test_debug_assert_variants_are_detected_in_result_fns(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn a() -> Result<()> {\n"
                '    debug_assert!(false, "invariant");\n'
                "    Ok(())\n"
                "}\n"
                "fn b() -> Result<()> {\n"
                "    debug_assert_eq!(1, 1);\n"
                "    Ok(())\n"
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertEqual([line for _, line, _ in findings], [2, 6])

    def test_test_modules_are_skipped_but_production_after_them_is_scanned(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn first() -> Result<()> {\n"
                '    panic!("first");\n'
                "    Ok(())\n"
                "}\n"
                "#[cfg(test)]\n"
                "mod foo {\n"
                "    #[test]\n"
                "    fn t() -> anyhow::Result<()> {\n"
                "        assert!(true);\n"
                "        Ok(())\n"
                "    }\n"
                "}\n"
                "mod tests { fn nested() {} }\n"
                "fn second() -> Result<()> {\n"
                '    panic!("second");\n'
                "    Ok(())\n"
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 1, out)
            self.assertEqual([line for _, line, _ in findings], [2, 15])

    def test_non_result_returns_and_string_braces_create_no_findings(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn iterator() -> impl Iterator<Item = Result<(), E>> {\n"
                '    panic!("not a Result");\n'
                "}\n"
                "fn generic<T>() -> T where T: Into<Result<(), E>> {\n"
                '    panic!("not a Result");\n'
                "}\n"
                "fn holder() -> Result<()> {\n"
                '    let url = "http://example/{brace}";\n'
                "    Ok(())\n"
                "}\n"
                "fn plain() {\n"
                '    panic!("plain fn");\n'
                "}\n"
            )
            code, out, findings = self.run_scan(root, source)
            self.assertEqual(code, 0, out)
            self.assertEqual(findings, [])

    def test_line_scoped_allowlist_entry_is_honored(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            source = (
                "fn fallible() -> Result<()> {\n    assert!(true);\n    Ok(())\n}\n"
            )
            code, out, _ = self.run_scan(
                root, source, "src/lib.rs:2 | proven invariant guard\n"
            )
            self.assertEqual(code, 0, out)
            self.assertIn("findings=0", out)

    def test_vanished_file_skipped_without_losing_findings(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = root / "src" / "lib.rs"
            src.parent.mkdir(parents=True, exist_ok=True)
            src.write_text(
                "fn fallible() -> Result<()> {\n    assert!(true);\n    Ok(())\n}\n",
                encoding="utf-8",
            )
            ghost = root / "src" / "gone.rs"
            allow = root / "allow.txt"
            allow.write_text("", encoding="utf-8")
            stdout = io.StringIO()
            with (
                mock.patch.object(module, "REPO_ROOT", root),
                mock.patch.object(unwrap_module, "REPO_ROOT", root),
                mock.patch.object(
                    module, "list_tracked_rust_files", return_value=[src, ghost]
                ),
                mock.patch.object(
                    sys,
                    "argv",
                    [
                        "check_no_panic_in_result_fn.py",
                        "--mode",
                        "warn",
                        "--allowlist",
                        str(allow),
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
