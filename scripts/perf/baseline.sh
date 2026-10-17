#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/.vtcode/perf"
mkdir -p "${OUT_DIR}"

LABEL="${1:-latest}"
OUT_JSON="${OUT_DIR}/${LABEL}.json"
RUN_DIR="$(mktemp -d "${TMPDIR:-/tmp}/vtcode-perf.XXXXXX")"
RELEASE_BIN="${ROOT_DIR}/target/release/vtcode"
# These metrics are assigned by measure_command through printf -v.
warm_startup_ms=""
warm_startup_src=""
first_user_io_ms=""
first_user_io_src=""

cleanup() {
  rm -rf "${RUN_DIR}"
}
trap cleanup EXIT

run_timed() {
  local name="$1"
  shift
  local log_file="${OUT_DIR}/${LABEL}-${name}.log"
  local time_file="${OUT_DIR}/${LABEL}-${name}.time"
  (cd "${ROOT_DIR}" && /usr/bin/time -p -o "${time_file}" "$@" >"${log_file}" 2>&1) || return $?
  awk '/^real / { printf "%.0f\n", $2 * 1000.0 }' "${time_file}"
}

run_cargo_timed() {
  local name="$1"
  shift
  if [[ "${PERF_KEEP_RUSTC_WRAPPER:-0}" == "1" ]]; then
    run_timed "${name}" cargo "$@"
  else
    run_timed "${name}" env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= cargo "$@"
  fi
}

ensure_release_binary() {
  echo "[perf] rebuilding target/release/vtcode for startup measurement"
  release_build_ms="$(run_cargo_timed release_build build --release --locked --bin vtcode)" || return $?
}

