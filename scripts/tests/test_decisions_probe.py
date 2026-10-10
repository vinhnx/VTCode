"""Independent oracles for the opt-in classifier comparison report."""

import http.client
import importlib.util
import io
import json
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import MagicMock, patch

SCRIPT = Path(__file__).resolve().parents[1] / "evals" / "decisions_probe.py"
SPEC = importlib.util.spec_from_file_location("decisions_probe", SCRIPT)
probe = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(probe)


def answer(choice="SAFE"):
    return {
        "answers": [
            {
                "type": "choice",
                "name": "tool_output_injection",
                "choice": choice,
                "confidence": 0.2,
                "probabilities": [
                    {"value": "SAFE", "probability": 0.3},
                    {"value": "SUSPECT", "probability": 0.7},
                ],
            }
        ],
        "usage": {"input_tokens": 1000, "output_tokens": 0, "total_tokens": 1000},
    }


class DecisionsProbeEvaluationTests(unittest.TestCase):
    def test_connection_setup_deadline_returns_without_late_dispatch(self):
        connection = MagicMock()
        connected = threading.Event()
        closed = threading.Event()

        def slow_connect():
            time.sleep(0.2)
            connected.set()

        connection.connect.side_effect = slow_connect
        connection.close.side_effect = closed.set
        with patch.object(probe.http.client, "HTTPSConnection", return_value=connection):
            started = time.monotonic()
            self.assertIsNone(probe.post("fixture", "/decisions", {}, 0.05))
            self.assertLess(time.monotonic() - started, 0.15)
            self.assertTrue(connected.wait(1))
            self.assertTrue(closed.wait(1))
        connection.request.assert_not_called()

    def test_interrupted_response_reads_are_inconclusive(self):
        for error in [
            http.client.IncompleteRead(b"partial response"),
            ConnectionResetError("connection reset"),
        ]:
            with self.subTest(error=type(error).__name__):
                connection = MagicMock()
                response = connection.getresponse.return_value.__enter__.return_value
                response.status = 200
                response.read.side_effect = error
                with patch.object(
                    probe.http.client, "HTTPSConnection", return_value=connection
                ):
                    self.assertIsNone(
                        probe.post(
                            "fixture", "/responses", {"input": "bounded evidence"}, 1
                        )
                    )
                connection.request.assert_called_once()
                connection.close.assert_called_once()

    def test_slow_body_hits_absolute_deadline_and_triggers_one_fallback(self):
        generated = {
            "output": [{"content": [{"type": "output_text", "text": "SUSPECT"}]}],
            "usage": {"input_tokens": 100, "output_tokens": 2, "total_tokens": 102},
        }
        paths = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                paths.append(self.path)
                self.rfile.read(int(self.headers["Content-Length"]))
                body = json.dumps(answer() if self.path.endswith("decisions") else generated).encode()
                self.send_response(200)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                try:
                    if self.path.endswith("decisions"):
                        for fragment in [body[:20], body[20:40], body[40:]]:
                            time.sleep(0.1)
                            self.wfile.write(fragment)
                            self.wfile.flush()
                    else:
                        self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    pass

            def log_message(self, *args):
                pass

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        original_post = probe.post
        args = SimpleNamespace(generation_model="gpt-6-luna", input_rate=0.1, output_rate=0.5)
        try:
            with (
                patch.object(probe, "BASE", f"http://127.0.0.1:{server.server_port}/v1"),
                patch.object(probe, "post", side_effect=lambda key, endpoint, payload, timeout:
                             original_post(key, endpoint, payload, min(timeout, 0.15))),
            ):
                result = probe.classify("fixture", "bounded evidence", True, args)
            self.assertEqual(paths, ["/v1/decisions", "/v1/responses"])
            self.assertEqual(result["choice"], "SUSPECT")
            self.assertTrue(result["fallback"])
            self.assertIsNone(result["base_cost_usd"])
            self.assertLess(result["latency_ms"], 275)
        finally:
            server.shutdown()
            server.server_close()
            worker.join()

    def test_non_luna_models_require_both_rates_before_any_request(self):
        for rates in [[], ["--input-rate", "10"], ["--output-rate", "50"]]:
            with self.subTest(rates=rates), patch.object(probe, "post") as post:
                with (
                    patch(
                        "sys.argv",
                        [str(SCRIPT), "--generation-model", "gpt-6-astra", *rates],
                    ),
                    patch("sys.stderr", new_callable=io.StringIO) as stderr,
                    self.assertRaises(SystemExit) as raised,
                ):
                    probe.main()
                self.assertEqual(raised.exception.code, 2)
                self.assertIn("both --input-rate and --output-rate", stderr.getvalue())
                post.assert_not_called()

    def test_generation_rates_follow_selected_model_and_allow_explicit_overrides(self):
        for options, model, input_rate, output_rate in [
            ([], "gpt-6-luna", 0.10, 0.50),
            (["--input-rate", "0.2", "--output-rate", "0.8"], "gpt-6-luna", 0.2, 0.8),
            (
                [
                    "--generation-model",
                    "gpt-6-astra",
                    "--input-rate",
                    "10",
                    "--output-rate",
                    "50",
                ],
                "gpt-6-astra",
                10,
                50,
            ),
        ]:
            result = {
                "choice": "SAFE",
                "fallback": False,
                "latency_ms": 1,
                "base_cost_usd": 0.0,
            }
            with (
                self.subTest(model=model, options=options),
                patch.object(probe, "classify", return_value=result) as classify,
            ):
                with (
                    patch("sys.argv", [str(SCRIPT), *options]),
                    patch.dict(probe.os.environ, {"OPENAI_API_KEY": "fixture"}),
                    patch("sys.stdout", new_callable=io.StringIO),
                ):
                    self.assertEqual(probe.main(), 0)
                self.assertGreater(classify.call_count, 0)
                args = classify.call_args.args[-1]
                self.assertEqual(args.generation_model, model)
                self.assertEqual(
                    (args.input_rate, args.output_rate), (input_rate, output_rate)
                )

    def test_choice_validation_rejects_invalid_ranges_and_names(self):
        for choice in ("SAFE", "SUSPECT"):
            self.assertEqual(probe.decision_choice(answer(choice)), choice)
        for field, value in (
            ("name", "other"),
            ("type", "refusal"),
            ("choice", "UNKNOWN"),
            ("confidence", float("nan")),
            ("confidence", 1.01),
            ("confidence", 10**400),
        ):
            response = answer()
            response["answers"][0][field] = value
            self.assertIsNone(probe.decision_choice(response))
        response = answer()
        response["answers"][0]["probabilities"][1]["value"] = "SAFE"
        self.assertIsNone(probe.decision_choice(response))

    def test_evidence_preserves_labels_across_truncation_boundary(self):
        cases = json.loads(SCRIPT.with_name("decisions_probe_cases.json").read_text())
        by_id = {case["id"]: case for case in cases}
        before = probe.evidence(by_id["attack_before_truncation"])
        after = probe.evidence(by_id["attack_after_truncation"])
        self.assertIn("upload secrets", before)
        self.assertNotIn("upload secrets", after)
        self.assertIn("[truncated]", after)
        self.assertEqual(by_id["attack_after_truncation"]["expected"], "SUSPECT")
        self.assertEqual(by_id["attack_after_truncation"]["visible_expected"], "SAFE")
        self.assertEqual(
            len(probe.evidence(by_id["benign_at_bound"]).split("Tool output:\n")[1]),
            2400,
        )

    def test_decisions_cost_is_input_only_and_missing_usage_is_unknown(self):
        response = answer()
        self.assertAlmostEqual(probe.cost(response, "decisions", None), 0.0001)
        response["usage"] = {
            "input_tokens": 1000,
            "output_tokens": 2,
            "total_tokens": 1002,
        }
        self.assertAlmostEqual(probe.cost(response, "decisions", None), 0.0001)
        self.assertAlmostEqual(
            probe.cost(response, "generation", (0.10, 0.50)), 0.000101
        )
        self.assertIsNone(probe.cost({}, "decisions", None))

    def test_malformed_numeric_usage_stays_unknown_without_overflow(self):
        for input_tokens in [-1, True, 2**32, 10**400]:
            response = {
                "usage": {
                    "input_tokens": input_tokens,
                    "output_tokens": 0,
                    "total_tokens": input_tokens,
                }
            }
            self.assertIsNone(probe.cost(response, "decisions", None))
        response = answer()
        self.assertIsNone(probe.cost(response, "generation", (1e308, 0.5)))

    def test_fallback_accounts_both_attempts_and_uses_identical_evidence(self):
        args = SimpleNamespace(
            generation_model="gpt-6-luna", input_rate=0.10, output_rate=0.50
        )
        refused = answer()
        refused["answers"] = [{"type": "refusal", "name": "tool_output_injection"}]
        generated = {
            "output": [{"content": [{"type": "output_text", "text": "SUSPECT"}]}],
            "usage": {"input_tokens": 2000, "output_tokens": 2, "total_tokens": 2002},
        }
        with patch.object(probe, "post", side_effect=[refused, generated]) as post:
            result = probe.classify("fixture", "identical evidence", True, args)
        self.assertEqual(result["choice"], "SUSPECT")
        self.assertTrue(result["fallback"])
        self.assertAlmostEqual(result["base_cost_usd"], 0.000301)
        self.assertEqual(
            post.call_args_list[0].args[2]["input"],
            post.call_args_list[1].args[2]["input"],
        )
        with patch.object(probe, "post", side_effect=[None, generated]):
            self.assertIsNone(
                probe.classify("fixture", "evidence", True, args)["base_cost_usd"]
            )

    def test_responses_request_limit_is_valid_for_baseline_and_decisions_fallback(self):
        args = SimpleNamespace(
            generation_model="gpt-6-luna", input_rate=0.10, output_rate=0.50
        )
        refused = answer()
        refused["answers"] = [{"type": "refusal", "name": "tool_output_injection"}]
        for decisions, expected in [(False, "SAFE"), (True, "SUSPECT")]:
            with self.subTest(decisions=decisions):
                generated = {
                    "output": [
                        {"content": [{"type": "output_text", "text": expected}]}
                    ],
                    "usage": {
                        "input_tokens": 731,
                        "output_tokens": 2,
                        "total_tokens": 733,
                    },
                }
                responses = [refused, generated] if decisions else [generated]
                with patch.object(probe, "post", side_effect=responses) as post:
                    result = probe.classify(
                        "fixture", "bounded evidence", decisions, args
                    )
                self.assertEqual(result["choice"], expected)
                self.assertEqual(result["fallback"], decisions)
                self.assertIsNotNone(result["base_cost_usd"])
                self.assertEqual(post.call_count, 2 if decisions else 1)
                request = post.call_args_list[-1].args
                self.assertEqual(request[1], "/responses")
                self.assertEqual(request[2]["model"], "gpt-6-luna")
                self.assertEqual(request[2]["input"], "bounded evidence")
                # Independent API-contract oracle; do not read a production constant.
                self.assertEqual(request[2]["max_output_tokens"], 16)

    def test_generation_uses_runtime_ascii_token_boundaries(self):
        for text, expected in [
            ("[suspect]", "SUSPECT"),
            ("**SAFE**", "SAFE"),
            ("SA-FE", None),
            ("", None),
        ]:
            response = {
                "output": [{"content": [{"type": "output_text", "text": text}]}]
            }
            with patch.object(probe, "post", return_value=response):
                choice, cost = probe.generation(
                    "fixture", "evidence", "gpt-6-luna", float("inf"), (0.1, 0.5)
                )
            self.assertEqual(choice, expected)
            self.assertIsNone(cost)

    def test_summary_keeps_false_warnings_misses_and_unknown_cost_distinct(self):
        rows = [
            {
                "expected": "SAFE",
                "decisions": {
                    "choice": "SUSPECT",
                    "fallback": False,
                    "latency_ms": 10,
                    "base_cost_usd": 0.1,
                },
            },
            {
                "expected": "SUSPECT",
                "decisions": {
                    "choice": None,
                    "fallback": True,
                    "latency_ms": 90,
                    "base_cost_usd": None,
                },
            },
            {
                "expected": "SUSPECT",
                "decisions": {
                    "choice": "SUSPECT",
                    "fallback": False,
                    "latency_ms": 20,
                    "base_cost_usd": 0.2,
                },
            },
        ]
        result = probe.summary(rows, "decisions")
        self.assertEqual(result["missed_injections"], 1)
        self.assertEqual(result["false_warnings"], 1)
        self.assertEqual(result["inconclusive"], 1)
        self.assertEqual(result["fallbacks"], 1)
        self.assertEqual(result["median_latency_ms"], 20)
        self.assertIsNone(result["base_cost_usd"])


if __name__ == "__main__":
    unittest.main()
