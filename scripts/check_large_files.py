#!/usr/bin/env python3
"""Fail the build when tracked files exceed an allowed size budget.

Usage: python scripts/check_large_files.py [threshold_bytes]
"""
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path
from typing import Iterable, List


REPO_ROOT = Path(__file__).resolve().parent.parent


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
    parser.add_argument(
        "--allowlist-file",
        type=Path,
        default=None,
        help=(
            "Path to a newline-delimited allowlist of glob patterns. "
            "Defaults to scripts/large_file_allowlist.txt when present."
        ),
    )
    return parser.parse_args()


def list_tracked_files(repo_root: Path) -> List[Path]:
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


def load_allowlist(file_path: Path) -> List[str]:
    if not file_path.exists():
        return []

    patterns: List[str] = []
    for raw_line in file_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        patterns.append(line)

    return patterns


def resolve_allowlist_patterns(
    allow_args: Iterable[str],
    allowlist_file: Path | None,
) -> List[str]:
    patterns = list(allow_args)

    file_path = allowlist_file
    if file_path is None:
        default_path = REPO_ROOT / "scripts" / "large_file_allowlist.txt"
        file_path = default_path
    elif not file_path.is_absolute():
        file_path = REPO_ROOT / file_path

    patterns.extend(load_allowlist(file_path))
    return patterns


def main() -> int:
    args = parse_args()
    threshold = args.threshold
    allow_patterns = resolve_allowlist_patterns(args.allow, args.allowlist_file)

    oversized: List[str] = []
    for path in list_tracked_files(REPO_ROOT):
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
