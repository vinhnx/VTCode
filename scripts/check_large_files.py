#!/usr/bin/env python3
"""Fail the build when tracked files exceed an allowed size budget.

Usage: python scripts/check_large_files.py [threshold_bytes]
"""
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path
from typing import List


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate tracked files stay below the configured byte threshold.",
    )
    parser.add_argument(
        "threshold",
        nargs="?",
        type=int,
        default=400_000,
        help="Maximum allowed file size in bytes (default: 400000).",
    )
    parser.add_argument(
        "--allow",
        action="append",
        default=[],
        help=(
            "Glob patterns to allow even when above the threshold. "
            "May be specified multiple times."
        ),
    )
    return parser.parse_args()


def list_tracked_files() -> List[Path]:
    repo_root = Path(__file__).resolve().parent.parent
    proc = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=repo_root,
        check=True,
        capture_output=True,
    )
    raw = proc.stdout.decode("utf-8", errors="ignore")
    paths = [repo_root / Path(p) for p in raw.split("\0") if p]
    return paths


def is_allowed(path: Path, allow_patterns: List[str]) -> bool:
    return any(path.match(pattern) for pattern in allow_patterns)


def main() -> int:
    args = parse_args()
    threshold = args.threshold
    allow_patterns = args.allow

    oversized: List[str] = []
    for path in list_tracked_files():
        try:
            size = path.stat().st_size
        except FileNotFoundError:
            # File removed since ls-files snapshot; ignore.
            continue
        if size > threshold and not is_allowed(path, allow_patterns):
            oversized.append(f"{path.relative_to(path.parents[1])}: {size} bytes")

    if oversized:
        joined = "\n".join(oversized)
        print(
            "The following tracked files exceed the allowed size threshold "
            f"of {threshold} bytes:\n{joined}",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
