#!/usr/bin/env python3
"""Fresh-workspace out-of-box E2E: first-run wizard (PTY) + exec smoke (live model).

Standard library only. Exit 0 on pass-or-skip; exit 1 on real failures.
Live phases degrade to `stopped=no_auth_or_infra` (exit 0) when credentials,
network, or the model backend are unavailable, mirroring
scripts/evals/recent_tool_batches.py.
"""
import argparse
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import subprocess
import sys
import tempfile
import time

REPO = Path(__file__).resolve().parents[2]
WIZARD_BANNER = "First-time setup wizard"
WIZARD_SAVED = "Saving your configuration"
# Markers that prove a permission prompt or denial happened (must stay absent
# under --skip-confirmations with an approving policy).
PROMPT_MARKERS = re.compile(r"needsapproval|needs_approval|permission denied|denied by policy", re.I)
# Infrastructure failures (auth/network/rate-limit/model) downgrade to skip.
INFRA = re.compile(
    r"invalid_api_key|incorrect api key|authentication|unauthorized|\b401\b|\b403\b|"
    r"\b429\b|rate.limit|connection refused|dns|timed out connecting|"
    r"model.*(?:not found|does not exist|not available)|"
    r"network.*(?:error|unreachable)|error sending request|\b50[0234]\b|"
    r"api key not found|no credentials found|no authenticated fallback",
    re.I,
)


