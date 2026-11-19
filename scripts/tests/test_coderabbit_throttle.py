"""Throttle and grace-period behavior for scripts/coderabbit-review.sh.

Two failure modes matter, and both are silent:

- A throttle keyed on time alone serves a "passed" verdict for code that has
  since changed, so a real finding is hidden behind a stale cache.
- A grace period that leaks past enforcement, or that swallows an *incomplete*
  review, turns "not evaluated" into "clean".

The tests drive the sourced script against real git fixtures so the diff
fingerprint reflects genuine repository state.
"""

import os
import shutil
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / "coderabbit-review.sh"


def run(
    directory: Path, body: str, env: dict | None = None
) -> subprocess.CompletedProcess:
    """Source the script (which does not run main) and evaluate `body`."""
    merged = {**os.environ, "REVIEW_SCOPE": "", "BASE_REF": "", **(env or {})}
    snippet = "\n".join(["set -eu", f"source {shlex.quote(str(SCRIPT))}", body])
    return subprocess.run(
        ["bash", "-c", snippet],
        cwd=directory,
        capture_output=True,
        text=True,
        env=merged,
    )


class DiffKeyTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-cr-key-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.write("a.txt", "one\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "init")

    def tearDown(self):
        self._tmp.cleanup()

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, name: str, text: str) -> None:
        (self.root / name).write_text(text, encoding="utf-8")

    def key(self, scope: str = "uncommitted", gate: str = "major") -> str:
        result = run(self.root, f"REVIEW_SCOPE={scope}\nGATE_SEVERITY={gate}\ndiff_key")
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout.strip()

    def test_same_tree_yields_the_same_key(self):
        self.assertEqual(self.key(), self.key())

    def test_editing_a_tracked_file_changes_the_key(self):
        first = self.key()
        self.write("a.txt", "two\n")
        self.assertNotEqual(first, self.key())

    def test_untracked_file_changes_the_key(self):
        first = self.key()
        self.write("new.txt", "data\n")
        self.assertNotEqual(first, self.key())

    def test_stricter_gate_is_a_different_question(self):
        self.write("a.txt", "two\n")
        self.assertNotEqual(self.key(gate="major"), self.key(gate="minor"))


class ThrottleTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-cr-throttle-")
        self.root = Path(self._tmp.name)
        self.git("init", "-q")
        self.git("config", "user.email", "test@example.com")
        self.git("config", "user.name", "Test")
        self.write("a.txt", "one\n")
        self.git("add", "-A")
        self.git("commit", "-qm", "init")
        # Keep state outside the worktree: a state file inside the repo appears in
        # `git status`, which is part of the diff fingerprint, so the record would
        # invalidate its own key. The default `.git/coderabbit-review` is safe for
        # the same reason.
        self.state = Path(tempfile.mkdtemp(prefix="vtcode-cr-state-"))
        self.write("a.txt", "two\n")

    def tearDown(self):
        self._tmp.cleanup()
        shutil.rmtree(self.state, ignore_errors=True)

    def git(self, *args: str) -> None:
        subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, name: str, text: str) -> None:
        (self.root / name).write_text(text, encoding="utf-8")

    def env(self, **overrides) -> dict:
        # The script reads CODERABBIT_* at source time. Setting the bare shell
        # names instead is silently overwritten by those defaults, which would
        # let a test pass against the default state dir.
        base = {
            "CODERABBIT_STATE_DIR": str(self.state),
            "CODERABBIT_THROTTLE_SECONDS": "3600",
        }
        base.update(overrides)
        return base

    def evaluate(self, body: str, **overrides) -> subprocess.CompletedProcess:
        return run(self.root, body, env=self.env(**overrides))

    def record(self, status: int = 2, **overrides) -> None:
        result = self.evaluate(
            'REVIEW_SCOPE=uncommitted\nkey=$(diff_key)\nrecord_review "$key" '
            + str(status),
            **overrides,
        )
        self.assertEqual(result.returncode, 0, result.stderr)

    def window(self, **overrides) -> subprocess.CompletedProcess:
        return self.evaluate(
            'REVIEW_SCOPE=uncommitted\nkey=$(diff_key)\nwithin_throttle_window "$key"',
            **overrides,
        )

    def test_no_state_means_not_throttled(self):
        self.assertNotEqual(self.window().returncode, 0)

    def test_recorded_diff_is_throttled(self):
        self.record()
        self.assertEqual(self.window().returncode, 0)

    def test_state_lands_in_the_configured_directory(self):
        """Guards against the override being ignored and polluting .git."""
        self.record()
        self.assertTrue(
            (self.state / "last-run").is_file(),
            "state must land in CODERABBIT_STATE_DIR, not the default location",
        )

    def test_changed_diff_is_not_throttled(self):
        self.record()
        self.write("a.txt", "three\n")
        self.assertNotEqual(
            self.window().returncode, 0, "a changed diff must not reuse a verdict"
        )

    def test_window_expiry_releases_the_throttle(self):
        self.record()
        # A zero window means every recorded run is already outside it.
        self.assertNotEqual(self.window(CODERABBIT_THROTTLE_SECONDS="0").returncode, 0)

    def test_cached_status_round_trips(self):
        self.record(status=2)
        result = self.evaluate(
            'REVIEW_SCOPE=uncommitted\nkey=$(diff_key)\ncached_status "$key"'
        )
        self.assertEqual(result.stdout.strip(), "2", result.stderr)

    def test_cached_status_for_a_different_diff_is_unknown(self):
        self.record(status=0)
        self.write("a.txt", "four\n")
        result = self.evaluate(
            'REVIEW_SCOPE=uncommitted\nkey=$(diff_key)\ncached_status "$key"'
        )
        self.assertEqual(
            result.stdout.strip(), "1", "a stale key must not report a pass"
        )


class GracePeriodTests(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory(prefix="vtcode-cr-grace-")

    def tearDown(self):
        self._tmp.cleanup()

    def status_of(self, review_status: int, grace: str) -> int:
        body = "\n".join(
            [
                f"GRACE_UNTIL={shlex.quote(grace)}",
                f"apply_grace {review_status} fresh",
            ]
        )
        return run(Path(self._tmp.name), body).returncode

    def test_past_deadline_enforces_a_block(self):
        self.assertEqual(self.status_of(2, "2000-01-01T00:00:00Z"), 2)

    def test_future_deadline_downgrades_a_block_to_a_warning(self):
        self.assertEqual(self.status_of(2, "2999-01-01T00:00:00Z"), 0)

    def test_empty_deadline_enforces_now(self):
        """An expired/unset grace must enforce; warn-forever would disable the gate."""
        self.assertEqual(self.status_of(2, ""), 2)

    def test_pass_stays_a_pass(self):
        self.assertEqual(self.status_of(0, "2000-01-01T00:00:00Z"), 0)

    def test_incomplete_is_never_downgraded(self):
        """An unevaluated review is not evidence of cleanliness, even in grace."""
        self.assertEqual(self.status_of(1, "2999-01-01T00:00:00Z"), 1)


if __name__ == "__main__":
    unittest.main()
