#!/usr/bin/env python3
"""Bounded, paired recent-batch pilot. Standard library only; no LLM grading."""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

REPO = Path(__file__).resolve().parents[2]
CASES = ("cross_file", "failed_verifier", "fresh_read", "oversized", "control")
DIAGNOSTIC = "local tool-result clearing projection"
FIELDS = ("original_tokens", "projected_tokens", "cleared_result_count", "protected_batch_count")
INFRA = re.compile(
    r"invalid_api_key|incorrect api key|authentication|unauthorized|\b401\b|\b403\b|"
    r"\b429\b|rate.limit|connection refused|dns|timed out connecting|"
    r"pricing.*unavailable|complete pricing|failed to.*provider|unsupported model|"
    r"model.*(?:not found|does not exist|not available)|"
    r"network.*(?:error|unreachable)|error sending request|\b50[0234]\b",
    re.I,
)
# This verifier lives outside agent workspaces; fixtures cannot change its oracle.
VERIFIER = """
import importlib.util, pathlib, sys
root = pathlib.Path(sys.argv[1])
sys.path.insert(0, str(root))
def load(name):
    spec = importlib.util.spec_from_file_location(name, root / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module
codec = load('codec')
adapter = load('adapter')
expected = sys.argv[2]
assert codec.normalize('  MiXeD  ') == expected
assert adapter.render('  MiXeD  ') == '[' + expected + ']'
assert codec.normalize('') == ''
assert adapter.render('') == '[]'
print("external-verification-complete")
"""
BASE_OVERRIDES = (
    "agent.reasoning_effort=medium",
    "agent.api_key_env=OPENAI_API_KEY",
    "default_primary_agent=build",
    "agent.allow_reasoning_effort_downgrade=false",
    "agent.todo_planning_mode=false",
    "agent.enable_split_tool_results=false",
    "agent.harness.orchestration_mode=single",
    "agent.harness.max_tool_calls_per_turn=16",
    "agent.harness.max_parallel_tool_calls=1",
    "agent.harness.max_tool_wall_clock_secs=30",
    "agent.harness.max_budget_usd=0.25",
    "agent.harness.auto_compaction_enabled=true",
    "agent.harness.tool_result_clearing.enabled=true",
    "agent.harness.tool_result_clearing.trigger_tokens=2500",
    "agent.harness.tool_result_clearing.keep_tool_uses=2",
    "agent.harness.tool_result_clearing.clear_at_least_tokens=1",
    "agent.harness.tool_result_clearing.clear_tool_inputs=true",
    "automation.full_auto.enabled=true",
    "automation.full_auto.require_profile_ack=false",
    "automation.full_auto.auto_grant_tool_limits=false",
    "automation.full_auto.max_turns=16",
    'automation.full_auto.allowed_tools=["exec_command","write_stdin","apply_patch"]',
    "tools.profile=vt_code",
    "mcp.enabled=false",
    "agent.persistent_memory.enabled=false",
    "prompt_cache.enabled=false",
    "tools.max_tool_loops=16",
    "permissions.auto.use_decisions_probe=false",
    "context.max_context_tokens=32000",
    "context.preserve_recent_turns=2",
    "debug.enable_tracing=true",
    "debug.trace_level=debug",
    'debug.trace_targets=["vtcode_core::core::agent::state"]',
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_hashes(root, names):
    hashes = {}
    for name in names:
        path = root / name
        hashes[name] = digest(path.read_bytes()) if path.is_file() and not path.is_symlink() else None
    return hashes


def fixture(case):
    """Deterministic fixtures with distinct earlier/middle/latest observations."""
    if case not in CASES:
        raise ValueError(f"unknown fixture: {case}")
    words = 30 if case == "control" else 800
    if case == "oversized":
        words = 8500
    files = {
        "codec.py": "def normalize(value):\n    return value.strip()\n",
        "adapter.py": "def render(value):\n    return value\n",
        "contract.txt": "normalize: strip whitespace then lowercase. render: bracket normalized text.\n",
        "check.py": (
            "import codec, adapter\n"
            "assert codec.normalize('  MiXeD  ') == 'mixed'\n"
            "assert adapter.render('  MiXeD  ') == '[mixed]'\n"
        ),
    }
    # Deterministic diverse filler resists tokenizer compression. It is task data,
    # never an instruction. Both variants get byte-identical files and prompts.
    filler = " ".join(hashlib.sha256(str(i).encode()).hexdigest()[:8] for i in range(words))
    observations = {
        "early": files["contract.txt"],
        "middle": "adapter.render must call codec.normalize before adding brackets.\n",
        "latest": "Preserve empty strings; verification covers mixed case and whitespace.\n",
    }
    for round_name, count in (("early", 1), ("middle", 4), ("latest", 3)):
        observation = observations[round_name]
        for index in range(count):
            files[f"{round_name}_{index}.txt"] = (
                observation + "\nDiagnostic samples: " + filler + "\n" + observation)
    files["advance.py"] = (
        "from pathlib import Path\n"
        "Path('contract.txt').write_text('normalize: strip whitespace then uppercase. render: bracket normalized text.\\n')\n"
        "print('Contract revision advanced; reread contract.txt before editing.')\n"
    )
    prompt = (
        "Repair codec.py and adapter.py according to contract.txt. This is a controlled context-retention "
        "experiment. Keep tools and planning fixed: use exec_command for inspection and verification, "
        "apply_patch for edits; do not delegate or plan with tools. Use no more than 16 tool calls total. "
        "Before editing, make THREE sequential assistant tool-call rounds, waiting for all results in each "
        "before the next: (1) one exec_command: cat early_0.txt codec.py adapter.py contract.txt; "
        "(2) FOUR exec_command calls in a SINGLE assistant response, one each for cat middle_0.txt, "
        "cat middle_1.txt, cat middle_2.txt, cat middle_3.txt; "
        "(3) THREE exec_command calls in a SINGLE assistant response, one each for cat latest_0.txt, "
        "cat latest_1.txt, cat latest_2.txt. "
        "Use yield_time_ms=1000 and max_output_tokens=24000 for every evidence call. "
        "Do not combine calls into one shell command or split a round across assistant responses. "
        "Then repair both source files and run python3 check.py. "
        "Leave observation files, scripts and check.py intact. "
        "An immutable external verifier also checks both functions on asymmetric and empty inputs. "
    )
    expected = "mixed"
    if case == "failed_verifier":
        prompt = "First run python3 check.py and observe its failure before the evidence rounds. " + prompt
    elif case == "fresh_read":
        prompt += (
            "After round 1, run python3 advance.py to change the contract. After round 3, "
            "reread contract.txt using a fresh exec_command before editing. check.py is intentionally "
            "the stale lowercase verifier: leave it intact and verify the revised uppercase behavior "
            "with a new inline Python check. The external verifier uses the revised contract."
        )
        expected = "MIXED"
    immutable = sorted(set(files) - {"codec.py", "adapter.py", "contract.txt"})
    return {"files": files, "prompt": prompt, "immutable": immutable, "expected": expected}


def command(binary, workspace, attempt, variant, prompt):
    overrides = BASE_OVERRIDES + (
        f"agent.harness.tool_result_clearing.keep_recent_tool_batches={variant}",
        "debug.debug_log_dir=" + json.dumps(str(attempt / "debug")),
    )
    args = [str(binary), "--provider", "openai", "--model", "gpt-6-luna", "--workspace", str(workspace)]
    for value in overrides:
        args.extend(("-c", value))
    args.extend(("exec", "--json", "--events", str(attempt / "export.jsonl"),
                 "--last-message-file", str(attempt / "last-message.txt"), prompt))
    return args, overrides


def read_events(workspace):
    paths = sorted(workspace.glob(".vtcode/sessions/*/events.jsonl"))
    if len(paths) != 1:
        raise ValueError(f"expected one canonical event log, found {len(paths)}")
    events = []
    for line in paths[0].read_text().splitlines():
        row = json.loads(line)
        event = row.get("event") if isinstance(row, dict) else None
        if not isinstance(event, dict) or not isinstance(event.get("type"), str):
            raise ValueError("malformed canonical event envelope")
        events.append(event)
    return events, str(paths[0])


def tool_count(events):
    # Count admission once per item.started; completion and update rows duplicate IDs.
    return len({event["item"]["id"] for event in events
                if event.get("type") == "item.started"
                and event.get("item", {}).get("type") == "tool_invocation"})


def summarize(events):
    terminal = [event for event in events if event["type"] == "thread.completed"]
    if len(terminal) != 1 or not any(event["type"] == "thread.started" for event in events):
        raise ValueError("missing or duplicate canonical thread lifecycle")
    terminal = terminal[0]
    usage = terminal.get("usage")
    keys = ("input_tokens", "cached_input_tokens", "cache_creation_tokens", "output_tokens")
    if not isinstance(usage, dict) or any(type(usage.get(key)) is not int or usage[key] < 0 for key in keys):
        usage = None
    elif usage["input_tokens"] + usage["output_tokens"] == 0:
        usage = None  # No observed provider usage, rather than a measured zero.
    cost = terminal.get("total_cost_usd")
    if usage is None or type(cost) not in (int, float) or not math.isfinite(cost) or cost < 0:
        cost = None
    return {
        "canonical_outcome": terminal.get("outcome_code"),
        "canonical_success": terminal.get("subtype") == "success",
        "usage": usage, "priced_cost_usd": cost,
        "tool_calls": tool_count(events),
        "compaction_boundaries": [event for event in events if event["type"] == "thread.compact_boundary"],
    }


def clearing_diagnostics(attempt):
    rows = []
    paths = sorted((attempt / "debug").glob("*.log"))
    if not paths:
        raise ValueError("missing debug diagnostics artifact")
    for path in paths:
        for line in path.read_text(errors="replace").splitlines():
            if DIAGNOSTIC not in line:
                continue
            row = {}
            for field in FIELDS:
                match = re.search(r"\b" + field + r"=(\d+)", line)
                if not match:
                    raise ValueError("incomplete clearing diagnostic")
                row[field] = int(match[1])
            rows.append(row)
    return rows


def stop_owned(process):
    """Always terminate the new session's process group, including descendants."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(process.pid, sig)
        except ProcessLookupError:
            break
        if sig == signal.SIGTERM:
            try:
                process.wait(timeout=0.3)
            except subprocess.TimeoutExpired:
                pass
    process.wait(timeout=5)


def run_process(args, workspace, attempt, env, deadline=120):
    started = time.monotonic()
    reason = None
    with (attempt / "stdout.jsonl").open("wb") as stdout, (attempt / "stderr.txt").open("wb") as stderr:
        process = subprocess.Popen(args, cwd=workspace, env=env, stdin=subprocess.DEVNULL,
                                   stdout=stdout, stderr=stderr, start_new_session=True)
        try:
            while process.poll() is None:
                if time.monotonic() - started >= deadline:
                    reason = "timeout"
                    break
                try:
                    events, _ = read_events(workspace)
                    if tool_count(events) >= 16:
                        reason = "tool_call_limit"
                        break
                except (ValueError, OSError):
                    pass  # The writer may be creating a log or appending its last line.
                time.sleep(0.02)
        finally:
            stop_owned(process)
    return process.returncode, reason, time.monotonic() - started


def verify(workspace, expected):
    try:
        process = subprocess.Popen([sys.executable, "-I", "-c", VERIFIER, str(workspace), expected],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
                                   cwd=workspace, start_new_session=True)
        try:
            stdout, stderr = process.communicate(timeout=10)
            return {"passed": process.returncode == 0 and stdout == "external-verification-complete\n",
                    "returncode": process.returncode,
                    "stdout": stdout, "stderr": stderr}
        finally:
            stop_owned(process)
    except subprocess.TimeoutExpired:
        return {"passed": False, "error": "verifier_timeout"}


def run_attempt(binary, attempt, case, variant, env, deadline=120):
    attempt.mkdir()
    workspace = attempt / "workspace"
    workspace.mkdir()
    spec = fixture(case)
    for name, content in spec["files"].items():
        (workspace / name).write_text(content)
    baseline = file_hashes(workspace, spec["files"])
    (attempt / "fixture.json").write_text(json.dumps(spec, indent=2))
    (attempt / "prompt.txt").write_text(spec["prompt"])
    # A local config fixes the API endpoint and prevents account routes/auxiliary
    # probes from being inherited. No credentials are written into artifacts.
    (workspace / "vtcode.toml").write_text(
        '[agent]\nprovider = "openai"\nmodel = "gpt-6-luna"\n'
        '[providers.openai]\nbase_url = "https://api.openai.com/v1"\n'
    )
    args, overrides = command(binary, workspace, attempt, variant, spec["prompt"])
    (attempt / "command.json").write_text(json.dumps(args, indent=2))
    (attempt / "overrides.json").write_text(json.dumps(overrides, indent=2))
    result = {"case": case, "variant": variant, "fixture_hashes": baseline,
              "prompt_sha256": digest(spec["prompt"].encode()),
              "config_sha256": digest(json.dumps(overrides).encode()),
              "workspace_config_sha256": digest((workspace / "vtcode.toml").read_bytes()),
              "verifier_sha256": digest(VERIFIER.encode())}
    try:
        code, reason, elapsed = run_process(args, workspace, attempt, env, deadline)
    except OSError as error:
        code, reason, elapsed = None, None, None
        (attempt / "stderr.txt").write_text(f"process startup failed: {error}")
    result.update(returncode=code, termination=reason, elapsed_seconds=elapsed)
    after = file_hashes(workspace, spec["files"])
    intact = all(after[name] == baseline[name] for name in spec["immutable"])
    intact = intact and file_hashes(workspace, ["vtcode.toml"])["vtcode.toml"] == result["workspace_config_sha256"]
    expected_contract = spec["files"]["contract.txt"] if case != "fresh_read" else (
        "normalize: strip whitespace then uppercase. render: bracket normalized text.\n")
    contract_intact = file_hashes(workspace, ["contract.txt"])["contract.txt"] == digest(expected_contract.encode())
    result.update(final_fixture_hashes=after, immutable_fixture_intact=intact,
                  contract_integrity=contract_intact)
    verification = verify(workspace, spec["expected"])
    result["independent_verification"] = verification
    evidence_error = None
    events = []
    try:
        events, path = read_events(workspace)
        result["canonical_event_log"] = path
        result["tool_calls"] = tool_count(events)
        result["compaction_boundaries"] = [event for event in events if event["type"] == "thread.compact_boundary"]
        result.update(summarize(events))
    except (ValueError, OSError) as error:
        evidence_error = str(error)
        result["evidence_error"] = evidence_error
    try:
        result["clearing_diagnostics"] = clearing_diagnostics(attempt)
        result["clearing_engaged"] = any(row["cleared_result_count"] > 0 for row in result["clearing_diagnostics"])
    except (ValueError, OSError) as error:
        result["diagnostic_error"] = str(error)
        # Diagnostics are secondary evidence; missing logs do not negate a
        # valid canonical outcome, but engagement remains unknown.
    failure_text = (attempt / "stderr.txt").read_text(errors="replace") + "\n" + "\n".join(
        str(event.get("message", "")) for event in events
        if event["type"] in ("turn.failed", "turn.blocked", "error"))
    infra = bool(INFRA.search(failure_text)) or (evidence_error is not None and reason is None)
    result["infrastructure_failure"] = infra
    result["status"] = (
        "inconclusive" if infra or reason else
        "success" if code == 0 and result.get("canonical_success") and verification["passed"]
                     and intact and contract_intact and result.get("tool_calls", 17) <= 16 else "failed"
    )
    result.setdefault("canonical_outcome", None)
    result.setdefault("compaction_boundaries", [])
    result.setdefault("tool_calls", None)
    result.setdefault("usage", None)
    result.setdefault("priced_cost_usd", None)
    result.setdefault("clearing_engaged", None)
    (attempt / "result.json").write_text(json.dumps(result, indent=2))
    return result


def provenance(binary):
    # Explicit scope excludes owner TODO and ignored/session/credential files.
    names = subprocess.check_output(
        ["git", "ls-files", "src", "crates", "Cargo.toml", "Cargo.lock", "build.rs", "scripts/evals"],
        cwd=REPO, text=True).splitlines()
    hashes = file_hashes(REPO, names)
    return {"head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
            "source_sha256": digest(json.dumps(hashes, sort_keys=True).encode()),
            "source_hashes": hashes, "binary_sha256": digest(binary.read_bytes()),
            "runner_sha256": digest(Path(__file__).read_bytes()),
            "base_overrides": BASE_OVERRIDES, "deadline_seconds": 120, "attempt_limit": 10,
            "tool_call_limit": 16, "budget_threshold_usd": 0.25,
            "provider": "openai", "model": "gpt-6-luna", "reasoning_effort": "medium"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=REPO / "target/debug/vtcode")
    parser.add_argument("--output", type=Path, required=True, help="New artifact directory; never overwritten")
    parser.add_argument("--live", action="store_true", help="Run up to ten billable attempts; otherwise freeze fixtures only")
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    report = {"provenance": provenance(binary), "attempts": [], "stopped": None,
              "interpretation": "Ten attempts are diagnostic, not statistical proof. Unengaged runs cannot establish retention benefits."}
    report_path = output / "report.json"
    report_path.write_text(json.dumps(report, indent=2))
    # Freeze fixture sources before dispatch.
    for case in CASES:
        (output / f"{case}.json").write_text(json.dumps(fixture(case), indent=2))
    if not args.live:
        print(f"Frozen fixtures and provenance: {report_path}")
        return
    if not os.environ.get("OPENAI_API_KEY"):
        report["stopped"] = "missing_openai_api_key"
        report_path.write_text(json.dumps(report, indent=2))
        print(f"Pilot inconclusive: OPENAI_API_KEY is unavailable. Artifacts: {report_path}")
        return
    # Keep credentials only in the child environment; isolate configuration/cache,
    # preserve HOME rather than changing the user's system identity.
    env = {key: os.environ[key] for key in ("PATH", "HOME", "OPENAI_API_KEY", "SSL_CERT_FILE", "SSL_CERT_DIR")
           if key in os.environ}
    env.update(VTCODE_CONFIG=str(output / "config"), VTCODE_DATA=str(output / "data"),
               VTCODE_HOME=str(output / "legacy"), USER=os.environ.get("USER", "eval"),
               XDG_CONFIG_HOME=str(output / "config"), XDG_CACHE_HOME=str(output / "cache"),
               XDG_DATA_HOME=str(output / "data"), XDG_STATE_HOME=str(output / "state"),
               NO_COLOR="1", VTCODE_TRUST_WORKSPACE="full-auto")
    for index, case in enumerate(CASES):
        for variant in ((0, 2) if index % 2 == 0 else (2, 0)):
            # Detect edits after freezing source/binary rather than comparing mixed runs.
            if provenance(binary) != report["provenance"]:
                report["stopped"] = "source_or_binary_changed"
                break
            result = run_attempt(binary, output / f"{index + 1}-{case}-{variant}", case, variant, env)
            report["attempts"].append(result)
            report_path.write_text(json.dumps(report, indent=2))
            print(f"{case} batches={variant}: {result['status']}, clearing={result['clearing_engaged']}", flush=True)
            if result["infrastructure_failure"]:
                report["stopped"] = "authentication_or_infrastructure_failure"
                break
        if report["stopped"]:
            break
    report_path.write_text(json.dumps(report, indent=2))
    print(f"Artifacts: {report_path}")


if __name__ == "__main__":
    main()
