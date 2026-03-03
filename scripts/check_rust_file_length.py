#!/usr/bin/env python3
"""Report or enforce Rust file length limits for VT Code governance."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate Rust source file length budgets.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail if files exceed --max-lines.",
    )
    parser.add_argument(
        "--max-lines",
        type=int,
        default=500,
        help="Maximum allowed line count in enforce mode (default: 500).",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=25,
        help="Number of longest files to print (default: 25).",
    )
    parser.add_argument(
        "--report-json",
        type=Path,
        default=None,
        help="Optional path to write JSON summary.",
    )
    return parser.parse_args()


def list_tracked_rust_files() -> list[Path]:
    proc = subprocess.run(
        ["git", "ls-files", "-z", "*.rs"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
    )
    raw = proc.stdout.decode("utf-8", errors="ignore")
    files = [REPO_ROOT / Path(rel) for rel in raw.split("\0") if rel]
    return files


def count_lines(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        return sum(1 for _ in handle)


def emit_report(summary: dict[str, Any], top_items: list[dict[str, Any]], top_count: int) -> None:
    print(
        "Rust file length summary: "
        f"total={summary['total_files']}, "
        f">{summary['max_lines']}={summary['over_max']}, "
        f">1000={summary['over_1000']}, "
        f">1500={summary['over_1500']}"
    )
    if not top_items:
        return

    print(f"\nTop {min(top_count, len(top_items))} longest tracked Rust files:")
    print("| Lines | Path |")
    print("| ---: | --- |")
    for item in top_items[:top_count]:
        print(f"| {item['lines']} | {item['path']} |")


def main() -> int:
    args = parse_args()
    files = list_tracked_rust_files()

    entries: list[dict[str, Any]] = []
    for path in files:
        lines = count_lines(path)
        entries.append(
            {
                "path": path.relative_to(REPO_ROOT).as_posix(),
                "lines": lines,
            }
        )

    entries.sort(key=lambda item: item["lines"], reverse=True)

    over_max = [item for item in entries if item["lines"] > args.max_lines]
    over_1000 = [item for item in entries if item["lines"] > 1000]
    over_1500 = [item for item in entries if item["lines"] > 1500]

    summary = {
        "mode": args.mode,
        "max_lines": args.max_lines,
        "total_files": len(entries),
        "over_max": len(over_max),
        "over_1000": len(over_1000),
        "over_1500": len(over_1500),
    }

    emit_report(summary, entries, args.top)

    if args.report_json is not None:
        report_path = args.report_json
        if not report_path.is_absolute():
            report_path = REPO_ROOT / report_path
        report_path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "summary": summary,
            "over_max": over_max,
            "over_1000": over_1000,
            "over_1500": over_1500,
        }
        report_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")

    if args.mode == "enforce" and over_max:
        print(
            "\nFile length enforcement failed.\n\n"
            "Remediation:\n"
            "1. Split oversized files into focused submodules.\n"
            "2. Preserve public API surface by re-exporting from mod.rs where needed.\n"
            "3. Re-run: python3 scripts/check_rust_file_length.py --mode enforce "
            f"--max-lines {args.max_lines}",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and over_max:
        print(
            "\nWarning mode: oversized files reported but not blocking. "
            "Promote to --mode enforce after baseline cleanup."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
