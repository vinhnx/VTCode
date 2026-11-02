"""Regression coverage for the platform-gated `unsafe` documentation gate.

The gate reproduces what `rust.unsafe_code` and `clippy::undocumented_unsafe_blocks`
require, but for every file regardless of whether it compiles on the host. These
tests pin the accepted/rejected shapes against clippy's real behavior, so a
future edit cannot silently make the gate under- or over-report.
"""

import importlib.util
import io
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "check_unsafe_docs", SCRIPTS / "check_unsafe_docs.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class ScanFileTests(unittest.TestCase):
    def scan(self, rel_path, content, *, root_files=None, rules=None):
        with tempfile.TemporaryDirectory(prefix="vtcode-unsafe-docs-") as directory:
            root = Path(directory)
            for name, text in (root_files or {}).items():
                extra = root / name
                extra.parent.mkdir(parents=True, exist_ok=True)
                extra.write_text(text)
            target = root / rel_path
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(content)
            with mock.patch.object(module, "REPO_ROOT", root):
                return module.scan_file(target, rules or [])

    def assert_reasons(self, findings, expected):
        self.assertEqual(sorted(f.reason for f in findings), sorted(expected))

    def test_bare_unsafe_reports_both_gaps(self):
        findings = self.scan(
            "src/sandboxing/linux.rs", "fn probe() {\n    unsafe { f() }\n}\n"
        )
        self.assert_reasons(
            findings, ["missing `unsafe_code` allowance", "missing SAFETY marker"]
        )
        self.assertEqual({f.line for f in findings}, {2})

    def test_marker_and_allowance_satisfy_the_gate(self):
        content = (
            '#[allow(unsafe_code, reason = "raw syscall probe")]\n'
            "fn probe() {\n"
            "    // SAFETY: documented probe that touches no user memory.\n"
            "    unsafe { f() }\n"
            "}\n"
        )
        self.assertEqual(self.scan("src/lib.rs", content), [])

    def test_crate_root_allowance_is_inherited(self):
        findings = self.scan(
            "src/a.rs",
            "fn f() {\n    // SAFETY: caller upholds the contract.\n    unsafe { g() }\n}\n",
            root_files={
                "src/lib.rs": '#![allow(unsafe_code, reason = "crate does raw FFI")]\n'
            },
        )
        self.assertEqual(findings, [])

    def test_type_alias_unsafe_does_not_fire_clippy_rules_but_needs_a_contract(self):
        for content in (
            'type Cb = unsafe extern "C" fn() -> u32;\n',
            'type Cb2 =\n    unsafe extern "C" fn() -> u32;\n',
        ):
            with self.subTest(content=content):
                findings = self.scan("src/lib.rs", content)
                reasons = [f.reason for f in findings]
                # Type-position `unsafe` fires neither clippy lint...
                self.assertNotIn("missing `unsafe_code` allowance", reasons)
                self.assertNotIn("missing SAFETY marker", reasons)
                # ...but it still needs a contract doc (Rule C).
                self.assertEqual(
                    reasons, ["type-position `unsafe` needs a contract doc"]
                )

    def test_type_alias_unsafe_with_contract_doc_passes(self):
        for marker in (
            "/// Contract: the pointer is valid while the library is loaded.",
            "    /// Indented doc comment also counts.",
        ):
            content = f'{marker}\ntype Cb = unsafe extern "C" fn() -> u32;\n'
            with self.subTest(marker=marker):
                self.assertEqual(self.scan("src/lib.rs", content), [])

    def test_type_alias_unsafe_safety_comment_is_not_a_contract_doc(self):
        # clippy rejects `// SAFETY:` on type-position `unsafe`
        # (`unnecessary_safety_comment`), so Rule C requires a `///` doc instead.
        content = (
            '// SAFETY: not accepted here.\ntype Cb = unsafe extern "C" fn() -> u32;\n'
        )
        self.assert_reasons(
            self.scan("src/lib.rs", content),
            ["type-position `unsafe` needs a contract doc"],
        )

    def test_plain_type_alias_needs_no_contract(self):
        self.assertEqual(
            self.scan("src/lib.rs", 'type Cb = extern "C" fn() -> u32;\n'), []
        )

    def test_unsafe_extern_block_needs_no_marker(self):
        content = (
            '#[allow(unsafe_code, reason = "win32 ffi")]\n'
            'unsafe extern "system" { fn declared(a: u32) -> i32; }\n'
        )
        self.assertEqual(self.scan("src/lib.rs", content), [])

    def test_unsafe_impl_needs_a_marker(self):
        content = (
            '#[allow(unsafe_code, reason = "probe")]\n'
            "struct S;\n"
            "unsafe impl Send for S {}\n"
        )
        self.assert_reasons(self.scan("src/lib.rs", content), ["missing SAFETY marker"])

    def test_marker_is_case_insensitive_but_must_be_prefixed(self):
        accepted = (
            '#[allow(unsafe_code, reason = "probe")]\n'
            "fn ok() {\n    // Safety: lowercase marker word still counts.\n    unsafe { f() }\n}\n"
        )
        rejected = (
            '#[allow(unsafe_code, reason = "probe")]\n'
            "fn bad() {\n    // This sentence merely mentions SAFETY without the marker.\n    unsafe { f() }\n}\n"
        )
        self.assertEqual(self.scan("src/lib.rs", accepted), [])
        self.assert_reasons(
            self.scan("src/lib.rs", rejected), ["missing SAFETY marker"]
        )

    def test_allowlist_entry_suppresses_a_finding(self):
        content = "fn probe() {\n    unsafe { f() }\n}\n"
        rules = [module.AllowRule(pattern="src/lib.rs", line=None)]
        self.assertEqual(self.scan("src/lib.rs", content, rules=rules), [])

    def test_excluded_paths_cover_vendored_and_test_trees(self):
        with mock.patch.object(module, "REPO_ROOT", Path("/repo")):
            for rel in (
                "patches/crossterm/src/tty.rs",
                "fuzz/target.rs",
                "tests/it.rs",
                "benches/b.rs",
            ):
                with self.subTest(rel=rel):
                    self.assertTrue(module.is_excluded(Path("/repo") / rel))
            self.assertFalse(module.is_excluded(Path("/repo/crates/x/src/a.rs")))


