#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
OUT_DIR="${ROOT_DIR}/.vtcode/perf"
BASELINE="${1:-${OUT_DIR}/baseline.json}"
CURRENT="${2:-${OUT_DIR}/latest.json}"
OUT_MD="${OUT_DIR}/diff.md"

export BASELINE CURRENT OUT_MD

python3 - <<'PY'
import os
import json
from pathlib import Path

baseline_path = Path(os.environ["BASELINE"])
current_path = Path(os.environ["CURRENT"])
out_path = Path(os.environ["OUT_MD"])

baseline = json.loads(baseline_path.read_text())
current = json.loads(current_path.read_text())

keys = ["cargo_check_ms", "core_bench_ms", "tools_bench_ms", "startup_ms"]
rows = []
for key in keys:
    b = baseline["metrics"].get(key)
    c = current["metrics"].get(key)
    if b is None or c is None:
        continue
    if float(b) == 0.0:
        pct = 0.0
    else:
        pct = ((float(c) - float(b)) / float(b)) * 100.0
    direction = "faster" if pct < 0 else "slower" if pct > 0 else "no change"
    rows.append((key, b, c, pct, direction))

lines = [
    "# VT Code Performance Diff",
    "",
    f"Baseline: `{baseline_path}`",
    f"Current: `{current_path}`",
    "",
    "| Metric | Baseline | Current | Delta | Interpretation |",
    "|---|---:|---:|---:|---|",
]
for key, b, c, pct, direction in rows:
    lines.append(f"| `{key}` | {b} | {c} | {pct:+.2f}% | {direction} |")

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
print(f"\n[perf] wrote {out_path}")
PY
