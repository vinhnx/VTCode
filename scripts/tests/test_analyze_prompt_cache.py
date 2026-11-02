"""Cache reporting must distinguish observations, absent counters and estimates."""

import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("analyze_prompt_cache", Path(__file__).parents[1] / "analyze_prompt_cache.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PromptCacheReportTests(unittest.TestCase):
    def summarize(self, rows, **kwargs):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trajectory.jsonl"
            path.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
            return module.summarize([path, path], **kwargs)

    def usage(self, **overrides):
        return {"kind": "prompt_cache_metrics", "provider": "openai", "model": "gpt-6-luna",
                "input_tokens": 200, "cache_read_tokens_reported": 73,
                "cache_write_tokens_reported": 41, "elapsed_ms": 90, **overrides}

    def test_weighted_reads_cost_and_route_filtering(self):
        result = self.summarize([
            self.usage(), self.usage(input_tokens=100, cache_read_tokens_reported=0, cache_write_tokens_reported=25),
            self.usage(provider="merge-gateway"),
        ], provider="openai", model="gpt-6-luna", rates=(10, 1, 12.5))
        group, = result["groups"]
        self.assertEqual(group["requests"], 2)  # Duplicate paths cannot double-count input.
        self.assertAlmostEqual(group["weighted_read_fraction"], 73 / 300)
        # 161 ordinary inputs + 73 reads + 66 writes, priced separately.
        self.assertAlmostEqual(group["estimated_input_cost_usd"], (1610 + 73 + 825) / 1_000_000)

    def test_missing_and_legacy_zeros_are_unknown_but_explicit_zero_is_known(self):
        for row in [self.usage(cache_write_tokens_reported=None),
                    {"kind": "prompt_cache_metrics", "model": "gpt-6-luna", "prompt_tokens": 200,
                     "cache_read_tokens": 0, "cache_creation_tokens": 0}]:
            group, = self.summarize([row], rates=(10, 1, 12.5))["groups"]
            self.assertIsNone(group["estimated_input_cost_usd"])
            self.assertEqual(group["requests_missing_write_counter"], 1)
        group, = self.summarize([self.usage(cache_read_tokens_reported=0, cache_write_tokens_reported=0)],
                               rates=(10, 1, 12.5))["groups"]
        self.assertEqual(group["weighted_read_fraction"], 0)
        self.assertAlmostEqual(group["estimated_input_cost_usd"], 0.002)

    def test_invalid_counters_and_projection_estimates_are_separate(self):
        result = self.summarize([
            self.usage(cache_read_tokens_reported=201), self.usage(input_tokens=-1), [],
            {"kind": "history_projection_metrics", "provider": "openai", "model": "gpt-6-luna",
             "original_estimated_tokens": 900, "projected_estimated_tokens": 300, "changed_message_count": 2},
        ])
        self.assertEqual(result["malformed_rows"], 3)
        group, = result["groups"]
        self.assertEqual(group["requests"], 0)
        self.assertEqual(group["estimated_tokens_removed_by_clearing"], 600)
        self.assertEqual(group["clearing_changed_projections"], 1)
        self.assertIsNone(group["estimated_input_cost_usd"])

    def test_legacy_prompt_tokens_cannot_be_used_as_a_normalized_denominator(self):
        group, = self.summarize([{"kind": "prompt_cache_metrics", "provider": "anthropic", "model": "legacy",
                                 "prompt_tokens": 10, "cache_read_tokens": 800, "cache_creation_tokens": 90}])["groups"]
        self.assertEqual(group["requests"], 1)
        self.assertEqual(group["legacy_reported_prompt_tokens"], 10)
        self.assertEqual(group["input_tokens"], 0)
        self.assertIsNone(group["weighted_read_fraction"])
        self.assertIsNone(group["observed_read_fraction_lower_bound"])

    def test_auxiliary_tracing_exports_are_separate_from_foreground_usage(self):
        rows = [self.usage(), {"fields": self.usage(scope="auxiliary", input_tokens=400)}]
        result = self.summarize(rows)
        self.assertEqual(len(result["groups"]), 2)
        auxiliary, = self.summarize(rows, scope="auxiliary")["groups"]
        self.assertEqual(auxiliary["requests"], 1)
        self.assertEqual(auxiliary["input_tokens"], 400)

    def test_cli_rejects_partial_rates_and_never_prints_raw_content(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trajectory.jsonl"
            path.write_text("broken JSON\n" + json.dumps(self.usage(content="never-print-this-content")), encoding="utf-8")
            command = [sys.executable, str(Path(module.__file__)), str(path)]
            result = subprocess.run(command, capture_output=True, text=True, check=True)
            self.assertEqual(json.loads(result.stdout)["malformed_rows"], 1)
            self.assertNotIn("never-print-this-content", result.stdout + result.stderr)
            rejected = subprocess.run(command + ["--input-rate", "10"], capture_output=True, text=True)
            self.assertEqual(rejected.returncode, 2)
            self.assertIn("pricing requires an exact provider and model", rejected.stderr)


if __name__ == "__main__":
    unittest.main()
