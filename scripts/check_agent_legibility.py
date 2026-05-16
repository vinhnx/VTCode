#!/usr/bin/env python3
"""Report or enforce agent legibility signals for active hotspot modules."""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
HEADER_SCAN_LINES = 80
DELEGATION_THRESHOLD = 1200
HEADER_FIELDS = (
    "Agent Legibility:",
    "Entrypoint:",
    "Common changes:",
    "Constraints:",
    "Verify:",
)
TOP_LEVEL_MOD_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+;")


@dataclass(frozen=True)
class LegibilityTarget:
    path: str
    label: str
    delegation_threshold: int = DELEGATION_THRESHOLD


TARGETS = (
    LegibilityTarget(
        path="src/agent/runloop/unified/session_setup/ui.rs",
        label="session setup ui",
    ),
    LegibilityTarget(
        path="src/agent/runloop/unified/turn/context.rs",
        label="turn context",
    ),
    LegibilityTarget(
        path="src/agent/runloop/unified/turn/turn_processing/plan_mode.rs",
        label="plan mode",
    ),
    LegibilityTarget(
        path="src/agent/runloop/unified/turn/tool_outcomes/execution_result.rs",
        label="execution result",
    ),
    LegibilityTarget(
        path="src/agent/runloop/unified/turn/session/slash_commands/diagnostics/memory.rs",
        label="diagnostics memory",
    ),
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate agent legibility signals for active hotspot modules.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail when required signals are missing.",
    )
    parser.add_argument(
        "--report-json",
        type=Path,
        default=None,
        help="Optional path to write a JSON summary.",
    )
    return parser.parse_args()


def count_lines(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        return sum(1 for _ in handle)


def read_lines(path: Path) -> list[str]:
    return path.read_text(encoding="utf-8", errors="ignore").splitlines()


def missing_header_fields(lines: list[str]) -> list[str]:
    header_window = "\n".join(lines[:HEADER_SCAN_LINES])
    return [field for field in HEADER_FIELDS if field not in header_window]


def has_delegation_signal(path: Path, lines: list[str]) -> bool:
    companion_dir = path.with_suffix("")
    if companion_dir.is_dir() and any(child.suffix == ".rs" for child in companion_dir.rglob("*.rs")):
        return True

    return any(TOP_LEVEL_MOD_RE.match(line) for line in lines[:HEADER_SCAN_LINES])


def evaluate_target(target: LegibilityTarget) -> dict[str, Any]:
    path = REPO_ROOT / target.path
    if not path.exists():
        return {
            "path": target.path,
            "label": target.label,
            "exists": False,
            "lines": 0,
            "missing_header_fields": list(HEADER_FIELDS),
            "delegation_required": False,
            "has_delegation_signal": False,
            "status": "missing-file",
        }

    lines = read_lines(path)
    line_count = count_lines(path)
    missing_fields = missing_header_fields(lines)
    delegation_required = line_count > target.delegation_threshold
    delegation_signal = has_delegation_signal(path, lines)

    status = "ok"
    if missing_fields:
        status = "missing-header"
    elif delegation_required and not delegation_signal:
        status = "needs-delegation"

    return {
        "path": target.path,
        "label": target.label,
        "exists": True,
        "lines": line_count,
        "missing_header_fields": missing_fields,
        "delegation_required": delegation_required,
        "has_delegation_signal": delegation_signal,
        "status": status,
    }


def header_status(result: dict[str, Any]) -> str:
    missing = result["missing_header_fields"]
    if not missing:
        return "ok"
    return "missing " + ", ".join(field.rstrip(":") for field in missing)


def delegation_status(result: dict[str, Any]) -> str:
    if not result["delegation_required"]:
        return "not-needed"
    if result["has_delegation_signal"]:
        return "ok"
    return "missing support-module signal"


def emit_report(results: list[dict[str, Any]], mode: str) -> None:
    missing_headers = sum(1 for result in results if result["missing_header_fields"])
    missing_delegation = sum(
        1
        for result in results
        if result["delegation_required"] and not result["has_delegation_signal"]
    )
    print(
        "Agent legibility hotspot summary: "
        f"targets={len(results)}, missing_headers={missing_headers}, "
        f"delegation_gaps={missing_delegation}, mode={mode}"
    )
    print("\n| File | Lines | Header | Delegation |")
    print("| --- | ---: | --- | --- |")
    for result in results:
        print(
            f"| {result['path']} | {result['lines']} | {header_status(result)} | {delegation_status(result)} |"
        )


def write_report(path: Path, payload: dict[str, Any]) -> None:
    report_path = path if path.is_absolute() else REPO_ROOT / path
    report_path.parent.mkdir(parents=True, exist_ok=True)
    report_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def main() -> int:
    args = parse_args()
    results = [evaluate_target(target) for target in TARGETS]
    emit_report(results, args.mode)

    failing = [
        result
        for result in results
        if result["missing_header_fields"]
        or (result["delegation_required"] and not result["has_delegation_signal"])
    ]

    if args.report_json is not None:
        write_report(
            args.report_json,
            {
                "mode": args.mode,
                "targets": [asdict(target) for target in TARGETS],
                "results": results,
            },
        )

    if not failing:
        return 0

    remediation = (
        "\nRemediation:\n"
        "1. Add a top-of-file Agent Legibility header with Entrypoint, Common changes, Constraints, and Verify markers.\n"
        "2. For oversized roots, extract helper clusters into responsibility-named support modules or directories.\n"
        "3. Re-run: python3 scripts/check_agent_legibility.py"
    )

    if args.mode == "enforce":
        print(
            "\nAgent legibility enforcement failed." + remediation,
            file=sys.stderr,
        )
        return 1

    print(
        "\nWarning mode: agent legibility gaps reported but not blocking." + remediation
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())