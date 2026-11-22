"""Regression coverage for the host-vs-target cfg gate in build scripts.

Risk-first notes (agentic-testing): the bug-prone surface is the detector's
boundary, not the happy path. These tests pin both sides of it:

- target-looking predicates are flagged (including nested `not(all(...))`),
  while host-independent ones (`feature`, `test`, `debug_assertions`) are not;
- `CARGO_CFG_TARGET_*` — the correct pattern — is never matched;
- commented-out and string-literal examples are masked, so prose and docs
  describing the bug do not produce findings;
- a Rust lifetime (`'a`, `'static`) must not be mistaken for an unterminated
  char literal, which would mask the rest of the file and hide findings;
- `cfg_attr` is covered too, while similarly named macros (`mycfg!`) are not;
- line/column reporting points at the real source location;
- `scan_repo` sees only tracked `build.rs` files that still exist on disk.
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location(
    "check_build_script_cfg", SCRIPTS / "check_build_script_cfg.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class ScanTextTests(unittest.TestCase):
    def test_flags_plain_windows_cfg(self):
        findings = module.scan_text("fn main() { if cfg!(windows) {} }", "build.rs")
        self.assertEqual(len(findings), 1)
        self.assertIn("cfg!(windows)", findings[0].snippet)

    def test_flags_nested_target_predicate(self):
        source = 'if cfg!(not(all(windows, target_env = "msvc"))) {}'
        findings = module.scan_text(source, "build.rs")
        self.assertEqual(len(findings), 1)

    def test_flags_attribute_form(self):
        source = '#[cfg(target_os = "macos")]\nfn f() {}'
        self.assertEqual(len(module.scan_text(source, "build.rs")), 1)

    def test_flags_every_target_key(self):
        keys = [
            "target_os",
            "target_arch",
            "target_env",
            "target_family",
            "target_abi",
            "target_vendor",
            "target_endian",
            "target_pointer_width",
            "windows",
            "unix",
        ]
        for key in keys:
            with self.subTest(key=key):
                source = f'if cfg!({key} = "x") {{}}'
                self.assertEqual(len(module.scan_text(source, "build.rs")), 1)

    def test_flags_cfg_attr_target_predicate(self):
        source = '#[cfg_attr(target_os = "windows", derive(Debug))]\nfn f() {}'
        self.assertEqual(len(module.scan_text(source, "build.rs")), 1)

    def test_similarly_named_macro_is_not_flagged(self):
        source = "fn main() { mycfg!(windows) {} }"
        self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_lifetime_does_not_mask_later_predicate(self):
        # A `'static` with no later `'` used to blank the whole tail of the file.
        source = 'fn flag() -> &\'static str {\n    if cfg!(windows) { "a" } else { "b" }\n}\n'
        findings = module.scan_text(source, "build.rs")
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].line, 2)

    def test_char_literal_does_not_mask_later_predicate(self):
        for literal in ["'x'", "'/'", "'\\n'", "'\\u{1F600}'"]:
            with self.subTest(literal=literal):
                source = f"fn main() {{ let c = {literal}; if cfg!(windows) {{}} }}"
                findings = module.scan_text(source, "build.rs")
                self.assertEqual(len(findings), 1)
                self.assertIn("cfg!(windows)", findings[0].snippet)

    def test_allows_host_independent_cfg(self):
        for source in [
            'if cfg!(feature = "test-support") {}',
            "if cfg!(test) {}",
            "if cfg!(debug_assertions) {}",
            "#[cfg(not(docsrs))] fn f() {}",
        ]:
            with self.subTest(source=source):
                self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_allows_env_var_pattern(self):
        source = 'let target = var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");'
        self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_masks_line_comment_example(self):
        source = (
            "// cfg!(windows) here would check the host, not the target\nfn main() {}"
        )
        self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_masks_block_comment_example(self):
        source = '/* #[cfg(target_env = "msvc")] */\nfn main() {}'
        self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_masks_string_literal_example(self):
        source = 'let example = "cfg!(target_os = \\"macos\\")";\nfn main() {}'
        self.assertEqual(module.scan_text(source, "build.rs"), [])

    def test_line_and_column_point_at_predicate(self):
        source = "// header\nfn main() {\n    if cfg!(windows) {}\n}\n"
        findings = module.scan_text(source, "build.rs")
        self.assertEqual(len(findings), 1)
        self.assertEqual(findings[0].line, 3)
        self.assertEqual(findings[0].column, 8)


class ScanRepoTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-build-cfg-")
        self.root = Path(self.tmp.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)

    def tearDown(self):
        self.tmp.cleanup()

    def write(self, relative: str, text: str):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def test_finds_root_and_nested_tracked_scripts(self):
        self.write("build.rs", "fn main() { if cfg!(windows) {} }\n")
        self.write(
            "patches/x/build.rs", 'fn main() { if cfg!(target_env = "msvc") {} }\n'
        )
        subprocess.run(["git", "add", "."], cwd=self.root, check=True)
        findings = module.scan_repo(self.root)
        self.assertEqual([f.path for f in findings], ["build.rs", "patches/x/build.rs"])

    def test_untracked_script_is_ignored(self):
        self.write("build.rs", "fn main() {}\n")
        self.write("untracked/build.rs", "fn main() { if cfg!(windows) {} }\n")
        subprocess.run(["git", "add", "build.rs"], cwd=self.root, check=True)
        self.assertEqual(module.scan_repo(self.root), [])

    def test_tracked_but_deleted_script_is_skipped(self):
        script = self.write("build.rs", "fn main() { if cfg!(windows) {} }\n")
        subprocess.run(["git", "add", "build.rs"], cwd=self.root, check=True)
        script.unlink()
        self.assertEqual(module.scan_repo(self.root), [])

    def test_finding_render_includes_remedy(self):
        finding = module.Finding("build.rs", 1, 1, "cfg!(windows)")
        rendered = finding.render()
        self.assertIn("build.rs:1:1", rendered)
        self.assertIn("CARGO_CFG_TARGET_*", rendered)


if __name__ == "__main__":
    unittest.main()