# Measure a command's mean warm latency (2 warmups + 8 measured runs).
# Usage: measure_command <metric_name> <command> [args...]
measure_command() {
  local name="$1"
  shift
  local json_path="${OUT_DIR}/${LABEL}-${name}.json"
  local log_path="${OUT_DIR}/${LABEL}-${name}.log"
  local ms

  ms="$(python3 - "${ROOT_DIR}/scripts/perf" "${RUN_DIR}/${name}" "${json_path}" "${log_path}" "$@" <<'PY'
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from startup_env import isolated_environment

fixture_root = Path(sys.argv[2]).resolve()
environment = isolated_environment(fixture_root)
json_path = Path(sys.argv[3])
log_path = Path(sys.argv[4])
command = sys.argv[5:]
warmup_runs = []
measured_runs = []

for _ in range(2):
    started = time.perf_counter()
    subprocess.run(command, check=True, env=environment, cwd=fixture_root / "workspace", stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    warmup_runs.append((time.perf_counter() - started) * 1000.0)

for _ in range(8):
    started = time.perf_counter()
    subprocess.run(command, check=True, env=environment, cwd=fixture_root / "workspace", stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    measured_runs.append((time.perf_counter() - started) * 1000.0)

summary = {
    "command": command,
    "warmup_runs_ms": [round(value, 3) for value in warmup_runs],
    "runs_ms": [round(value, 3) for value in measured_runs],
    "mean_ms": round(statistics.mean(measured_runs), 3),
    "min_ms": round(min(measured_runs), 3),
    "max_ms": round(max(measured_runs), 3),
    "p95_ms": round(statistics.quantiles(measured_runs, n=20)[-1], 3),
}
json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
log_path.write_text(
    "\n".join(
        [
            f"mean {summary['mean_ms']}",
            f"min {summary['min_ms']}",
            f"max {summary['max_ms']}",
            f"p95 {summary['p95_ms']}",
        ]
    )
    + "\n",
    encoding="utf-8",
)
print(summary["mean_ms"])
PY
  )"
  printf -v "${name}_ms" '%s' "${ms}"
  printf -v "${name}_src" '%s' 'release-python-mean'
}

measure_cold_startup() {
  local json_path="${OUT_DIR}/${LABEL}-cold_startup.json"
  local log_path="${OUT_DIR}/${LABEL}-cold_startup.log"
  local ms

  ms="$(python3 - "${ROOT_DIR}/scripts/perf" "${RELEASE_BIN}" "${RUN_DIR}" "${json_path}" "${log_path}" <<'PY'
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from startup_env import isolated_environment

binary = Path(sys.argv[2])
run_dir = Path(sys.argv[3])
json_path = Path(sys.argv[4])
log_path = Path(sys.argv[5])
runs = []

for index in range(3):
    cold_dir = run_dir / f"cold-{index}"
    cold_dir.mkdir()
    environment = isolated_environment(cold_dir / "environment")
    cold_binary = cold_dir / "vtcode"
    shutil.copy2(binary, cold_binary)
    cold_binary.chmod(0o755)
    started = time.perf_counter()
    subprocess.run(
        [str(cold_binary), "--version"],
        check=True,
        env=environment,
        cwd=cold_dir / "environment/workspace",
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    runs.append((time.perf_counter() - started) * 1000.0)

summary = {
    "command": [str(binary), "--version"],
    "runs_ms": [round(value, 3) for value in runs],
    "mean_ms": round(sum(runs) / len(runs), 3),
    "min_ms": round(min(runs), 3),
    "max_ms": round(max(runs), 3),
}
json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
log_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print(summary["mean_ms"])
PY
  )"
  cold_startup_ms="${ms}"
  cold_startup_src='release-fresh-copy-mean'
}

measure_interactive_first_render() {
  local json_path="${OUT_DIR}/${LABEL}-interactive_first_render.json"
  local log_path="${OUT_DIR}/${LABEL}-interactive_first_render.log"
  local ms

  ms="$(python3 - "${ROOT_DIR}/scripts/perf" "${RELEASE_BIN}" "${RUN_DIR}" "${json_path}" "${log_path}" <<'PY'
import json
import fcntl
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
from pathlib import Path

sys.path.insert(0, sys.argv[1])
from startup_env import isolated_environment

binary = sys.argv[2]
run_dir = Path(sys.argv[3])
json_path = Path(sys.argv[4])
log_path = Path(sys.argv[5])
log_path.write_bytes(b"")


def respond_to_terminal_queries(fd, output):
    replies = []
    if b"\x1b]10;?" in output:
        replies.append(b"\x1b]10;rgb:ffff/ffff/ffff\x07")
    if b"\x1b]11;?" in output:
        replies.append(b"\x1b]11;rgb:0000/0000/0000\x07")
    if b"\x1b[6n" in output:
        replies.append(b"\x1b[1;1R")
    if b"\x1b[18t" in output:
        replies.append(b"\x1b[8;24;80t")
    if b"\x1b[>c" in output or b"\x1b[c" in output:
        replies.append(b"\x1b[?1;2c")
    if b"\x1b[?u" in output:
        replies.append(b"\x1b[?1u")
    for reply in replies:
        os.write(fd, reply)


def run_sample(index):
    fixture_root = (run_dir / f"pty-{index}").resolve()
    environment = isolated_environment(fixture_root)
    workspace = fixture_root / "workspace"

    started = time.perf_counter()
    pid, fd = pty.fork()
    if pid == 0:
        fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        os.chdir(workspace)
        os.execve(binary, [binary, "--provider", "ollama", "--model", "llama3"], environment)

    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))

    output = bytearray()
    first_render_ms = None
    child_reaped = False

    def terminate_child():
        nonlocal child_reaped, fd
        if child_reaped:
            return
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        # Close the PTY before reaping: macOS can drain terminal output during exit.
        os.close(fd)
        fd = -1
        try:
            os.waitpid(pid, 0)
        except ChildProcessError:
            pass
        child_reaped = True

    deadline = started + 20.0
    try:
        while time.perf_counter() < deadline:
            waited_pid, status = os.waitpid(pid, os.WNOHANG)
            if waited_pid == pid:
                child_reaped = True
                break
            readable, _, _ = select.select([fd], [], [], 0.05)
            if not readable:
                continue
            try:
                chunk = os.read(fd, 65536)
            except OSError:
                break
            if not chunk:
                break
            output.extend(chunk)
            respond_to_terminal_queries(fd, chunk)
            if first_render_ms is None and (b"Type a request" in output or b"> VT Code" in output):
                first_render_ms = (time.perf_counter() - started) * 1000.0
                terminate_child()
                break
        if first_render_ms is None:
            terminate_child()
    finally:
        if not child_reaped:
            terminate_child()
        if fd != -1:
            os.close(fd)
        with log_path.open("ab") as log:
            log.write(f"sample {index}\n".encode() + bytes(output) + b"\n")

    if first_render_ms is None:
        raise RuntimeError("interactive UI did not render the request prompt")
    return first_render_ms


