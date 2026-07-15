#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/.vtcode/perf"
mkdir -p "${OUT_DIR}"

LABEL="${1:-latest}"
OUT_JSON="${OUT_DIR}/${LABEL}.json"

run_timed() {
  local name="$1"
  shift
  local log_file="${OUT_DIR}/${LABEL}-${name}.log"
  local time_file="${OUT_DIR}/${LABEL}-${name}.time"
  (cd "${ROOT_DIR}" && /usr/bin/time -p -o "${time_file}" "$@" >"${log_file}" 2>&1)
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

ensure_vtcode_binary() {
  if [[ -x "${ROOT_DIR}/target/debug/vtcode" ]]; then
    return 0
  fi

  echo "[perf] building target/debug/vtcode for startup measurement"
  if [[ "${PERF_KEEP_RUSTC_WRAPPER:-0}" == "1" ]]; then
    (cd "${ROOT_DIR}" && cargo build --quiet --bin vtcode)
  else
    (cd "${ROOT_DIR}" && env CARGO_BUILD_RUSTC_WRAPPER= RUSTC_WRAPPER= cargo build --quiet --bin vtcode)
  fi
}

echo "[perf] collecting baseline in ${OUT_JSON}"

check_ms="$(run_cargo_timed cargo_check check --workspace --quiet)"
core_bench_ms="$(run_cargo_timed bench_core bench -p vtcode-core --bench tool_pipeline -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"
tools_bench_ms="$(run_cargo_timed bench_tools bench -p vtcode-tools --bench cache_bench -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"

# Measure a command's mean startup latency (warm: 2 warmups + 8 measured runs).
# Usage: measure_command <metric_name> <command> [args...]
# Sets <metric_name>_ms (float) and <metric_name>_src ("hyperfine"|"binary-mean").
measure_command() {
  local name="$1"; shift
  local json_path="${OUT_DIR}/${LABEL}-${name}.json"
  local log_path="${OUT_DIR}/${LABEL}-${name}.log"
  local time_path="${OUT_DIR}/${LABEL}-${name}.time"
  local src ms
  ensure_vtcode_binary
  if command -v hyperfine >/dev/null 2>&1; then
    src="hyperfine"
    ( cd "${ROOT_DIR}" && hyperfine --warmup 2 --runs 8 --export-json "${json_path}" "$@" >/dev/null )
    ms="$(python3 - "$json_path" <<'PY'
import json, sys
from pathlib import Path
print(round(json.loads(Path(sys.argv[1]).read_text())["results"][0]["mean"] * 1000.0, 3))
PY
)"
  else
    src="binary-mean"
    ms="$(
      MCMD_CMD="$*" \
      MCMD_JSON="${json_path}" \
      MCMD_LOG="${log_path}" \
      MCMD_TIME="${time_path}" \
      python3 - <<'PY'
import json
import os
import statistics
import subprocess
import time
from pathlib import Path

cmd = os.environ.get("MCMD_CMD", "").split()
log_path = Path(os.environ["MCMD_LOG"])
time_path = Path(os.environ["MCMD_TIME"])
warmup_runs = []
measured_runs = []
for _ in range(2):
    started = time.perf_counter()
    subprocess.run(
        cmd,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    warmup_runs.append((time.perf_counter() - started) * 1000.0)

for _ in range(8):
    started = time.perf_counter()
    subprocess.run(
        cmd,
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    measured_runs.append((time.perf_counter() - started) * 1000.0)

summary = {
    "warmup_runs_ms": [round(value, 3) for value in warmup_runs],
    "runs_ms": [round(value, 3) for value in measured_runs],
    "mean_ms": round(statistics.mean(measured_runs), 3),
    "min_ms": round(min(measured_runs), 3),
    "max_ms": round(max(measured_runs), 3),
    "p95_ms": round(statistics.quantiles(measured_runs, n=20)[-1], 3),
}
log_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
time_path.write_text(
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
  fi
  printf -v "${name}_ms" '%s' "$ms"
  printf -v "${name}_src" '%s' "$src"
}

# startup_ms: clap-only path — bypasses from_cli_args (binary load + parse only).
# Stable signal for binary/loader cost, NOT a proxy for the optimization plan.
measure_command startup "${ROOT_DIR}/target/debug/vtcode" --version
# first_user_io_ms: real from_cli_args path (local, no network). Exercises config
# load, dotfolder init, guardian init, theme resolution, and auth resolution —
# i.e. the startup work the optimization plan actually targets.
measure_command first_user_io "${ROOT_DIR}/target/debug/vtcode" auth openai

startup_ms="${startup_ms}"
startup_src="${startup_src}"
first_user_io_ms="${first_user_io_ms}"
first_user_io_src="${first_user_io_src}"

python3 - <<PY
import json
from datetime import datetime, timezone

out = {
    "timestamp_utc": datetime.now(timezone.utc).isoformat(),
    "workspace": r"${ROOT_DIR}",
    "label": r"${LABEL}",
    "metrics": {
        "cargo_check_ms": int(r"${check_ms}"),
        "core_bench_ms": int(r"${core_bench_ms}"),
        "tools_bench_ms": int(r"${tools_bench_ms}"),
        "startup_ms": float(r"${startup_ms}"),
        "first_user_io_ms": float(r"${first_user_io_ms}"),
    },
    "startup_source": r"${startup_src}",
    "first_user_io_source": r"${first_user_io_src}",
}
with open(r"${OUT_JSON}", "w", encoding="utf-8") as f:
    json.dump(out, f, indent=2, sort_keys=True)
print(f"[perf] wrote {r'${OUT_JSON}'}")
PY
