"""Regression coverage for the tiered Rust file-length governance scan.

Risk-first notes (agentic-testing): the bug-prone surfaces are

- the TOCTOU gap between the `git ls-files` snapshot and file reads (a
  concurrent session can delete or rename a file mid-scan, which used to
  crash the whole Zen step with FileNotFoundError) — pinned fail-open;
- tier classification by path (`tests.rs` / `*_tests.rs` / `tests_*.rs` /
  `tests/`, `benches/`, `fuzz/`, `examples/` vs. lookalike src names);
- boundary off-by-ones on the tier budgets (800/801 src, 1000/1001 tests);
- the ratchet baseline: caps must block growth, and entries whose file
  shrank below the budget, vanished, or demoted to the test tier must go
  stale so the ledger ratchets down instead of accumulating headroom.

Tests pin behavior with literal fixtures (never the production counter as
oracle): each file is written with a literal line count and expectations are
derived from those literals, not from calling the evaluator.
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
spec = importlib.util.spec_from_file_location(
    "check_rust_file_length", SCRIPTS / "check_rust_file_length.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def write_lines(path: Path, count: int) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("content line\n" * count, encoding="utf-8")
    return path


def run_main(files, *argv):
    """Run module.main() against a literal file list; return (code, stdout)."""
    stdout = io.StringIO()
    stderr = io.StringIO()
    with (
        mock.patch.object(module, "list_tracked_rust_files", return_value=files),
        mock.patch.object(sys, "argv", ["check_rust_file_length.py", *argv]),
        contextlib.redirect_stdout(stdout),
        contextlib.redirect_stderr(stderr),
    ):
        code = module.main()
    return code, stdout.getvalue()


class CountLinesTests(unittest.TestCase):
    def test_missing_path_returns_none(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertIsNone(module.count_lines(Path(tmp) / "gone.rs"))

    def test_counts_literal_lines(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = write_lines(Path(tmp) / "a.rs", 7)
            self.assertEqual(module.count_lines(path), 7)


class TierClassificationTests(unittest.TestCase):
    def test_cfg_test_module_filename_conventions_are_test_tier(self):
        for path in (
            "crates/x/src/tests.rs",
            "crates/x/src/plugin_tests.rs",
            "crates/x/src/tests_plugin.rs",
            "crates/x/tests/plugin.rs",
            "crates/x/benches/bench.rs",
            "crates/x/examples/demo.rs",
            "fuzz/fuzz_targets/target.rs",
        ):
            with self.subTest(path=path):
                self.assertTrue(module.is_test_tier(path))

    def test_lookalike_names_stay_src_tier(self):
        for path in (
            "crates/x/src/plugin_test.rs",
            "crates/x/src/test_plugin.rs",
            "crates/x/src/testing.rs",
            "crates/x/test/plugin.rs",
            "crates/x/src/tests.rs.bak",
        ):
            with self.subTest(path=path):
                self.assertFalse(module.is_test_tier(path))

    def test_excluded_and_lookalike_paths(self):
        self.assertTrue(module.is_excluded("patches/crossterm/src/lib.rs"))
        self.assertTrue(module.is_excluded("vendored/generated.rs"))
        self.assertTrue(module.is_excluded("vendored/tables.generated.rs"))
        # Asymmetric lookalikes: singular dir name and prefixed dir stay in budget.
        self.assertFalse(module.is_excluded("patches_extra/src/lib.rs"))
        self.assertFalse(module.is_excluded("crates/x/src/generated_helper.rs"))


class MissingFileRaceTests(unittest.TestCase):
    def test_vanished_file_skipped_without_crashing(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            real = write_lines(root / "src" / "lib.rs", 10)
            ghost = root / "src" / "gone.rs"
            with mock.patch.object(module, "REPO_ROOT", root):
                code, out = run_main([real, ghost], "--mode", "warn")
        self.assertEqual(code, 0, out)
        self.assertIn("skipped_missing=1", out)
        self.assertIn("total=1", out)

    def test_max_lines_boundary(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exact = write_lines(root / "exact.rs", 500)
            over = write_lines(root / "over.rs", 501)
            with mock.patch.object(module, "REPO_ROOT", root):
                code, _ = run_main([exact], "--mode", "enforce", "--max-lines", "500")
                self.assertEqual(code, 0)
                code, _ = run_main([over], "--mode", "enforce", "--max-lines", "500")
                self.assertEqual(code, 1)


class TierBudgetTests(unittest.TestCase):
    """Both sides of each tier boundary, with the default budgets."""

    def run_with_root(self, root, files, *argv):
        with mock.patch.object(module, "REPO_ROOT", root):
            return run_main(files, *argv)

    def test_src_enforce_boundary_800(self):
        # 800 lines is still over the 500 warn threshold (soft signal counts
        # it), but it passes enforce; 801 crosses the enforce budget.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exact = write_lines(root / "src" / "exact.rs", 800)
            over = write_lines(root / "src" / "over.rs", 801)
            code, out = self.run_with_root(
                root, [exact], "--mode", "enforce", "--quiet"
            )
            self.assertEqual(code, 0, out)
            self.assertIn("src_over_budget=0", out)
            code, out = self.run_with_root(root, [over], "--mode", "enforce", "--quiet")
            self.assertEqual(code, 1, out)
            self.assertIn("src_over_budget=1", out)

    def test_src_warn_threshold_500(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exact = write_lines(root / "src" / "exact.rs", 500)
            over = write_lines(root / "src" / "over.rs", 501)
            code, out = self.run_with_root(root, [exact], "--mode", "warn")
            self.assertEqual(code, 0, out)
            self.assertIn("over_warn=0", out)
            code, out = self.run_with_root(root, [over], "--mode", "warn")
            self.assertEqual(code, 0, out)
            self.assertIn("over_warn=1", out)

    def test_test_tier_boundary_1000_and_src_asymmetry(self):
        # Same 900-line count: the test-tier file passes while the src file
        # fails — the tier, not the size, decides the budget.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            test_file = write_lines(root / "crates" / "x" / "src" / "tests.rs", 900)
            code, out = self.run_with_root(root, [test_file], "--mode", "enforce")
            self.assertEqual(code, 0, out)
            self.assertIn("src_capped=0", out)
            src_file = write_lines(root / "crates" / "x" / "src" / "lib.rs", 900)
            code, out = self.run_with_root(root, [src_file], "--mode", "enforce")
            self.assertEqual(code, 1, out)
            self.assertIn("src_over_budget=1", out)

    def test_test_tier_budget_1000(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            exact = write_lines(root / "crates" / "x" / "src" / "tests.rs", 1000)
            over = write_lines(root / "crates" / "x" / "src" / "widget_tests.rs", 1001)
            code, out = self.run_with_root(root, [exact], "--mode", "enforce")
            self.assertEqual(code, 0, out)
            code, out = self.run_with_root(root, [over], "--mode", "enforce")
            self.assertEqual(code, 1, out)
            self.assertIn("test_over_budget=1", out)

    def test_excluded_files_skip_all_budgets(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            vendored = write_lines(root / "patches" / "crossterm" / "lib.rs", 5000)
            generated = write_lines(root / "vendor" / "generated.rs", 5000)
            code, out = self.run_with_root(
                root, [vendored, generated], "--mode", "enforce", "--quiet"
            )
            self.assertEqual(code, 0, out)
            self.assertIn("excluded=2", out)
            self.assertIn("total=2", out)

    def test_warn_mode_never_blocks_any_violation(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            huge = write_lines(root / "src" / "huge.rs", 5000)
            # Ledger entry for a vanished file: stale, yet warn stays exit 0.
            baseline = root / "baseline.txt"
            baseline.write_text("src/gone.rs=900\n", encoding="utf-8")
            code, out = self.run_with_root(
                root, [huge], "--mode", "warn", "--baseline", str(baseline)
            )
            self.assertEqual(code, 0, out)


class BaselineRatchetTests(unittest.TestCase):
    """Caps block growth; shrink/demotion/renames make entries stale."""

    def run_with_baseline(self, root, files, ledger_text, *argv):
        baseline = root / "baseline.txt"
        baseline.write_text(ledger_text, encoding="utf-8")
        with mock.patch.object(module, "REPO_ROOT", root):
            return run_main(files, "--mode", "enforce", "--baseline", str(baseline), *argv)

    def test_cap_at_exact_size_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            big = write_lines(root / "src" / "big.rs", 900)
            code, out = self.run_with_baseline(root, [big], "src/big.rs=900\n")
            self.assertEqual(code, 0, out)
            self.assertIn("src_capped=1", out)

    def test_growth_past_cap_fails_distinctly_from_missing_cover(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            grown = write_lines(root / "src" / "big.rs", 901)
            code, out = self.run_with_baseline(root, [grown], "src/big.rs=900\n")
            self.assertEqual(code, 1, out)
            self.assertIn("past_cap=1", out)
            self.assertIn("grown past their ratchet cap", out)
            self.assertIn("| 901 | 900 | src/big.rs |", out)
            self.assertNotIn("no baseline cover", out)

    def test_shrunken_file_makes_entry_stale(self):
        # The file dropped below 800; the 900 cap must fail as stale so the
        # ledger ratchets down instead of keeping headroom forever.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            shrunk = write_lines(root / "src" / "big.rs", 700)
            code, out = self.run_with_baseline(root, [shrunk], "src/big.rs=900\n")
            self.assertEqual(code, 1, out)
            self.assertIn("stale_baseline=1", out)
            self.assertIn("Stale baseline entries", out)
            self.assertIn("- src/big.rs", out)

    def test_entry_for_missing_file_is_stale(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            other = write_lines(root / "src" / "other.rs", 10)
            code, out = self.run_with_baseline(root, [other], "src/gone.rs=900\n")
            self.assertEqual(code, 1, out)
            self.assertIn("stale_baseline=1", out)

    def test_entry_for_test_tier_file_is_stale(self):
        # Test-tier files are never baselined: the file exists but its entry
        # covers no src-tier file, so the entry is dead weight.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            test_file = write_lines(root / "crates" / "x" / "src" / "tests.rs", 900)
            code, out = self.run_with_baseline(
                root, [test_file], "crates/x/src/tests.rs=900\n"
            )
            self.assertEqual(code, 1, out)
            self.assertIn("stale_baseline=1", out)

    def test_glob_entry_covers_and_ratchets_tightest_cap(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            big = write_lines(root / "src" / "debt" / "big.rs", 900)
            code, out = self.run_with_baseline(
                root, [big], "src/debt/*.rs=900\nsrc/debt/big.rs=850\n"
            )
            # The tightest matching cap (850) is the binding constraint.
            self.assertEqual(code, 1, out)
            self.assertIn("past_cap=1", out)

    def test_emit_baseline_lists_all_over_budget_src_sorted(self):
        # Uncovered, capped, and past-cap src files all appear with their
        # current sizes, path-sorted; test-tier and excluded files never do.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            uncovered = write_lines(root / "src" / "uncovered.rs", 950)
            capped = write_lines(root / "src" / "capped.rs", 900)
            grown = write_lines(root / "src" / "grown.rs", 910)
            test_file = write_lines(root / "crates" / "x" / "src" / "tests.rs", 900)
            vendored = write_lines(root / "patches" / "v" / "lib.rs", 900)
            baseline = root / "baseline.txt"
            baseline.write_text(
                "src/capped.rs=900\nsrc/grown.rs=900\n", encoding="utf-8"
            )
            with mock.patch.object(module, "REPO_ROOT", root):
                code, out = run_main(
                    [uncovered, capped, grown, test_file, vendored],
                    "--mode",
                    "enforce",
                    "--emit-baseline",
                    "--baseline",
                    str(baseline),
                )
            self.assertEqual(code, 0)
            self.assertEqual(
                out,
                "src/capped.rs=900\nsrc/grown.rs=910\nsrc/uncovered.rs=950\n",
            )

    def test_duplicate_ledger_entry_fails_fast(self):
        # A duplicate with a higher cap would silently raise the cap via
        # last-wins dict semantics and break the ratchet-down invariant.
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = write_lines(root / "src" / "lib.rs", 10)
            code, _ = self.run_with_baseline(
                root, [src], "src/lib.rs=850\nsrc/lib.rs=900\n"
            )
            self.assertEqual(code, 2)

    def test_malformed_ledger_fails_fast_without_scanning(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            src = write_lines(root / "src" / "lib.rs", 10)
            for bad in ("src/lib.rs\n", "src/lib.rs=abc\n", "src/lib.rs=\n"):
                with self.subTest(entry=bad.strip()):
                    code, _ = self.run_with_baseline(root, [src], bad)
                    self.assertEqual(code, 2)


if __name__ == "__main__":
    unittest.main()
