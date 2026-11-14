#!/usr/bin/env python3
"""Tiered Rust file-length governance with a ratchet baseline.

Tracked `*.rs` files are bucketed into tiers (vendored and generated paths are
excluded entirely):

- `src` tier (production source): warn at 500 lines, enforce at `--max-lines`
  (default 800). Files over the enforce budget fail unless the baseline ledger
  caps them; caps only ever ratchet down.
- `test` tier (`tests.rs`, `*_tests.rs`, `tests_*.rs` files plus `tests/`,
  `benches/`, `fuzz/`, and `examples/` directories): warn at 800 lines,
  enforce at `--test-max-lines` (default 1000). Long fixtures are lower-risk
  and get the higher budget. The universal 1000-line hard ceiling
  (`check_file_length.py`) still applies.

The baseline (`scripts/rust_length_baseline.txt`) is the debt ledger for
src-tier files that were already over budget when the gate was adopted.
Entries use the same `glob=CAP` shape as `scripts/file_length_allowlist.txt`;
growing a file past its cap fails, and an entry whose file shrank below the
enforce budget (or no longer matches a src-tier file) goes stale and must be
removed in the same change.

Usage:
    python3 scripts/check_rust_file_length.py [--mode warn|enforce]
        [--max-lines N] [--test-max-lines N] [--baseline PATH]
    python3 scripts/check_rust_file_length.py --emit-baseline
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable, List, Optional, Tuple


REPO_ROOT = Path(__file__).resolve().parent.parent

DEFAULT_MAX_LINES = 800
DEFAULT_TEST_MAX_LINES = 1000
SRC_WARN_LINES = 500
TEST_WARN_LINES = 800
DEFAULT_BASELINE = Path("scripts/rust_length_baseline.txt")

# Vendored third-party trees and machine-generated output never belong in the
# tier budgets; the universal hard ceiling exempts them via the allowlist.
EXCLUDED_PATTERNS = (
    "patches/**",
    "**/generated.rs",
    "**/*.generated.rs",
)

# Path segments that demote a file into the relaxed test budget, plus the
# cfg(test) module filename conventions (`tests.rs`, `*_tests.rs`, `tests_*.rs`
# — all loaded via `#[cfg(test)] mod ...;`). Oddball test modules (e.g.
# `test_support.rs`) stay src-tier; all are well under the enforce budget.
TEST_TIER_SEGMENTS = frozenset({"tests", "benches", "fuzz", "examples"})
TEST_TIER_FILENAME = "tests.rs"
TEST_TIER_SUFFIX = "_tests.rs"
TEST_TIER_PREFIX = "tests_"

GLOB_CHARS = "*?["


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate tiered Rust file length budgets with a ratchet baseline.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail on budget violations (default: warn).",
    )
    parser.add_argument(
        "--max-lines",
        type=int,
        default=DEFAULT_MAX_LINES,
        help="src-tier enforce budget (default: 800; warn target is 500).",
    )
    parser.add_argument(
        "--test-max-lines",
        type=int,
        default=DEFAULT_TEST_MAX_LINES,
        help="test-tier enforce budget (default: 1000).",
    )
    parser.add_argument(
        "--baseline",
        type=Path,
        default=None,
        help=(
            "Path to the ratchet baseline ledger. "
            f"Defaults to {DEFAULT_BASELINE} when present."
        ),
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
    parser.add_argument(
        "--emit-baseline",
        action="store_true",
        help=(
            "Print `path=lines` ledger entries for every src-tier file over the "
            "enforce budget (sorted by path) and exit. Re-snapshotting is an "
            "explicit adoption decision: merging its output into the baseline "
            "must never raise an existing cap."
        ),
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Print only the summary plus violations (for gate scripts).",
    )
    return parser.parse_args()


@dataclass
class FileEntry:
    path: str
    lines: int


@dataclass
class Evaluation:
    max_lines: int
    test_max_lines: int
    total_scanned: int = 0
    skipped_excluded: int = 0
    skipped_missing: int = 0
    over_warn: List[FileEntry] = field(default_factory=list)
    over_budget: List[FileEntry] = field(default_factory=list)
    past_cap: List[Tuple[FileEntry, int]] = field(default_factory=list)
    over_test_budget: List[FileEntry] = field(default_factory=list)
    baseline_covered: List[FileEntry] = field(default_factory=list)
    stale_baseline: List[str] = field(default_factory=list)


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


def count_lines(path: Path) -> int | None:
    try:
        with path.open("r", encoding="utf-8", errors="ignore") as handle:
            return sum(1 for _ in handle)
    except OSError:
        # File removed since the ls-files snapshot; ignore.
        return None


def is_excluded(relative_path: str) -> bool:
    return any(fnmatch.fnmatch(relative_path, pattern) for pattern in EXCLUDED_PATTERNS)


def is_test_tier(relative_path: str) -> bool:
    parts = relative_path.split("/")
    filename = parts[-1]
    return (
        filename == TEST_TIER_FILENAME
        or filename.endswith(TEST_TIER_SUFFIX)
        or filename.startswith(TEST_TIER_PREFIX)
        or any(segment in TEST_TIER_SEGMENTS for segment in parts[:-1])
    )


def has_glob(pattern: str) -> bool:
    return any(char in pattern for char in GLOB_CHARS)


def load_baseline(file_path: Path) -> dict[str, int]:
    """Parse `pattern=CAP` ledger entries; caps are mandatory for ratcheting."""
    if not file_path.exists():
        return {}

    baseline: dict[str, int] = {}
    for line_number, raw_line in enumerate(file_path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        pattern, separator, cap_text = line.partition("=")
        pattern = pattern.strip()
        if separator == "" or not pattern or not cap_text.strip().isdigit():
            raise ValueError(
                f"{file_path}:{line_number}: baseline entries must be `pattern=CAP` "
                f"(got '{line}')"
            )
        if pattern in baseline:
            # Last-wins would silently raise a cap and break the ratchet.
            raise ValueError(
                f"{file_path}:{line_number}: duplicate baseline entry for '{pattern}'"
            )
        baseline[pattern] = int(cap_text.strip())

    return baseline


def resolve_baseline_path(path_arg: Path | None) -> Path:
    path = path_arg if path_arg is not None else DEFAULT_BASELINE
    return path if path.is_absolute() else REPO_ROOT / path


def match_baseline(baseline: dict[str, int], relative_path: str) -> Optional[int]:
    if relative_path in baseline:
        return baseline[relative_path]
    caps = [
        cap
        for pattern, cap in baseline.items()
        if has_glob(pattern) and fnmatch.fnmatch(relative_path, pattern)
    ]
    return min(caps) if caps else None


def find_stale_baseline(
    baseline: dict[str, int], over_budget_src_paths: list[str]
) -> List[str]:
    """Flag ledger entries that no longer cover an over-budget src file.

    A file shrinking below the enforce budget (or being renamed, demoted to
    the test tier, or excluded) leaves its entry without a purpose; the entry
    goes stale so the ledger ratchets down instead of accumulating headroom.
    """
    covered = set(over_budget_src_paths)
    stale: List[str] = []
    for pattern in baseline:
        if has_glob(pattern):
            if not any(fnmatch.fnmatch(path, pattern) for path in over_budget_src_paths):
                stale.append(pattern)
        elif pattern not in covered:
            stale.append(pattern)
    return stale


def collect_entries(files: Iterable[Path]) -> tuple[List[FileEntry], int]:
    entries: List[FileEntry] = []
    skipped_missing = 0
    for path in files:
        lines = count_lines(path)
        if lines is None:
            skipped_missing += 1
            continue
        entries.append(FileEntry(path=path.relative_to(REPO_ROOT).as_posix(), lines=lines))
    entries.sort(key=lambda entry: entry.lines, reverse=True)
    return entries, skipped_missing


def evaluate(
    entries: List[FileEntry],
    baseline: dict[str, int],
    *,
    max_lines: int,
    test_max_lines: int,
) -> Evaluation:
    result = Evaluation(max_lines=max_lines, test_max_lines=test_max_lines)

    for entry in entries:
        result.total_scanned += 1
        if is_excluded(entry.path):
            result.skipped_excluded += 1
            continue
        if is_test_tier(entry.path):
            if entry.lines > TEST_WARN_LINES:
                result.over_warn.append(entry)
            if entry.lines > test_max_lines:
                result.over_test_budget.append(entry)
            continue

        if entry.lines > SRC_WARN_LINES:
            result.over_warn.append(entry)
        if entry.lines <= max_lines:
            continue
        cap = match_baseline(baseline, entry.path)
        if cap is None:
            result.over_budget.append(entry)
        elif entry.lines > cap:
            result.past_cap.append((entry, cap))
        else:
            result.baseline_covered.append(entry)

    # Only src files still over the enforce budget keep a ledger entry alive;
    # anything else makes the entry stale so the ledger ratchets down.
    over_budget_src_paths = [
        entry.path for entry in result.over_budget
    ] + [entry.path for entry, _ in result.past_cap] + [
        entry.path for entry in result.baseline_covered
    ]
    result.stale_baseline = find_stale_baseline(baseline, over_budget_src_paths)
    return result


def entries_table(items: Iterable[FileEntry]) -> List[str]:
    rows = ["| Lines | Path |", "| ---: | --- |"]
    rows.extend(f"| {entry.lines} | {entry.path} |" for entry in items)
    return rows


def emit_report(result: Evaluation, top_count: int, *, mode: str, quiet: bool = False) -> None:
    print(
        "Rust file length summary: "
        f"total={result.total_scanned}, "
        f"excluded={result.skipped_excluded}, "
        f"skipped_missing={result.skipped_missing}, "
        f"over_warn={len(result.over_warn)}, "
        f"src_over_budget={len(result.over_budget)}, "
        f"src_capped={len(result.baseline_covered)}, "
        f"past_cap={len(result.past_cap)}, "
        f"test_over_budget={len(result.over_test_budget)}, "
        f"stale_baseline={len(result.stale_baseline)}"
    )

    if not quiet and top_count > 0 and result.over_warn:
        shown = result.over_warn[:top_count]
        print(f"\nTop {len(shown)} over warn budgets (src 500 / tests 800, soft):")
        print("\n".join(entries_table(shown)))

    # Violation tables stay enforce-only: the warn signal must stay a bounded
    # top-N view so the fast gate output does not grow with the debt ledger.
    if mode != "enforce":
        return

    if result.over_budget:
        print("\nsrc-tier files over budget with no baseline cover (split required):")
        print("\n".join(entries_table(result.over_budget)))

    if result.past_cap:
        print("\nsrc-tier files grown past their ratchet cap (split or tighten plan):")
        rows = ["| Lines | Cap | Path |", "| ---: | ---: | --- |"]
        rows.extend(
            f"| {entry.lines} | {cap} | {entry.path} |" for entry, cap in result.past_cap
        )
        print("\n".join(rows))

    if result.over_test_budget:
        print("\ntest-tier files over the test budget:")
        print("\n".join(entries_table(result.over_test_budget)))

    if result.stale_baseline:
        print("\nStale baseline entries (match no src-tier file, remove them):")
        for pattern in result.stale_baseline:
            print(f"- {pattern}")

    if not quiet and top_count > 0 and result.baseline_covered:
        shown = result.baseline_covered[:top_count]
        print(f"\nTop {len(shown)} baseline-covered files (split plans welcome):")
        print("\n".join(entries_table(shown)))


def write_json_report(report_path: Path, result: Evaluation, mode: str) -> None:
    if not report_path.is_absolute():
        report_path = REPO_ROOT / report_path
    report_path.parent.mkdir(parents=True, exist_ok=True)
    payload: dict[str, Any] = {
        "summary": {
            "mode": mode,
            "max_lines": result.max_lines,
            "test_max_lines": result.test_max_lines,
            "total_scanned": result.total_scanned,
            "skipped_excluded": result.skipped_excluded,
            "skipped_missing": result.skipped_missing,
            "over_warn": len(result.over_warn),
            "src_over_budget": len(result.over_budget),
            "src_capped": len(result.baseline_covered),
            "past_cap": len(result.past_cap),
            "test_over_budget": len(result.over_test_budget),
            "stale_baseline": len(result.stale_baseline),
        },
        "over_budget": [
            {"path": entry.path, "lines": entry.lines} for entry in result.over_budget
        ],
        "past_cap": [
            {"path": entry.path, "lines": entry.lines, "cap": cap}
            for entry, cap in result.past_cap
        ],
        "over_test_budget": [
            {"path": entry.path, "lines": entry.lines}
            for entry in result.over_test_budget
        ],
        "stale_baseline": result.stale_baseline,
    }
    report_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def main() -> int:
    args = parse_args()

    baseline_path = resolve_baseline_path(args.baseline)
    try:
        baseline = load_baseline(baseline_path)
    except ValueError as exc:
        print(f"Invalid baseline ledger: {exc}", file=sys.stderr)
        return 2

    files = list_tracked_rust_files()
    entries, skipped_missing = collect_entries(files)
    result = evaluate(
        entries,
        baseline,
        max_lines=args.max_lines,
        test_max_lines=args.test_max_lines,
    )
    result.skipped_missing += skipped_missing

    if args.emit_baseline:
        over_budget_src = sorted(
            [(entry.path, entry.lines) for entry in result.over_budget]
            + [(entry.path, entry.lines) for entry, _ in result.past_cap]
            + [(entry.path, entry.lines) for entry in result.baseline_covered]
        )
        for path, lines in over_budget_src:
            print(f"{path}={lines}")
        return 0

    emit_report(result, args.top, mode=args.mode, quiet=args.quiet)

    if args.report_json is not None:
        write_json_report(args.report_json, result, args.mode)

    failures = (
        len(result.over_budget)
        + len(result.past_cap)
        + len(result.over_test_budget)
        + len(result.stale_baseline)
    )
    if args.mode == "enforce" and failures:
        print(
            "\nRust file length enforcement failed.\n\n"
            "Remediation:\n"
            "1. Split oversized src files into focused submodules (one responsibility "
            "per file); preserve the public API surface by re-exporting from mod.rs.\n"
            "2. Never grow a file past its baseline cap. Shrinking a file below "
            f"--max-lines {args.max_lines} makes its baseline entry stale; remove the "
            "entry in the same change.\n"
            "3. New src files over budget fail outright; tests get the higher "
            f"--test-max-lines {args.test_max_lines} budget.\n"
            "4. Vendored/generated trees are excluded; never add first-party source "
            "to the baseline without a split plan.\n"
            "5. Re-run: python3 scripts/check_rust_file_length.py --mode enforce "
            f"--max-lines {args.max_lines}",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and failures:
        print(
            "\nWarning mode: tiered budget findings reported but not blocking. "
            "The release gate (check.sh) enforces the same budgets."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
