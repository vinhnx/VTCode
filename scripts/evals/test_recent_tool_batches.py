"""Offline outcome and process-lifecycle regressions for the paired pilot."""
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

import recent_tool_batches as pilot

FAKE = r"""
#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
root = pathlib.Path(args[args.index('--workspace') + 1])
attempt = root.parent
mode = os.environ.get('SIMULATED_MODE', 'success')
if mode == 'auth':
    print('401 invalid_api_key', file=sys.stderr)
    sys.exit(1)
root.joinpath('codec.py').write_text('def normalize(value):\n    return value.strip().lower()\n')
root.joinpath('adapter.py').write_text("import codec\ndef render(value):\n    return '[' + codec.normalize(value) + ']'\n")
if mode == 'verifier_failure':
    root.joinpath('adapter.py').write_text('def render(value):\n    return value\n')
if mode == 'tamper':
    root.joinpath('check.py').write_text('print("passed")\n')
events = [
    {'type': 'thread.started'},
    {'type': 'item.started', 'item': {'type': 'tool_invocation', 'id': 'one'}},
    {'type': 'item.completed', 'item': {'type': 'tool_invocation', 'id': 'one'}},
    {'type': 'thread.compact_boundary', 'original_message_count': 12, 'compacted_message_count': 4},
    {'type': 'thread.completed', 'subtype': 'success', 'outcome_code': 'completed',
     'usage': {'input_tokens': 3100, 'cached_input_tokens': 1100, 'cache_creation_tokens': 0, 'output_tokens': 200},
     'total_cost_usd': 0.001},
]
if mode == 'timeout':
    import time
    events.pop()
if mode == 'missing_pricing':
    events[-1].pop('total_cost_usd')
if mode == 'missing_usage':
    events[-1].pop('usage')
store = root / '.vtcode/sessions/fake/events.jsonl'
store.parent.mkdir(parents=True)
store.write_text('broken\n' if mode == 'malformed' else ''.join(
    json.dumps({'schema_version': '0.16.0', 'event': e}) + '\n' for e in events))
debug = attempt / 'debug'
debug.mkdir()
count = 0 if mode == 'unengaged' else 3
(debug / 'session.log').write_text(
    'DEBUG local tool-result clearing projection original_tokens=3100 projected_tokens=1800 '
    f'cleared_result_count={count} protected_batch_count=2\n')
if mode == 'timeout':
    time.sleep(30)
""".lstrip()


class RecentBatchPilotTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.binary = self.root / "fake-vtcode"
        self.binary.write_text(FAKE)
        self.binary.chmod(0o755)

    def tearDown(self):
        self.temp.cleanup()

    def attempt(self, mode="success", variant=2):
        env = dict(os.environ, SIMULATED_MODE=mode)
        return pilot.run_attempt(self.binary, self.root / mode, "cross_file", variant, env, deadline=5)

    def test_success_uses_canonical_events_external_verifier_and_diagnostics(self):
        result = self.attempt()
        self.assertEqual(result["status"], "success")
        self.assertTrue(result["independent_verification"]["passed"])
        self.assertTrue(result["clearing_engaged"])
        self.assertEqual(result["tool_calls"], 1)
        self.assertEqual(result["usage"]["input_tokens"], 3100)
        self.assertEqual(result["priced_cost_usd"], 0.001)
        self.assertEqual(len(result["compaction_boundaries"]), 1)

    def test_external_verifier_failure_overrides_canonical_success(self):
        result = self.attempt("verifier_failure")
        self.assertEqual(result["status"], "failed")
        self.assertTrue(result["canonical_success"])
        self.assertFalse(result["independent_verification"]["passed"])

    def test_early_success_exit_cannot_bypass_external_assertions(self):
        workspace = self.root / "early-exit"
        workspace.mkdir()
        workspace.joinpath("codec.py").write_text("raise SystemExit(0)\n")
        workspace.joinpath("adapter.py").write_text("raise SystemExit(0)\n")
        verification = pilot.verify(workspace, "mixed")
        self.assertEqual(verification["returncode"], 0)
        self.assertFalse(verification["passed"])

    def test_fixture_tampering_is_rejected_even_with_successful_verification(self):
        result = self.attempt("tamper")
        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["immutable_fixture_intact"])
        self.assertTrue(result["independent_verification"]["passed"])

    def test_malformed_events_are_inconclusive(self):
        result = self.attempt("malformed")
        self.assertEqual(result["status"], "inconclusive")
        self.assertTrue(result["infrastructure_failure"])
        self.assertIn("evidence_error", result)

    def test_missing_prices_or_usage_remain_unknown(self):
        for mode in ("missing_pricing", "missing_usage"):
            result = self.attempt(mode)
            self.assertIsNone(result["priced_cost_usd"])
            if mode == "missing_usage":
                self.assertIsNone(result["usage"])

    def test_nonengaged_clearing_does_not_imply_benefit(self):
        result = self.attempt("unengaged")
        self.assertEqual(result["status"], "success")
        self.assertFalse(result["clearing_engaged"])

    def test_authentication_failure_stops_before_outcome_claim(self):
        result = self.attempt("auth")
        self.assertEqual(result["status"], "inconclusive")
        self.assertTrue(result["infrastructure_failure"])
        self.assertIsNone(result["usage"])
        self.assertIsNone(result["priced_cost_usd"])

    def test_deadline_terminates_owned_process_group_and_retains_artifacts(self):
        workspace = self.root / "workspace"
        workspace.mkdir()
        code, reason, elapsed = pilot.run_process(
            [sys.executable, "-c", "import time; time.sleep(30)"],
            workspace, self.root, dict(os.environ), deadline=0.1)
        self.assertEqual(reason, "timeout")
        self.assertNotEqual(code, 0)
        self.assertLess(elapsed, 3)
        self.assertTrue((self.root / "stderr.txt").is_file())

    def test_attempt_timeout_retains_clearing_and_compaction_evidence(self):
        result = pilot.run_attempt(
            self.binary, self.root / "timeout", "cross_file", 2,
            dict(os.environ, SIMULATED_MODE="timeout"), deadline=1)
        self.assertEqual(result["status"], "inconclusive")
        self.assertEqual(result["termination"], "timeout")
        self.assertTrue(result["clearing_engaged"])
        self.assertEqual(len(result["compaction_boundaries"]), 1)
        self.assertIsNone(result["usage"])

    def test_live_pilot_stops_after_first_authentication_failure(self):
        output = self.root / "auth-artifacts"
        with mock.patch.dict(os.environ, {"OPENAI_API_KEY": "offline-test-key", "SIMULATED_MODE": "auth"}), mock.patch.object(
            sys, "argv", ["recent_tool_batches.py", "--live", "--binary", str(self.binary),
                          "--output", str(output)]), mock.patch("builtins.print"):
            # Live child environment intentionally omits unrelated environment
            # variables; inject the fake binary's mode without exposing it in
            # the production runner.
            original = pilot.run_attempt
            def simulated(binary, attempt, case, variant, env):
                return original(binary, attempt, case, variant, dict(env, SIMULATED_MODE="auth"))
            with mock.patch.object(pilot, "run_attempt", side_effect=simulated):
                pilot.main()
        report = json.loads((output / "report.json").read_text())
        self.assertEqual(report["stopped"], "authentication_or_infrastructure_failure")
        self.assertEqual(len(report["attempts"]), 1)
        self.assertEqual(report["attempts"][0]["status"], "inconclusive")

    def test_live_preflight_stops_without_credentials_or_attempts(self):
        output = self.root / "artifacts"
        with mock.patch.dict(os.environ, {}, clear=True), mock.patch.object(
            sys, "argv", ["recent_tool_batches.py", "--live", "--binary", str(self.binary),
                          "--output", str(output)]), mock.patch("builtins.print"):
            pilot.main()
        report = json.loads((output / "report.json").read_text())
        self.assertEqual(report["stopped"], "missing_openai_api_key")
        self.assertEqual(report["attempts"], [])

    def test_tool_call_watchdog_stops_at_limit(self):
        workspace = self.root / "workspace"
        workspace.mkdir()
        program = """
import json, pathlib, time
path = pathlib.Path('.vtcode/sessions/a/events.jsonl')
path.parent.mkdir(parents=True)
path.write_text(''.join(json.dumps({'event': {'type': 'item.started',
    'item': {'id': str(i), 'type': 'tool_invocation'}}}) + '\\n' for i in range(16)))
time.sleep(30)
"""
        code, reason, elapsed = pilot.run_process(
            [sys.executable, "-c", program], workspace, self.root, dict(os.environ), deadline=2)
        self.assertEqual(reason, "tool_call_limit")
        self.assertNotEqual(code, 0)
        self.assertLess(elapsed, 2)

    def test_paired_fixtures_are_identical_and_only_variant_changes(self):
        for index, case in enumerate(pilot.CASES):
            spec = pilot.fixture(case)
            self.assertEqual(spec, pilot.fixture(case))
            workspace = self.root / case
            workspace.mkdir()
            for name, content in spec["files"].items():
                workspace.joinpath(name).write_text(content)
            self.assertFalse(pilot.verify(workspace, spec["expected"])["passed"], "fixture starts broken")
            first, overrides0 = pilot.command(self.binary, workspace, self.root, 0, spec["prompt"])
            second, overrides2 = pilot.command(self.binary, workspace, self.root, 2, spec["prompt"])
            differences = [(a, b) for a, b in zip(first, second) if a != b]
            self.assertEqual(differences, [
                ("agent.harness.tool_result_clearing.keep_recent_tool_batches=0",
                 "agent.harness.tool_result_clearing.keep_recent_tool_batches=2")])
            self.assertEqual(len(first), len(second))
            self.assertIn("agent.harness.max_budget_usd=0.25", overrides0)
            self.assertIn("automation.full_auto.auto_grant_tool_limits=false", overrides2)

    def test_incomplete_diagnostics_do_not_report_nonengagement(self):
        debug = self.root / "debug"
        debug.mkdir()
        debug.joinpath("session.log").write_text(pilot.DIAGNOSTIC + " original_tokens=1")
        with self.assertRaisesRegex(ValueError, "incomplete clearing diagnostic"):
            pilot.clearing_diagnostics(self.root)


if __name__ == "__main__":
    unittest.main()
