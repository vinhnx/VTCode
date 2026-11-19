"""Scope detection for scripts/coderabbit-review.sh must not invert its meaning.

The reviewed risk is a scope check that reports the wrong answer, because both
mistakes are silent: skipping a review that has work to do, or spending a
CodeRabbit allowance unit on an empty diff. The tests drive real git fixtures so
`--committed` and `--uncommitted` are compared against genuine repository state
rather than a stubbed predicate.
"""

import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "coderabbit-review.sh"


def load_functions(
    directory: Path, scope: str, base: str
) -> subprocess.CompletedProcess:
    """Source the script in a child shell and evaluate the scope predicate.

    The script only runs `main` when executed directly, so sourcing here defines
    the predicates without starting a review.
    """
    snippet = "\n".join(
        [
            "set -eu",
            f"source {shlex.quote(str(SCRIPT))}",
            f"REVIEW_SCOPE={shlex.quote(scope)}",
            f"BASE_REF={shlex.quote(base)}",
            "has_changes",
        ]
    )
    return subprocess.run(
        ["bash", "-c", snippet],
        cwd=directory,
        capture_output=True,
        text=True,
        env={**os.environ, "REVIEW_SCOPE": "", "BASE_REF": ""},
    )


class CoderabbitScopeTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-cr-scope-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.write("a.txt", "one\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "init")
        # A base branch distinct from HEAD makes committed diffs real, so the
        # committed predicate is not trivially comparing HEAD with itself.
        self.git("branch", "base")

    def tearDown(self):
        self._tmp.cleanup()

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, name: str, text: str) -> None:
        (self.root / name).write_text(text, encoding="utf-8")

    def predicate(self, scope: str, base: str = "base") -> int:
        result = load_functions(self.root, scope, base)
        self.assertNotIn("ERROR", result.stdout, result.stdout + result.stderr)
        return result.returncode

    def test_clean_tree_has_nothing_to_review(self):
        self.assertEqual(self.predicate(""), 1)
        self.assertEqual(self.predicate("uncommitted"), 1)
        self.assertEqual(self.predicate("committed"), 1)

    def test_uncommitted_change_only_counts_for_uncommitted_scope(self):
        self.write("a.txt", "two\n")
        self.assertEqual(self.predicate("uncommitted"), 0)
        self.assertEqual(self.predicate("committed"), 1)
        self.assertEqual(self.predicate(""), 0)

    def test_untracked_file_counts_as_uncommitted(self):
        self.write("new.txt", "data\n")
        self.assertEqual(self.predicate("uncommitted"), 0)
        self.assertEqual(self.predicate("committed"), 1)

    def test_committed_change_only_counts_for_committed_scope(self):
        self.write("a.txt", "three\n")
        self.git("commit", "-qam", "change")
        self.assertEqual(self.predicate("committed"), 0)
        self.assertEqual(self.predicate("uncommitted"), 1)
        self.assertEqual(self.predicate(""), 0)

    def test_committed_scope_ignores_dirty_worktree(self):
        """The bug this guards: a HEAD-vs-worktree diff inside committed scope."""
        self.write("a.txt", "four\n")
        self.git("commit", "-qam", "committed change")
        self.write("a.txt", "dirty\n")
        self.assertEqual(self.predicate("committed"), 0)
        self.assertEqual(self.predicate("uncommitted"), 0)

    def test_unresolvable_base_defers_instead_of_skipping(self):
        self.assertEqual(self.predicate("committed", "does-not-exist"), 0)


if __name__ == "__main__":
    unittest.main()
