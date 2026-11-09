"""Regression coverage for the docs/AGENTS.md link and line-budget gate.

Each test builds a throwaway git repository so tracked-file discovery runs for real:
- a broken link in a tracked crate `AGENTS.md` fails, and the same file untracked is ignored;
- a broken link in a maintainer note fails;
- a module `AGENTS.md` line of exactly the budget passes while budget + 1 fails;
- the root `AGENTS.md` is exempt from the per-line budget.
"""

import contextlib
import importlib.util
import io
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("check_docs_links", SCRIPTS / "check_docs_links.py")
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)

BUDGET = module.MAX_MODULE_AGENTS_LINE_CHARS


class CheckDocsLinksTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-docs-links-")
        self.root = Path(self._tmp.name)
        subprocess.run(["git", "init", "-q"], cwd=self.root, check=True)
        for doc in module.ENTRYPOINT_DOCS:
            self.write(doc, "# entrypoint\n")
        self.track(*module.ENTRYPOINT_DOCS)

    def tearDown(self):
        self._tmp.cleanup()

    def write(self, relative: str, text: str) -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def track(self, *relative: str) -> None:
        subprocess.run(["git", "add", "--", *relative], cwd=self.root, check=True)

    def run_main(self) -> tuple[int, str]:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(self.root)
        return status, stderr.getvalue()

    def test_clean_fixture_passes(self):
        self.write("crates/a/AGENTS.md", "[Root](../../AGENTS.md)\n")
        self.track("crates/a/AGENTS.md")
        self.assertEqual(self.run_main(), (0, ""))

    def test_broken_link_in_tracked_crate_agents_fails(self):
        self.write("crates/a/AGENTS.md", "[Root](../AGENTS.md)\n")
        self.track("crates/a/AGENTS.md")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("crates/a/AGENTS.md -> ../AGENTS.md", stderr)

    def test_untracked_agents_file_is_ignored(self):
        self.write("crates/a/AGENTS.md", "[Root](../AGENTS.md)\n")
        self.assertEqual(self.run_main(), (0, ""))

    def test_broken_link_in_maintainer_note_fails(self):
        self.write(f"{module.MAINTAINER_NOTES_DIR}/a.md", "[Gone](missing.md)\n")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("maintainer-notes/a.md -> missing.md", stderr)

    def test_external_and_anchor_links_are_skipped(self):
        self.write("crates/a/AGENTS.md", "[x](https://example.com) [y](#rules) [z](mailto:a@b.c)\n")
        self.track("crates/a/AGENTS.md")
        self.assertEqual(self.run_main(), (0, ""))

    def test_module_line_budget_boundary(self):
        self.write("crates/a/AGENTS.md", "x" * BUDGET + "\n")
        self.track("crates/a/AGENTS.md")
        self.assertEqual(self.run_main(), (0, ""))

        self.write("crates/a/AGENTS.md", "short\n" + "x" * (BUDGET + 1) + "\n")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn(f"crates/a/AGENTS.md:2 ({BUDGET + 1} chars)", stderr)

    def test_root_agents_is_exempt_from_line_budget(self):
        self.write("AGENTS.md", "x" * (BUDGET + 50) + "\n")
        self.assertEqual(self.run_main(), (0, ""))

    def test_broken_link_in_any_maintained_doc_fails(self):
        self.write("docs/guides/a.md", "[Gone](missing.md)\n")
        self.track("docs/guides/a.md")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("docs/guides/a.md -> missing.md", stderr)

    def test_vendored_markdown_is_not_checked(self):
        self.write("patches/lib/README.md", "[Gone](missing.md)\n")
        self.track("patches/lib/README.md")
        self.assertEqual(self.run_main(), (0, ""))

    def test_links_inside_code_are_not_checked(self):
        self.write(
            "docs/a.md",
            "Use `[x](inline.md)` syntax.\n\n```markdown\nSee [FORMS.md](FORMS.md)\n```\n\n"
            "~~~\n[y](tilde.md)\n~~~\n",
        )
        self.track("docs/a.md")
        self.assertEqual(self.run_main(), (0, ""))

    def test_link_after_a_closed_fence_is_checked(self):
        self.write("docs/a.md", "```\n[x](skipped.md)\n```\n\n[Gone](after.md)\n")
        self.track("docs/a.md")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("docs/a.md -> after.md", stderr)
        self.assertNotIn("skipped.md", stderr)

    def write_budget(self, expression: str) -> None:
        self.write(module.PROMPT_BUDGET_SOURCE, f"pub const DEFAULT_MAX_BYTES: usize = {expression};\n")

    def test_instruction_budget_parses_the_rust_constant(self):
        self.write_budget("16 * 1_024")
        self.assertEqual(module.instruction_budget(self.root), 16384)

    def test_root_plus_largest_module_agents_budget_boundary(self):
        self.write_budget("2 * 50")
        self.write("AGENTS.md", "r" * 59 + "\n")
        self.write("crates/a/AGENTS.md", "a" * 19 + "\n")
        self.write("crates/b/AGENTS.md", "b" * 39 + "\n")
        self.track("AGENTS.md", "crates/a/AGENTS.md", "crates/b/AGENTS.md")
        self.assertEqual(self.run_main(), (0, ""))

        self.write("crates/b/AGENTS.md", "b" * 40 + "\n")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("plus crates/b/AGENTS.md need 101 bytes", stderr)

    def test_missing_entrypoint_fails(self):
        (self.root / module.ENTRYPOINT_DOCS[-1]).unlink()
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn(module.ENTRYPOINT_DOCS[-1], stderr)


if __name__ == "__main__":
    unittest.main()