class InvocationTests(unittest.TestCase):
    def run_main(self, argv, files):
        output = io.StringIO()
        with (
            mock.patch.object(module, "parse_allow_rules", return_value=[]),
            mock.patch.object(module, "expand_paths", return_value=files),
            mock.patch.object(sys, "argv", ["check_unsafe_docs.py", *argv]),
            redirect_stdout(output),
        ):
            return module.main(), output.getvalue()

    def test_enforce_mode_fails_on_findings_and_warn_mode_passes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            bad = root / "src" / "bad.rs"
            bad.parent.mkdir(parents=True)
            bad.write_text("fn probe() {\n    unsafe { f() }\n}\n")

            with mock.patch.object(module, "REPO_ROOT", root):
                enforce_status, _ = self.run_main(["--mode", "enforce"], [bad])
                warn_status, _ = self.run_main(["--mode", "warn"], [bad])

        self.assertEqual(enforce_status, 1)
        self.assertEqual(warn_status, 0)

    def test_clean_tree_passes_in_enforce_mode(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            good = root / "src" / "good.rs"
            good.parent.mkdir(parents=True)
            good.write_text(
                '#[allow(unsafe_code, reason = "probe")]\n'
                "fn f() {\n    // SAFETY: nothing is dereferenced.\n    unsafe { g() }\n}\n"
            )
            with mock.patch.object(module, "REPO_ROOT", root):
                status, _ = self.run_main(["--mode", "enforce"], [good])

        self.assertEqual(status, 0)


if __name__ == "__main__":
    unittest.main()
