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
baseline_startup_source = baseline.get("startup_source", "unknown")
current_startup_source = current.get("startup_source", "unknown")
baseline_io_source = baseline.get("first_user_io_source", "unknown")
current_io_source = current.get("first_user_io_source", "unknown")
baseline_cold_source = baseline.get("cold_startup_source", "unknown")
current_cold_source = current.get("cold_startup_source", "unknown")
baseline_interactive_source = baseline.get("interactive_first_render_source", "unknown")
current_interactive_source = current.get("interactive_first_render_source", "unknown")
baseline_environment = baseline.get("startup_environment", "unknown")
current_environment = current.get("startup_environment", "unknown")

keys = [
    "release_build_ms",
    "cargo_check_ms",
    "tool_pipeline_bench_ms",
    "agent_harness_bench_ms",
    "release_binary_bytes",
    "cold_startup_ms",
    "warm_startup_ms",
    "startup_ms",
    "first_user_io_ms",
    "interactive_first_render_ms",
]
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
    if key in {"release_build_ms", "cargo_check_ms", "tool_pipeline_bench_ms", "agent_harness_bench_ms"}:
        direction = "command wall time (includes build/setup)"
    elif key == "release_binary_bytes":
        direction = "smaller" if pct < 0 else "larger" if pct > 0 else "no change"
    else:
        direction = "faster" if pct < 0 else "slower" if pct > 0 else "no change"
    rows.append((key, b, c, pct, direction))

lines = [
    "# VT Code Performance Diff",
    "",
    f"Baseline: `{baseline_path}`",
    f"Current: `{current_path}`",
    f"Baseline startup source: `{baseline_startup_source}`",
    f"Current startup source: `{current_startup_source}`",
    f"Baseline first_user_io source: `{baseline_io_source}`",
    f"Current first_user_io source: `{current_io_source}`",
    f"Baseline cold startup source: `{baseline_cold_source}`",
    f"Current cold startup source: `{current_cold_source}`",
    f"Baseline interactive source: `{baseline_interactive_source}`",
    f"Current interactive source: `{current_interactive_source}`",
    f"Baseline startup environment: `{baseline_environment}`",
    f"Current startup environment: `{current_environment}`",
    "",
    "| Metric | Baseline | Current | Delta | Interpretation |",
    "|---|---:|---:|---:|---|",
]
for key, b, c, pct, direction in rows:
    lines.append(f"| `{key}` | {b} | {c} | {pct:+.2f}% | {direction} |")

if baseline_startup_source != current_startup_source:
    lines.extend(
        [
            "",
            "> Warning: startup sources differ, so `startup_ms` is not directly comparable.",
        ]
    )

if baseline_io_source != current_io_source:
    lines.extend(
        [
            "",
            "> Warning: first_user_io sources differ, so `first_user_io_ms` is not directly comparable.",
        ]
    )

if baseline_cold_source != current_cold_source:
    lines.extend(
        [
            "",
            "> Warning: cold startup sources differ, so `cold_startup_ms` is not directly comparable.",
        ]
    )

if baseline_interactive_source != current_interactive_source:
    lines.extend(
        [
            "",
            "> Warning: interactive sources differ, so `interactive_first_render_ms` is not directly comparable.",
        ]
    )

if baseline_environment != current_environment:
    lines.extend(["", "> Warning: startup environments differ; historical startup measurements may not be comparable."])

out_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
print("\n".join(lines))
print(f"\n[perf] wrote {out_path}")
PY
