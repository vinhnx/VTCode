#!/usr/bin/env python3
"""Validate Zen governance allowlist hygiene (explicit silencing and stale entries)."""

from __future__ import annotations

import argparse
import subprocess
import sys
from fnmatch import fnmatch
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate scripts/zen_allowlist.txt entries are explicit and scoped.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail on violations.",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=Path("scripts/zen_allowlist.txt"),
        help="Allowlist path (default: scripts/zen_allowlist.txt).",
    )
    return parser.parse_args()


def list_tracked_paths() -> list[str]:
    proc = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
    )
    raw = proc.stdout.decode("utf-8", errors="ignore")
    return [p for p in raw.split("\0") if p]


def parse_rule(line: str) -> tuple[str, str | None]:
    token = line.strip()
    if "|" not in token:
        return token, None

    lhs, rhs = token.split("|", 1)
    pattern = lhs.strip()
    rationale = rhs.strip()
    return pattern, rationale


def matches_any(pattern: str, tracked: list[str]) -> bool:
    # Normalize optional line-scoped rules: path.rs:123
    if ":" in pattern:
        prefix, _, suffix = pattern.rpartition(":")
        if suffix.isdigit() and prefix:
            pattern = prefix

    for path in tracked:
        if fnmatch(path, pattern):
            return True
    return False


def main() -> int:
    args = parse_args()
    allowlist_path = args.allowlist
    if not allowlist_path.is_absolute():
        allowlist_path = REPO_ROOT / allowlist_path

    if not allowlist_path.exists():
        print(
            f"Missing allowlist file: {allowlist_path.relative_to(REPO_ROOT)}\n\n"
            "Remediation:\n"
            "1. Create scripts/zen_allowlist.txt.\n"
            "2. Add scoped rules in format: path[:line] | rationale.\n"
            "3. Re-run: python3 scripts/check_zen_allowlist.py",
            file=sys.stderr,
        )
        return 1

    tracked = list_tracked_paths()

    missing_rationale: list[str] = []
    stale_rules: list[str] = []
    duplicate_rules: list[str] = []
    seen: set[str] = set()

    for raw_line in allowlist_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue

        pattern, rationale = parse_rule(line)
        if not pattern:
            continue

        if pattern in seen:
            duplicate_rules.append(pattern)
        seen.add(pattern)

        if not rationale:
            missing_rationale.append(pattern)

        if not matches_any(pattern, tracked):
            stale_rules.append(pattern)

    total_rules = len(seen)
    violations = len(missing_rationale) + len(stale_rules) + len(duplicate_rules)

    print(
        "Zen allowlist summary: "
        f"rules={total_rules}, missing_rationale={len(missing_rationale)}, "
        f"stale={len(stale_rules)}, duplicates={len(duplicate_rules)}, mode={args.mode}"
    )

    if missing_rationale:
        print("\nEntries missing explicit rationale (must include '| rationale'):")
        for item in missing_rationale:
            print(f"- {item}")

    if stale_rules:
        print("\nStale allowlist entries (no tracked files matched):")
        for item in stale_rules:
            print(f"- {item}")

    if duplicate_rules:
        print("\nDuplicate allowlist entries:")
        for item in sorted(set(duplicate_rules)):
            print(f"- {item}")

    if args.mode == "enforce" and violations > 0:
        print(
            "\nZen allowlist enforcement failed.\n\n"
            "Remediation:\n"
            "1. Add explicit rationale for each entry: path[:line] | rationale.\n"
            "2. Remove or fix stale entries that no longer match tracked files.\n"
            "3. Remove duplicate entries.\n"
            "4. Re-run: python3 scripts/check_zen_allowlist.py --mode enforce --allowlist scripts/zen_allowlist.txt",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and violations > 0:
        print(
            "\nWarning mode: allowlist hygiene issues reported but not blocking. "
            "Promote to --mode enforce after cleanup."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
