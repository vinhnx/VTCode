import os
import json
import sys
import unittest

sys.path.insert(0, os.path.dirname(__file__))

from eval_engine import EvaluationEngine


class EvaluationEngineCommandTests(unittest.TestCase):
    def setUp(self):
        self.engine = EvaluationEngine(provider="test-provider", model="test-model")

    def test_advanced_profile_is_explicit(self):
        command = self.engine.build_command(
            "find symbols", "advanced_vtcode", "/tmp/final-message.txt"
        )

        self.assertIn("-c", command)
        self.assertIn("tools.profile=advanced_vtcode", command)
        self.assertEqual(
            command[-5:],
            [
                "exec",
                "--json",
                "--last-message-file",
                "/tmp/final-message.txt",
                "find symbols",
            ],
        )

    def test_default_profile_is_explicit(self):
        command = self.engine.build_command(
            "list files", "codex_default", "/tmp/final-message.txt"
        )

        self.assertIn("tools.profile=codex_default", command)

    def test_unknown_profile_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "Unsupported eval profile"):
            self.engine.build_command(
                "archival case", "archived_baseline", "/tmp/final-message.txt"
            )

    def test_ai_tool_surface_suite_has_only_executable_profiles(self):
        cases_path = os.path.join(
            os.path.dirname(__file__),
            "..",
            "docs",
            "development",
            "ai-tool-surface-eval-cases.json",
        )
        with open(cases_path, encoding="utf-8") as cases_file:
            cases = json.load(cases_file)

        self.assertEqual(len(cases), 7)
        self.assertEqual(
            {case["profile"] for case in cases},
            {"codex_default", "advanced_vtcode"},
        )
        self.assertNotIn(
            "tool_surface_archived_baseline_unavailable",
            {case["id"] for case in cases},
        )
        self.assertEqual(
            [case["metric"] for case in cases[:2]],
            ["llm_grader", "llm_grader"],
        )
        self.assertTrue(
            all(case["metric"] in self.engine.metrics for case in cases)
        )


if __name__ == "__main__":
    unittest.main()
