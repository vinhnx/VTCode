"""Regression coverage for the maintained Markdown boundary."""

from contextlib import redirect_stdout
import importlib.util
import io
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


spec = importlib.util.spec_from_file_location("check_markdown", Path(__file__).parents[1] / "check_markdown.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class MarkdownSelectionTests(unittest.TestCase):
    def test_only_tracked_maintained_regular_files_are_selected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", directory], check=True)
            (root / "README.md").write_text("# Readme\n")
            (root / "untracked.md").write_text("# Untracked\n")
            (root / "linked.md").symlink_to(root / "untracked.md")
            (root / "loop.md").symlink_to("loop.md")
            subprocess.run(["git", "-C", directory, "add", "README.md", "linked.md", "loop.md"], check=True)
            with mock.patch.object(module, "ROOT", root):
                self.assertEqual(module.tracked_markdown(), ["README.md"])

    def test_maintained_sources_and_instructions_are_selected(self):
        for path in ["README.md", "docs/development/testing.md", "src/AGENTS.md", ".vtcode/README.md"]:
            with self.subTest(path=path):
                self.assertTrue(module.is_maintained_markdown(path))

    def test_symlinked_parent_directory_is_excluded(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            subprocess.run(["git", "init", "-q", str(root)], check=True)
            docs = root / "docs"
            docs.mkdir()
            (docs / "guide.md").write_text("# Original\n")
            (root / "README.md").write_text("# Readme\n")
            subprocess.run(["git", "-C", str(root), "add", "README.md", "docs/guide.md"], check=True)
            with mock.patch.object(module, "ROOT", root):
                self.assertEqual(module.tracked_markdown(), ["README.md", "docs/guide.md"])
                with tempfile.TemporaryDirectory() as external_directory:
                    external = Path(external_directory)
                    (external / "guide.md").write_text("# External\n")
                    docs.rename(root / "original_docs")
                    docs.symlink_to(external, target_is_directory=True)
                    self.assertEqual(module.tracked_markdown(), ["README.md"])
                    self.assertEqual((external / "guide.md").read_text(), "# External\n")

    def test_protected_vendor_and_fixture_files_are_excluded(self):
        for path in [
            "docs/project/TODO.md", "patches/crossterm/README.md",
            "crates/core/src/fixtures/invalid.md", "tests/snapshots/table.md",
            "crates/core/embedded_assets_source/docs/map.md", ".vtcode/reviews/session.md", "Cargo.toml",
        ]:
            with self.subTest(path=path):
                self.assertFalse(module.is_maintained_markdown(path))


class MarkdownInvocationTests(unittest.TestCase):
    def test_linter_exit_status_is_preserved_with_a_bounded_summary(self):
        files = ["README.md", "docs/another guide.md"]
        for exit_code, status in [(0, "passed"), (1, "failed (exit 1)"), (2, "failed (exit 2)")]:
            with self.subTest(exit_code=exit_code), \
                    mock.patch.object(module, "tracked_markdown", return_value=files), \
                    mock.patch.object(sys, "argv", ["check_markdown.py"]), \
                    mock.patch.object(module.subprocess, "run", return_value=subprocess.CompletedProcess([], exit_code)) as run:
                output = io.StringIO()
                with redirect_stdout(output):
                    self.assertEqual(module.main(), exit_code)
                command = run.call_args.args[0]
                self.assertIn("--loglevel=error", command)
                self.assertIn("markdownlint-cli2@0.23.3", command)
                self.assertEqual(command[-2:], [":README.md", ":docs/another guide.md"])
                self.assertEqual(run.call_args.kwargs, {"cwd": module.ROOT, "check": False})
                self.assertEqual(output.getvalue(), f"Markdown lint {status}: 2 selected files.\n")

    def test_list_prints_the_full_inventory_without_running_the_linter(self):
        with mock.patch.object(module, "tracked_markdown", return_value=["README.md", "docs/guide.md"]), \
                mock.patch.object(sys, "argv", ["check_markdown.py", "--list"]), \
                mock.patch.object(module.subprocess, "run") as run:
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(module.main(), 0)
            self.assertEqual(output.getvalue(), "README.md\ndocs/guide.md\n")
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
