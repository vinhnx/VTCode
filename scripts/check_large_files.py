#!/usr/bin/env python3
"""Fail the build when tracked files exceed an allowed size budget.

Usage: python scripts/check_large_files.py [threshold_bytes]

Allowlist entries may optionally append `=max_bytes` to specify a custom
limit for matching files while still enforcing the global threshold for others.
"""
from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from typing import Iterable, List, Optional


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


@dataclass
class AllowRule:
    pattern: str
    max_size: Optional[int] = None


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


def normalize_pattern(pattern: str) -> str:
    normalized = pattern.strip()
    if normalized.startswith("./"):
        normalized = normalized[2:]

    repo_prefix = f"{REPO_ROOT.name}/"
    if normalized.startswith(repo_prefix):
        normalized = normalized[len(repo_prefix) :]

    return normalized.replace("\\", "/")


def parse_allow_rule(entry: str) -> AllowRule:
    text = entry.strip()
    if not text:
        raise ValueError("Allowlist entries must contain a pattern")

    pattern = text
    max_size: Optional[int] = None

    if "=" in text:
        pattern_text, _, size_text = text.partition("=")
        pattern = pattern_text.strip()
        size_compact = size_text.replace("_", "").strip()
        if not pattern:
            raise ValueError("Allowlist entry is missing a pattern before '='")
        if size_compact:
            try:
                max_size = int(size_compact)
            except ValueError as exc:  # pragma: no cover - defensive
                raise ValueError(
                    f"Invalid size limit '{size_text}' in allowlist entry"
                ) from exc
        else:
            raise ValueError("Allowlist entry must specify a size after '='")

    return AllowRule(pattern=pattern, max_size=max_size)


def is_allowed(path: Path, size: int, allow_rules: List[AllowRule]) -> bool:
    relative_posix = path.relative_to(REPO_ROOT).as_posix()
    absolute_posix = path.as_posix()

    for rule in allow_rules:
        normalized = normalize_pattern(rule.pattern)
        if fnmatch(relative_posix, normalized) or fnmatch(
            absolute_posix, rule.pattern.replace("\\", "/")
        ):
            if rule.max_size is None or size <= rule.max_size:
                return True

    return False


def load_allowlist(file_path: Path) -> List[AllowRule]:
    if not file_path.exists():
        return []

    rules: List[AllowRule] = []
    for raw_line in file_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        rules.append(parse_allow_rule(line))

    return rules


def resolve_allowlist_patterns(
    allow_args: Iterable[str],
    allowlist_file: Path | None,
) -> List[AllowRule]:
    patterns: List[AllowRule] = []
    for entry in allow_args:
        entry_text = entry.strip()
        if not entry_text:
            continue
        patterns.append(parse_allow_rule(entry_text))

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
    try:
        allow_rules = resolve_allowlist_patterns(args.allow, args.allowlist_file)
    except ValueError as exc:
        print(f"Invalid allowlist entry: {exc}", file=sys.stderr)
        return 2

    oversized: List[str] = []
    for path in list_tracked_files(REPO_ROOT):
        try:
            size = path.stat().st_size
        except FileNotFoundError:
            # File removed since ls-files snapshot; ignore.
            continue
        if size > threshold and not is_allowed(path, size, allow_rules):
            oversized_path = path.relative_to(REPO_ROOT)
            oversized.append(f"{oversized_path.as_posix()}: {size} bytes")

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
