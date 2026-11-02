"""Regression coverage for the 1000-line hard-ceiling gate.

Risk-first notes (agentic-testing): the bug-prone surface here is boundary
semantics, not happy paths. These tests pin both sides of every boundary with
literal fixtures (`"x\\n" * N`, never the production counter as oracle):

- exactly `max_lines` passes while `max_lines + 1` fails;
- a cap allows `cap` but denies `cap + 1`, regardless of rule order;
- `patches/**` matches nested paths but not sibling directories;
- binary skip triggers on extension and on content, while plain text counts;
- stale detection fires only when a rule matches no tracked path at all.
"""

import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

SCRIPTS = Path(__file__).resolve().parents[1]
REPO_ROOT = SCRIPTS.parent
spec = importlib.util.spec_from_file_location(
    "check_file_length", SCRIPTS / "check_file_length.py"
)
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def write_lines(path: Path, count: int, *, trailing_newline: bool = True) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    text = "content line\n" * count
    if count and not trailing_newline:
        text = text[:-1]
    path.write_text(text, encoding="utf-8")
    return path


class ParseAllowRuleTests(unittest.TestCase):
    def test_plain_pattern_has_no_cap(self):
        rule = module.parse_allow_rule("Cargo.lock")
        self.assertEqual((rule.pattern, rule.max_lines), ("Cargo.lock", None))

    def test_cap_accepts_underscores(self):
        rule = module.parse_allow_rule("docs/big.md=2_500")
        self.assertEqual((rule.pattern, rule.max_lines), ("docs/big.md", 2500))

    def test_blank_entry_rejected(self):
        with self.assertRaises(ValueError):
            module.parse_allow_rule("   ")

    def test_missing_pattern_rejected(self):
        with self.assertRaises(ValueError):
            module.parse_allow_rule("=500")

    def test_missing_limit_rejected(self):
        with self.assertRaises(ValueError):
            module.parse_allow_rule("docs/big.md=")

    def test_non_numeric_limit_rejected(self):
        with self.assertRaises(ValueError):
            module.parse_allow_rule("docs/big.md=many")


class MatchingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-file-length-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_exact_and_nested_glob_match(self):
        nested = self.root / "patches" / "a" / "b.rs"
        nested.parent.mkdir(parents=True)
        nested.touch()
        rules = [module.AllowRule(pattern="patches/**")]
        self.assertTrue(module.is_allowed(nested, 10_000, rules, repo_root=self.root))

    def test_sibling_directory_does_not_match(self):
        sibling = self.root / "patches-old" / "x.rs"
        sibling.parent.mkdir(parents=True)
        sibling.touch()
        rules = [module.AllowRule(pattern="patches/**")]
        self.assertFalse(module.is_allowed(sibling, 10_001, rules, repo_root=self.root))

    def test_cap_boundary_allows_cap_denies_cap_plus_one(self):
        target = write_lines(self.root / "src" / "big.rs", 1200)
        at_cap = [module.AllowRule(pattern="src/big.rs", max_lines=1200)]
        below_cap = [module.AllowRule(pattern="src/big.rs", max_lines=1199)]
        self.assertTrue(module.is_allowed(target, 1200, at_cap, repo_root=self.root))
        self.assertFalse(module.is_allowed(target, 1200, below_cap, repo_root=self.root))

    def test_any_matching_rule_wins_regardless_of_order(self):
        target = write_lines(self.root / "src" / "big.rs", 1200)
        tight = module.AllowRule(pattern="src/*.rs", max_lines=1100)
        roomy = module.AllowRule(pattern="src/big.rs", max_lines=1300)
        for rules in ([tight, roomy], [roomy, tight]):
            with self.subTest(rules=[r.pattern for r in rules]):
                self.assertTrue(
                    module.is_allowed(target, 1200, rules, repo_root=self.root)
                )

    def test_dot_slash_and_backslash_prefixes_normalize(self):
        target = write_lines(self.root / "src" / "big.rs", 1200)
        for pattern in ("./src/big.rs", "src\\big.rs"):
            with self.subTest(pattern=pattern):
                rules = [module.AllowRule(pattern=pattern, max_lines=1200)]
                self.assertTrue(
                    module.is_allowed(target, 1200, rules, repo_root=self.root)
                )


class LineCountTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-file-length-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_trailing_newline_does_not_change_count(self):
        with_newline = write_lines(self.root / "a.txt", 1000, trailing_newline=True)
        without_newline = write_lines(self.root / "b.txt", 1000, trailing_newline=False)
        self.assertEqual(module.count_lines(with_newline), 1000)
        self.assertEqual(module.count_lines(without_newline), 1000)

    def test_empty_file_counts_zero(self):
        empty = self.root / "empty.txt"
        empty.write_text("", encoding="utf-8")
        self.assertEqual(module.count_lines(empty), 0)


class BinaryDetectionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-file-length-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_null_byte_in_sniff_window_detected(self):
        blob = self.root / "blob.dat"
        blob.write_bytes(b"GIF89a" + b"\x00" * 8 + b"x" * 100)
        self.assertTrue(module.looks_binary(blob))

    def test_plain_text_not_detected(self):
        text = write_lines(self.root / "notes.txt", 50)
        self.assertFalse(module.looks_binary(text))

    def test_media_extension_skipped_regardless_of_content(self):
        fake_png = self.root / "shot.png"
        fake_png.write_text("not really an image\n", encoding="utf-8")
        self.assertTrue(module.is_binary_path(fake_png))
        self.assertFalse(module.is_binary_path(self.root / "shot.txt"))

    def test_null_byte_past_sniff_window_counts_as_text(self):
        # Documented trade-off: only the first SNIFF_BYTES are inspected, like
        # grep. A late NUL does not flip a text file to binary.
        late = self.root / "late.bin"
        late.write_bytes(b"a" * (module.SNIFF_BYTES + 16) + b"\x00")
        self.assertFalse(module.looks_binary(late))


class EvaluateTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-file-length-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def evaluate(self, files, rules=(), *, max_lines=1000):
        return module.evaluate(files, list(rules), max_lines=max_lines, repo_root=self.root)

    def test_exactly_at_limit_passes_one_over_fails(self):
        at_limit = write_lines(self.root / "at.txt", 1000)
        one_over = write_lines(self.root / "over.txt", 1001)
        result = self.evaluate([at_limit, one_over])
        self.assertEqual([item.path for item in result.over_unallowed], ["over.txt"])
        self.assertEqual(result.over_allowed, [])
        self.assertEqual(result.scanned_text, 2)

    def test_allowed_files_land_in_over_allowed(self):
        big = write_lines(self.root / "gen" / "out.txt", 1500)
        rules = [module.AllowRule(pattern="gen/**", max_lines=2000)]
        result = self.evaluate([big], rules)
        self.assertEqual(result.over_unallowed, [])
        self.assertEqual([item.lines for item in result.over_allowed], [1500])

    def test_entries_sorted_longest_first(self):
        files = [
            write_lines(self.root / "small.txt", 5),
            write_lines(self.root / "large.txt", 100),
            write_lines(self.root / "mid.txt", 20),
        ]
        result = self.evaluate(files)
        self.assertEqual(
            [item.lines for item in result.entries], [100, 20, 5]
        )

    def test_binary_and_missing_files_skipped_without_findings(self):
        binary = self.root / "shot.gif"
        binary.write_bytes(b"GIF89a\x01\x00more-bytes")
        missing = self.root / "gone.txt"
        result = self.evaluate([binary, missing])
        self.assertEqual(result.skipped_binary, 1)
        self.assertEqual(result.skipped_missing, 1)
        self.assertEqual(result.entries, [])
        self.assertEqual(result.over_unallowed, [])

    def test_rule_matching_under_limit_file_is_not_stale(self):
        small = write_lines(self.root / "src" / "ok.rs", 10)
        rules = [module.AllowRule(pattern="src/*.rs", max_lines=100)]
        result = self.evaluate([small], rules)
        self.assertEqual(result.stale_rules, [])

    def test_rule_matching_only_binary_file_is_not_stale(self):
        binary = self.root / "assets" / "logo.png"
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"\x89PNG\r\n\x1a\n\x00\x00")
        rules = [module.AllowRule(pattern="assets/*.png")]
        result = self.evaluate([binary], rules)
        self.assertEqual(result.stale_rules, [])
        self.assertEqual(result.skipped_binary, 1)

    def test_rule_matching_nothing_is_stale(self):
        present = write_lines(self.root / "src" / "here.rs", 10)
        rules = [module.AllowRule(pattern="src/nowhere.rs")]
        result = self.evaluate([present], rules)
        self.assertEqual(result.stale_rules, ["src/nowhere.rs"])


class ResolveAllowlistTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="vtcode-file-length-")
        self.root = Path(self.tmp.name)

    def tearDown(self):
        self.tmp.cleanup()

    def test_cli_and_file_patterns_merge_comments_skipped(self):
        allowlist = self.root / "custom.txt"
        allowlist.write_text(
            "# generated output\n\nCargo.lock\ndocs/big.md=2_000\n",
            encoding="utf-8",
        )
        rules = module.resolve_allowlist_patterns(["extra/**"], allowlist)
        self.assertEqual(
            [(rule.pattern, rule.max_lines) for rule in rules],
            [("extra/**", None), ("Cargo.lock", None), ("docs/big.md", 2000)],
        )

    def test_missing_allowlist_file_yields_no_rules(self):
        rules = module.resolve_allowlist_patterns([], self.root / "absent.txt")
        self.assertEqual(rules, [])

    def test_default_path_resolves_under_repo_root(self):
        scripts_dir = self.root / "scripts"
        scripts_dir.mkdir()
        (scripts_dir / "file_length_allowlist.txt").write_text("Cargo.lock\n")
        with mock.patch.object(module, "REPO_ROOT", self.root):
            rules = module.resolve_allowlist_patterns([], None)
        self.assertEqual(
            [(rule.pattern, rule.max_lines) for rule in rules],
            [("Cargo.lock", None)],
        )


class GateIntegrationTests(unittest.TestCase):
    """End-to-end oracles against the real checkout (no fixtures)."""

    def run_gate(self, *args):
        return subprocess.run(
            [sys.executable, str(SCRIPTS / "check_file_length.py"), *args],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            timeout=120,
        )

    def test_repo_allowlist_keeps_enforce_green(self):
        proc = self.run_gate("--mode", "enforce", "--quiet", "--top", "0")
        self.assertEqual(proc.returncode, 0, proc.stdout + proc.stderr)
        self.assertIn(">1000_unallowed=0", proc.stdout)
        self.assertIn("stale_allowlist=0", proc.stdout)

    def test_empty_allowlist_fails_and_never_flags_binaries(self):
        proc = self.run_gate(
            "--mode", "enforce", "--allowlist-file", "/dev/null", "--quiet", "--top", "0"
        )
        self.assertEqual(proc.returncode, 1, proc.stdout + proc.stderr)
        combined = proc.stdout + proc.stderr
        # Cargo.lock is a known >1000-line text file: must be reported.
        self.assertIn("Cargo.lock", combined)
        # Binary assets must be skipped, never reported as violations.
        self.assertNotIn("vtcode.gif", combined)
        self.assertNotIn(".png", combined)

    def test_invalid_allowlist_entry_exits_unusable(self):
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".txt", delete=False
        ) as handle:
            handle.write("docs/big.md=\n")
            path = handle.name
        try:
            proc = self.run_gate("--allowlist-file", path)
        finally:
            Path(path).unlink(missing_ok=True)
        self.assertEqual(proc.returncode, 2, proc.stdout + proc.stderr)
        self.assertIn("Invalid allowlist entry", proc.stderr)


if __name__ == "__main__":
    unittest.main()
