"""Regression coverage for the out-of-scope MCP provider docs gate.

The risk is a policy gate that either blocks legitimate work or fails open on
the case it exists for. Each test builds a throwaway git repository so the
tracked-file and added-file discovery run against real git state, and the
provider keys are asymmetric (a grandfathered `sem` vs a new `specstream`) so a
constant answer cannot pass.
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
spec = importlib.util.spec_from_file_location(
    "check_mcp_provider_docs", SCRIPTS / "check_mcp_provider_docs.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


class McpProviderDocsGateTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-mcp-gate-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")

    def tearDown(self):
        self._tmp.cleanup()

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, relative: str, text: str = "content\n") -> None:
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit_all(self, message: str = "baseline") -> None:
        # An empty tree cannot be committed, so always seed a tracked file. It is
        # unrelated to provider docs, which keeps the baseline honest.
        if not any(self.root.iterdir()) or not self._has_staged_or_tracked():
            self.write("README.md", "# fixture\n")
        self.git("add", "-A")
        self.git("commit", "-qm", message)

    def _has_staged_or_tracked(self) -> bool:
        result = subprocess.run(
            ["git", "diff", "--cached", "--name-only"],
            cwd=self.root,
            capture_output=True,
            text=True,
        )
        return bool(result.stdout.strip())

    def run_main(self) -> tuple[int, str]:
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(["--root", str(self.root)])
        return status, stderr.getvalue()

    def test_new_provider_guide_fails(self):
        self.write("docs/guides/sem-mcp.md")
        self.commit_all()
        self.write("docs/guides/specstream-mcp.md")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("docs/guides/specstream-mcp.md", stderr)

    def test_new_provider_example_fails(self):
        self.commit_all()
        self.write("docs/examples/specstream-mcp.toml")
        status, stderr = self.run_main()
        self.assertEqual(status, 1)
        self.assertIn("docs/examples/specstream-mcp.toml", stderr)

    def test_existing_provider_guide_edit_is_allowed(self):
        self.write("docs/guides/sem-mcp.md")
        self.write("docs/examples/sem-mcp.toml")
        self.commit_all()
        self.write("docs/guides/sem-mcp.md", "# improved\n")
        status, stderr = self.run_main()
        self.assertEqual((status, stderr), (0, ""))

    def test_generic_mcp_docs_are_allowed(self):
        self.commit_all()
        self.write("docs/guides/mcp-integration.md")
        self.write("docs/guides/mcp-interop.md")
        self.write("docs/mcp/mcp_integration_guide.md")
        status, stderr = self.run_main()
        self.assertEqual((status, stderr), (0, ""))

    def test_unrelated_guide_is_allowed(self):
        self.commit_all()
        self.write("docs/guides/how-to-write-tests.md")
        status, stderr = self.run_main()
        self.assertEqual((status, stderr), (0, ""))

    def test_staged_scope_sees_only_staged_additions(self):
        self.commit_all()
        self.write("docs/guides/specstream-mcp.md")
        self.git("add", "docs/guides/specstream-mcp.md")
        self.write("docs/guides/other-vendor-mcp.md")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(["--root", str(self.root), "--staged"])
        self.assertEqual(status, 1)
        self.assertIn("specstream", stderr.getvalue())
        self.assertNotIn("other-vendor", stderr.getvalue())

    def test_committed_provider_guide_on_branch_fails(self):
        """The baseline is the base revision, not HEAD, so a branch addition fails."""
        self.write("docs/guides/sem-mcp.md")
        self.commit_all("base")
        self.git("checkout", "-q", "-b", "feature")
        self.write("docs/guides/specstream-mcp.md")
        self.git("add", "-A")
        self.git("commit", "-qm", "add provider")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(["--root", str(self.root), "--base", "main"])
        self.assertEqual(
            status, 1, "a committed branch addition must not grandfather itself"
        )
        self.assertIn("specstream", stderr.getvalue())

    def test_rename_to_new_provider_identity_fails(self):
        """Without --no-renames the destination is reported as R, not A, and is missed."""
        self.write("docs/guides/sem-mcp.md")
        self.commit_all("base")
        self.git("checkout", "-q", "-b", "feature")
        self.git("mv", "docs/guides/sem-mcp.md", "docs/guides/specstream-mcp.md")
        self.git("commit", "-qm", "rename")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(["--root", str(self.root), "--base", "main"])
        self.assertEqual(status, 1)
        self.assertIn("specstream", stderr.getvalue())

    def test_editing_an_existing_provider_guide_on_a_branch_passes(self):
        self.write("docs/guides/sem-mcp.md")
        self.commit_all("base")
        self.git("checkout", "-q", "-b", "feature")
        self.write("docs/guides/sem-mcp.md", "# improved\n")
        self.git("commit", "-qam", "edit")
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            status = module.main(["--root", str(self.root), "--base", "main"])
        self.assertEqual((status, stderr.getvalue()), (0, ""))

    def test_provider_key_ignores_non_provider_paths(self):
        self.assertIsNone(module._provider_key("docs/guides/mcp-integration.md"))
        self.assertIsNone(module._provider_key("docs/guides/sem-mcp-notes.md"))
        self.assertIsNone(module._provider_key("docs/other/sem-mcp.md"))
        self.assertEqual(module._provider_key("docs/guides/sem-mcp.md"), "sem")
        self.assertEqual(module._provider_key("docs/examples/sem-mcp.toml"), "sem")

    def test_non_git_directory_is_an_error(self):
        with tempfile.TemporaryDirectory(prefix="vtcode-mcp-nogit-") as directory:
            stderr = io.StringIO()
            with contextlib.redirect_stderr(stderr):
                status = module.main(["--root", directory])
            self.assertEqual(status, 1)
            self.assertIn("not a git repository", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