runs = [run_sample(index) for index in range(3)]
summary = {
    "command": [binary, "--provider", "ollama", "--model", "llama3"],
    "runs_ms": [round(value, 3) for value in runs],
    "mean_ms": round(sum(runs) / len(runs), 3),
    "min_ms": round(min(runs), 3),
    "max_ms": round(max(runs), 3),
}
json_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
print(summary["mean_ms"])
PY
  )"
  interactive_first_render_ms="${ms}"
  interactive_first_render_src='release-pty-prompt-mean'
}

ensure_release_binary
echo "[perf] collecting release baseline in ${OUT_JSON}"

check_ms="$(run_cargo_timed cargo_check check --workspace --quiet --locked)"
tool_pipeline_bench_ms=""
agent_harness_bench_ms=""
if [[ "${PERF_RUN_BENCHMARKS:-0}" == "1" ]]; then
  tool_pipeline_bench_ms="$(run_cargo_timed bench_tool_pipeline bench --locked -p vtcode-core --bench tool_pipeline -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"
  agent_harness_bench_ms="$(run_cargo_timed bench_agent_harness bench --locked -p vtcode-core --bench agent_harness -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"
else
  echo "[perf] skipping Criterion workloads (set PERF_RUN_BENCHMARKS=1 to enable)"
fi

measure_cold_startup
measure_command warm_startup env VTCODE_STARTUP_TRACE=0 "${RELEASE_BIN}" --version

measure_command first_user_io "${RELEASE_BIN}" tool-policy status

measure_interactive_first_render

startup_ms="${warm_startup_ms}"
startup_src="${warm_startup_src}"

release_binary_bytes="$(wc -c <"${RELEASE_BIN}" | tr -d ' ')"
python3 - <<PY
import json
from datetime import datetime, timezone

out = {
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "workspace": r"${ROOT_DIR}",
    "label": r"${LABEL}",
    "binary": r"${RELEASE_BIN}",
    "metrics": {
        "release_build_ms": int(r"${release_build_ms}"),
        "cargo_check_ms": int(r"${check_ms}"),
        "tool_pipeline_bench_ms": None if not r"${tool_pipeline_bench_ms}" else int(r"${tool_pipeline_bench_ms}"),
        "agent_harness_bench_ms": None if not r"${agent_harness_bench_ms}" else int(r"${agent_harness_bench_ms}"),
        "release_binary_bytes": int(r"${release_binary_bytes}"),
        "cold_startup_ms": float(r"${cold_startup_ms}"),
        "warm_startup_ms": float(r"${warm_startup_ms}"),
        "startup_ms": float(r"${startup_ms}"),
        "first_user_io_ms": float(r"${first_user_io_ms}"),
        "interactive_first_render_ms": float(r"${interactive_first_render_ms}"),
    },
    "startup_source": r"${startup_src}",
    "cold_startup_source": r"${cold_startup_src}",
    "first_user_io_source": r"${first_user_io_src}",
    "interactive_first_render_source": r"${interactive_first_render_src}",
    "startup_environment": "isolated-workspace-v1",
    "measurement_notes": {
        "release_build_ms": "Cargo build command wall time; never an operation latency",
        "tool_pipeline_bench_ms": "Cargo benchmark command wall time, including compilation and harness setup; operation estimates are in the Criterion log",
        "agent_harness_bench_ms": "Cargo benchmark command wall time, including compilation and harness setup; operation estimates are in the Criterion log",
    },
}
with open(r"${OUT_JSON}", "w", encoding="utf-8") as f:
    json.dump(out, f, indent=2, sort_keys=True)
print(f"[perf] wrote {r'${OUT_JSON}'}")
PY
