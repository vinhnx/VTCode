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

echo "[perf] collecting baseline in ${OUT_JSON}"

check_ms="$(run_timed cargo_check cargo check --workspace --quiet)"
core_bench_ms="$(run_timed bench_core cargo bench -p vtcode-core --bench tool_pipeline -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"
tools_bench_ms="$(run_timed bench_tools cargo bench -p vtcode-tools --bench cache_bench -- --sample-size 20 --warm-up-time 0.5 --measurement-time 1)"

startup_ms=""
startup_src=""
if command -v hyperfine >/dev/null 2>&1; then
  if [[ ! -x "${ROOT_DIR}/target/debug/vtcode" ]]; then
    (cd "${ROOT_DIR}" && cargo build --quiet)
  fi
  startup_src="hyperfine"
  (cd "${ROOT_DIR}" && hyperfine --warmup 2 --runs 8 --export-json "${OUT_DIR}/${LABEL}-startup.json" "./target/debug/vtcode --version" >/dev/null)
  startup_ms="$(python3 - <<PY
import json
from pathlib import Path
p = Path(r"${OUT_DIR}/${LABEL}-startup.json")
data = json.loads(p.read_text())
print(round(data["results"][0]["mean"] * 1000.0, 3))
PY
)"
else
  startup_src="single-run"
  startup_ms="$(run_timed startup cargo run --quiet -- --version)"
fi

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
    },
    "startup_source": r"${startup_src}",
}
with open(r"${OUT_JSON}", "w", encoding="utf-8") as f:
    json.dump(out, f, indent=2, sort_keys=True)
print(f"[perf] wrote {r'${OUT_JSON}'}")
PY