def isolated_child_env(config_dir, data_dir, extra=None):
    """Minimal child env: real PATH/HOME (keyring stays reachable), isolated
    VT Code config/state, temp HOME-independent credential discovery."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", str(Path.home())),
        "USER": os.environ.get("USER", "e2e"),
        "TERM": "xterm-256color",
        "LANG": os.environ.get("LANG", "C.UTF-8"),
        "VTCODE_CONFIG": str(config_dir),
        "VTCODE_DATA": str(data_dir),
    }
    if extra:
        env.update(extra)
    return env


def run_wizard_phase(binary, workspace, config_dir, data_dir, transcript_path, timeout=150):
    """Drive the interactive first-run wizard under a PTY, accepting every
    default with Enter. A dummy OpenAI key makes provider discovery
    deterministic without touching real credentials (the wizard never calls
    the model). Returns (ok, detail)."""
    env = isolated_child_env(config_dir, data_dir, {"OPENAI_API_KEY": "e2e-dummy-key-never-sent"})
    workspace.mkdir(parents=True, exist_ok=True)
    pid, fd = pty.fork()
    if pid == 0:
        os.chdir(str(workspace))
        os.execvpe(str(binary), [str(binary)], env)
    output = bytearray()
    deadline = time.time() + timeout
    vtcode_toml = workspace / "vtcode.toml"
    dot_config = config_dir / "config.toml"
    try:
        while time.time() < deadline:
            ready, _, _ = select.select([fd], [], [], 1.0)
            if ready:
                try:
                    chunk = os.read(fd, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                output.extend(chunk)
            text = bytes(output).decode("utf-8", "replace")
            # Trust is persisted after the config save; require a real record
            # (`trusted_at` only exists on written records, unlike the
            # always-present empty table header).
            trust_done = dot_config.exists() and "trusted_at" in dot_config.read_text()
            if WIZARD_SAVED in text and vtcode_toml.exists() and trust_done:
                return finish_wizard(pid, fd, output, transcript_path, workspace, config_dir, "")
            try:
                # CR, not LF: ratatui raw-mode selections confirm on Enter
                # (\r), while line-discipline text prompts translate it via
                # ICRNL — so \r advances both prompt styles.
                os.write(fd, b"\r")
            except OSError:
                break
        return finish_wizard(pid, fd, output, transcript_path, workspace, config_dir, "timeout")
    finally:
        try:
            os.close(fd)
        except OSError:
            pass


def finish_wizard(pid, fd, output, transcript_path, workspace, config_dir, error):
    transcript_path.write_bytes(bytes(output))
    text = bytes(output).decode("utf-8", "replace")
    try:
        os.kill(pid, signal.SIGTERM)
        time.sleep(1.0)
        os.kill(pid, signal.SIGKILL)
    except (OSError, ProcessLookupError):
        pass
    try:
        os.waitpid(pid, os.WNOHANG)
    except (OSError, ChildProcessError):
        pass
    vtcode_toml = workspace / "vtcode.toml"
    dot_config = config_dir / "config.toml"
    checks = {
        "banner_shown": WIZARD_BANNER in text,
        "config_written": vtcode_toml.exists(),
        "trust_recorded": dot_config.exists() and "trusted_at" in dot_config.read_text(),
    }
    provider = model = None
    if vtcode_toml.exists():
        content = vtcode_toml.read_text()
        for line in content.splitlines():
            s = line.strip()
            if s.startswith("provider"):
                provider = s.split("=", 1)[1].strip().strip('"')
            if s.startswith("default_model"):
                model = s.split("=", 1)[1].strip().strip('"')
    ok = error == "" and all(checks.values())
    detail = {"checks": checks, "provider": provider, "model": model,
              "transcript": str(transcript_path)}
    if error:
        detail["error"] = error
    return ok, detail


def run_smoke_attempt(binary, workspace, config_dir, data_dir, events_path, prompt, extra_flags, timeout):
    env = isolated_child_env(config_dir, data_dir, {"VTCODE_TRUST_WORKSPACE": "full-auto"})
    for key in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY"):
        if key in os.environ:
            env[key] = os.environ[key]
    cmd = [str(binary), "exec", "--workspace", str(workspace), "--skip-confirmations",
           "--max-tool-calls", "12", "--events", str(events_path), *extra_flags, prompt]
    try:
        proc = subprocess.run(cmd, env=env, cwd=str(workspace), capture_output=True,
                              text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        return "fail", {"error": "exec timeout", "timeout": timeout}
    combined = (proc.stdout or "") + "\n" + (proc.stderr or "")
    if INFRA.search(combined):
        return "skipped", {"stopped": "no_auth_or_infra",
                           "hint": combined.strip().splitlines()[-1:] if combined.strip() else []}
    artifact = workspace / "hello.txt"
    artifact_ok = artifact.exists() and artifact.read_text().strip() == "Hello Luna"
    events_text = events_path.read_text() if events_path.exists() else ""
    prompt_hits = PROMPT_MARKERS.findall(events_text + combined)
    tool_calls = len(re.findall(r'"tool_name"\s*:\s*"exec_command"', events_text))
    detail = {"exit_code": proc.returncode, "artifact_ok": artifact_ok,
              "prompt_markers": sorted(set(prompt_hits)), "exec_calls": tool_calls}
    if artifact_ok and not prompt_hits and tool_calls >= 1:
        return "pass", detail
    detail["error"] = "smoke assertions failed"
    return "fail", detail


def run_smoke_phase(binary, workspace, config_dir, data_dir, events_path, timeout=300):
    """Run the permission smoke task headless. First attempt uses the
    wizard-written config with no flags (proves the wizard output suffices);
    if that hits missing auth (provider discovery is credential-order
    dependent), retry with explicit OpenAI flags so the permission assertions
    still execute. Returns (verdict, detail)."""
    prompt = ("In this workspace: 1) Create hello.txt containing exactly 'Hello Luna' "
              "using a shell command (not the patch tool). 2) Show its contents with cat. "
              "3) Report the file size in bytes.")
    verdict, detail = run_smoke_attempt(
        binary, workspace, config_dir, data_dir, events_path, prompt, [], timeout)
    detail["mode"] = "wizard_config"
    if verdict == "pass" or verdict == "fail":
        return verdict, detail
    explicit_events = events_path.with_name("smoke-events-explicit.jsonl")
    verdict, detail = run_smoke_attempt(
        binary, workspace, config_dir, data_dir, explicit_events, prompt,
        ["--provider", "openai", "--model", "gpt-6-luna"], timeout)
    detail["mode"] = "explicit_openai_fallback"
    return verdict, detail


def main(argv=None):
    parser = argparse.ArgumentParser(description="Fresh-workspace out-of-box E2E.")
    parser.add_argument("--binary", default=str(REPO / "target" / "debug" / "vtcode"))
    parser.add_argument("--output", default=None, help="Report directory (default: fresh temp dir).")
    parser.add_argument("--wizard-timeout", type=int, default=150)
    parser.add_argument("--exec-timeout", type=int, default=300)
    parser.add_argument("--skip-live", action="store_true", help="Skip the live-model smoke phase.")
    args = parser.parse_args(argv)

    binary = Path(args.binary)
    if not binary.exists():
        print(f"Binary not found: {binary} (build with `cargo build --bin vtcode` first)")
        return 1
    output = Path(args.output) if args.output else Path(tempfile.mkdtemp(prefix="new-workspace-e2e-"))
    output.mkdir(parents=True, exist_ok=True)
    workspace = output / "workspace"
    config_dir = output / "config"
    data_dir = output / "data"

    report = {"binary": str(binary), "phases": {}}
    print(f"[e2e] workspace={workspace}")
    wizard_ok, wizard_detail = run_wizard_phase(
        binary, workspace, config_dir, data_dir, output / "wizard-transcript.txt",
        timeout=args.wizard_timeout)
    report["phases"]["wizard"] = {"ok": wizard_ok, **wizard_detail}
    print(f"[e2e] wizard: {'PASS' if wizard_ok else 'FAIL'} {wizard_detail}")

    if args.skip_live:
        report["phases"]["smoke"] = {"verdict": "skipped", "stopped": "skip_live_flag"}
    elif not wizard_ok:
        report["phases"]["smoke"] = {"verdict": "skipped", "stopped": "wizard_failed_prereq"}
    else:
        # `exec` requires the automation capability gate; enable it in the
        # ISOLATED user config (mirrors real user configs). This does not
        # change permission semantics (no --full-auto flag is passed).
        with open(config_dir / "vtcode.toml", "a") as f:
            f.write("\n[automation.full_auto]\nenabled = true\n")
        verdict, detail = run_smoke_phase(
            binary, workspace, config_dir, data_dir, output / "smoke-events.jsonl",
            timeout=args.exec_timeout)
        report["phases"]["smoke"] = {"verdict": verdict, **detail}
        print(f"[e2e] smoke: {verdict} {detail}")

    (output / "report.json").write_text(json.dumps(report, indent=2))
    print(f"[e2e] report: {output / 'report.json'}")
    failed = (not wizard_ok or report["phases"]["smoke"].get("verdict") == "fail")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
