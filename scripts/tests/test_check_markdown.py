"""Regression coverage for the maintained Markdown boundary."""

from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
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
        for path in [
            "README.md", "docs/development/testing.md", "src/AGENTS.md", ".vtcode/README.md",
            ".agents/skills/agentic-testing/SKILL.md",
        ]:
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
            "docs/project/TODO.md", "patches/crossterm/README.md", ".agents/skills/rust-skills/rules/own-borrow.md",
            "crates/core/src/fixtures/invalid.md", "tests/snapshots/table.md",
            "crates/core/embedded_assets_source/docs/map.md", ".vtcode/reviews/session.md", "Cargo.toml",
        ]:
            with self.subTest(path=path):
                self.assertFalse(module.is_maintained_markdown(path))


class MarkdownInvocationTests(unittest.TestCase):
    def test_linter_exit_status_is_preserved_with_a_bounded_summary(self):
        files = ["README.md", "docs/another guide.md"]
        base = ["npx", "--no-install", "--loglevel=error", "markdownlint-cli2@0.23.3",
                "--config", ".markdownlint-cli2.jsonc"]
        for exit_code, status in [(0, "passed"), (1, "failed (exit 1)"), (2, "failed (exit 2)")]:
            with self.subTest(exit_code=exit_code), \
                    mock.patch.object(module, "tracked_markdown", return_value=files), \
                    mock.patch.object(module, "linter_command", return_value=list(base)), \
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

    def test_missing_linter_binary_reports_a_toolchain_error_with_exit_two(self):
        base = ["npx", "--no-install", "--loglevel=error", "markdownlint-cli2@0.23.3",
                "--config", ".markdownlint-cli2.jsonc"]
        with mock.patch.object(module, "tracked_markdown", return_value=["README.md"]), \
                mock.patch.object(module, "linter_command", return_value=list(base)), \
                mock.patch.object(sys, "argv", ["check_markdown.py"]), \
                mock.patch.object(module.subprocess, "run",
                                  side_effect=FileNotFoundError(2, "No such file or directory", "npx")):
            stdout, stderr = io.StringIO(), io.StringIO()
            with redirect_stdout(stdout), redirect_stderr(stderr):
                self.assertEqual(module.main(), 2)
            self.assertIn("'npx' was not found on PATH", stderr.getvalue())
            self.assertIn("Install Node.js 22", stderr.getvalue())
            self.assertIn("failed (exit 2)", stdout.getvalue())

    def test_scoped_paths_lint_only_the_requested_files_in_requested_order(self):
        files = ["README.md", "docs/guide.md", "docs/other.md"]
        base = ["npx", "--no-install", "--loglevel=error", "markdownlint-cli2@0.23.3",
                "--config", ".markdownlint-cli2.jsonc"]
        with mock.patch.object(module, "tracked_markdown", return_value=files), \
                mock.patch.object(module, "linter_command", return_value=list(base)), \
                mock.patch.object(sys, "argv", ["check_markdown.py", "docs/guide.md", "./README.md",
                                                "docs/guide.md"]), \
                mock.patch.object(module.subprocess, "run",
                                  return_value=subprocess.CompletedProcess([], 0)) as run:
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(module.main(), 0)
            command = run.call_args.args[0]
            self.assertEqual(command[-2:], [":docs/guide.md", ":README.md"])
            self.assertEqual(output.getvalue(), "Markdown lint passed: 2 selected files.\n")

    def test_scoped_path_outside_the_maintained_selection_fails_closed(self):
        for requested in ["docs/missing.md", "docs/project/TODO.md", "Cargo.toml"]:
            with self.subTest(requested=requested), \
                    mock.patch.object(module, "tracked_markdown", return_value=["README.md"]), \
                    mock.patch.object(sys, "argv", ["check_markdown.py", requested]), \
                    mock.patch.object(module.subprocess, "run") as run:
                stderr = io.StringIO()
                with redirect_stderr(stderr):
                    with self.assertRaises(SystemExit) as raised:
                        module.main()
                self.assertEqual(raised.exception.code, 2)
                self.assertIn("is not a tracked, maintained Markdown file", stderr.getvalue())
                run.assert_not_called()

    def test_list_respects_path_scope(self):
        with mock.patch.object(module, "tracked_markdown", return_value=["README.md", "docs/guide.md"]), \
                mock.patch.object(sys, "argv", ["check_markdown.py", "--list", "docs/guide.md"]), \
                mock.patch.object(module.subprocess, "run") as run:
            output = io.StringIO()
            with redirect_stdout(output):
                self.assertEqual(module.main(), 0)
            self.assertEqual(output.getvalue(), "docs/guide.md\n")
            run.assert_not_called()


class LinterCommandTests(unittest.TestCase):
    def test_local_install_uses_no_install_with_the_pinned_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package = root / "node_modules" / "markdownlint-cli2" / "package.json"
            package.parent.mkdir(parents=True)
            package.write_text('{"name": "markdownlint-cli2", "version": "0.23.3"}\n')
            with mock.patch.object(module, "ROOT", root):
                self.assertEqual(module.linter_command(), [
                    "npx", "--no-install", "--loglevel=error",
                    "markdownlint-cli2@0.23.3", "--config", ".markdownlint-cli2.jsonc",
                ])

    def test_missing_local_install_falls_back_to_a_pinned_fetch(self):
        expected = [
            "npx", "--yes", "--loglevel=error",
            "markdownlint-cli2@0.23.3", "--config", ".markdownlint-cli2.jsonc",
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with mock.patch.object(module, "ROOT", root):
                with self.subTest(case="no node_modules"):
                    self.assertEqual(module.linter_command(), expected)
                (root / "node_modules" / "markdownlint-cli2").mkdir(parents=True)
                with self.subTest(case="partial install without package.json"):
                    self.assertEqual(module.linter_command(), expected)

    def test_pinned_version_matches_package_json(self):
        package_json = Path(module.__file__).parents[1] / "package.json"
        dev_dependencies = json.loads(package_json.read_text())["devDependencies"]
        self.assertEqual(dev_dependencies["markdownlint-cli2"], module.MARKDOWNLINT_CLI2_VERSION)


if __name__ == "__main__":
    unittest.main()
