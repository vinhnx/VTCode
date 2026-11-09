"""Stale allowlist detection for the structured-logging and large-file gates.

A stale file rule silently pre-authorizes future violations, and a stale
path:line rule breaks the gate when code moves, so both checkers must report
rules that excuse nothing. Each case pins both sides: a used rule is kept, an
unused one is reported.
"""

import importlib.util
import sys
import unittest
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


logging_gate = load("check_structured_logging")
large_files = load("check_large_files")


class StructuredLoggingStalenessTests(unittest.TestCase):
    def test_matching_rule_honors_line_pins(self):
        file_rule = logging_gate.AllowRule("crates/a/src/x.rs")
        line_rule = logging_gate.AllowRule("crates/b/src/y.rs", 12)
        rules = [file_rule, line_rule]
        self.assertIs(logging_gate.matching_rule("crates/a/src/x.rs", 99, rules), file_rule)
        self.assertIs(logging_gate.matching_rule("crates/b/src/y.rs", 12, rules), line_rule)
        self.assertIsNone(logging_gate.matching_rule("crates/b/src/y.rs", 13, rules))

    def test_only_rules_that_excused_nothing_are_stale(self):
        used = logging_gate.AllowRule("crates/a/src/x.rs")
        moved = logging_gate.AllowRule("crates/b/src/y.rs", 101)
        gone = logging_gate.AllowRule("crates/c/src/**")
        self.assertEqual(logging_gate.stale_rules([used, moved, gone], {used}), [moved, gone])
        self.assertEqual(logging_gate.stale_rules([used], {used}), [])


class LargeFileStalenessTests(unittest.TestCase):
    def tracked(self, *relative):
        return [large_files.REPO_ROOT / path for path in relative]

    def test_rule_matching_a_tracked_path_is_kept(self):
        rules = [large_files.parse_allow_rule("resources/gif/*.gif=6_000_000")]
        self.assertEqual(large_files.stale_rules(self.tracked("resources/gif/demo.gif"), rules), [])

    def test_rule_matching_no_tracked_path_is_stale(self):
        rules = [
            large_files.parse_allow_rule("resources/gif/demo.gif=6_000_000"),
            large_files.parse_allow_rule("resources/logo/removed.png=1_200_000"),
        ]
        self.assertEqual(
            large_files.stale_rules(self.tracked("resources/gif/demo.gif"), rules),
            ["resources/logo/removed.png"],
        )


if __name__ == "__main__":
    unittest.main()
