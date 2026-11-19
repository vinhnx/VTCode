"""Fast-path correctness for scripts/check-dev.sh.

The risk is a cache that lets a real regression through. Two failure modes:

- A key that ignores working-tree edits serves a stale "passing" verdict for code
  that changed since the last run.
- A key that ignores scope reuses a default-members verdict for a workspace run.

The tests drive the sourced script against real git fixtures, so the fingerprint
reflects genuine repository state.
"""

import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "check-dev.sh"


def run(
    directory: Path, body: str, env: dict | None = None
) -> subprocess.CompletedProcess:
    merged = {**os.environ, **(env or {})}
    snippet = "\n".join(["set -eu", f"source {shlex.quote(str(SCRIPT))}", body])
    return subprocess.run(
        ["bash", "-c", snippet],
        cwd=directory,
        capture_output=True,
        text=True,
        env=merged,
    )


class RustStateKeyTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-checkdev-key-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.write("src/main.rs", "fn main() {}\n")
        self.write("Cargo.toml", '[package]\nname = "x"\n')
        self.git("add", "-A")
        self.git("commit", "-qm", "init")

    def tearDown(self):
        self._tmp.cleanup()

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, name: str, text: str) -> None:
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def key(self, scope: str = "default-members") -> str:
        result = run(
            self.root,
            f"SCOPE_LABEL={scope}\nrust_state_key",
            env={"RUST_STATE_FILE": str(self.root / ".state")},
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_same_tree_yields_the_same_key(self):
        self.assertEqual(self.key(), self.key())

    def test_committed_rust_change_changes_the_key(self):
        first = self.key()
        self.write("src/main.rs", 'fn main() { println!("x"); }\n')
        self.git("commit", "-qam", "change")
        self.assertNotEqual(first, self.key())

    def test_uncommitted_rust_edit_changes_the_key(self):
        first = self.key()
        self.write("src/main.rs", "fn main() { /* dirty */ }\n")
        self.assertNotEqual(
            first, self.key(), "a working-tree edit must invalidate the fast path"
        )

    def test_new_untracked_rust_file_changes_the_key(self):
        first = self.key()
        self.write("src/extra.rs", "pub fn x() {}\n")
        self.assertNotEqual(first, self.key())

    def test_manifest_change_changes_the_key(self):
        first = self.key()
        self.write("Cargo.toml", '[package]\nname = "x"\nversion = "2"\n')
        self.assertNotEqual(first, self.key())

    def test_scope_change_changes_the_key(self):
        self.assertNotEqual(self.key("default-members"), self.key("workspace"))

    def test_non_rust_change_does_not_change_the_key(self):
        """A docs-only edit must not force a cargo rerun."""
        first = self.key()
        self.write("README.md", "# docs\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "docs")
        self.assertEqual(first, self.key())


class FastPathCacheTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-checkdev-cache-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.write("src/main.rs", "fn main() {}\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "init")
        # Keep the state file outside the worktree so it cannot pollute the key.
        self.state = Path(tempfile.mkdtemp(prefix="vtcode-checkdev-state-"))

    def tearDown(self):
        self._tmp.cleanup()
        shutil.rmtree(self.state, ignore_errors=True)

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, name: str, text: str) -> None:
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def evaluate(self, body: str) -> subprocess.CompletedProcess:
        return run(
            self.root,
            body,
            env={"RUST_STATE_FILE": str(self.state / "rust-state")},
        )

    def seed(self) -> None:
        result = self.evaluate("SCOPE_LABEL=default-members\nrecord_rust_state")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_unseeded_cache_reports_changed(self):
        result = self.evaluate("SCOPE_LABEL=default-members\nrust_inputs_unchanged")
        self.assertNotEqual(result.returncode, 0)

    def test_seeded_cache_reports_unchanged(self):
        self.seed()
        result = self.evaluate("SCOPE_LABEL=default-members\nrust_inputs_unchanged")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_edit_after_seed_reports_changed(self):
        self.seed()
        self.write("src/main.rs", "fn main() { /* changed */ }\n")
        result = self.evaluate("SCOPE_LABEL=default-members\nrust_inputs_unchanged")
        self.assertNotEqual(
            result.returncode, 0, "the fast path must not hide a working-tree edit"
        )

    def test_state_lands_in_the_configured_file(self):
        self.seed()
        self.assertTrue(
            (self.state / "rust-state").is_file(),
            "state must land in RUST_STATE_FILE, not the default location",
        )

    def test_default_state_path_resolves_in_a_linked_worktree(self):
        """In a worktree `.git` is a file, so `.git/<state>` would fail to create."""
        worktree = self.root / "linked"
        self.git("commit", "--allow-empty", "-qm", "base")
        self.git("worktree", "add", "-q", str(worktree), "-b", "linked-branch")
        result = run(
            worktree,
            "GIT_DIR_PATH=$(git rev-parse --git-dir)\n"
            'mkdir -p "$GIT_DIR_PATH" 2>/dev/null\n'
            'printf "probe\\n" > "${GIT_DIR_PATH}/check-dev-rust-state" && echo WROTE',
        )
        self.assertIn("WROTE", result.stdout + result.stderr)


class GateOrderTests(unittest.TestCase):
    def test_cheap_gates_are_ordered_before_cargo(self):
        """A fast gate should surface a sub-second failure before paying for clippy."""
        source = SCRIPT.read_text(encoding="utf-8")
        main_start = source.index("main() {")
        body = source[main_start:]
        shell_lint = body.index("run_shell_scripts_lint ||")
        governance = body.index("run_repo_governance_gates ||")
        clippy = body.index("run_clippy ||")
        self.assertLess(shell_lint, clippy, "shell lint must run before clippy")
        self.assertLess(governance, clippy, "governance gates must run before clippy")

    def test_shell_lint_is_not_duplicated(self):
        source = SCRIPT.read_text(encoding="utf-8")
        body = source[source.index("main() {") :]
        self.assertEqual(
            body.count("run_shell_scripts_lint ||"),
            1,
            "shell lint must run exactly once per invocation",
        )


if __name__ == "__main__":
    unittest.main()
