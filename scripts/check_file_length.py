#!/usr/bin/env python3
"""Fail when tracked text files exceed the line-count budget (hard ceiling).

This is the hard counterpart to the soft Rust complexity signal in
`check_rust_file_length.py` (warn at 500 lines): every tracked text file must
stay at or under `--max-lines` (default: 1000), except for entries listed in
`scripts/file_length_allowlist.txt`.

Allowlist entries use the same shape as `scripts/large_file_allowlist.txt`:

    glob/pattern
    glob/pattern=MAX_LINES

A bare pattern fully exempts matching files (reserved for generated output such
as `Cargo.lock`). A `=MAX_LINES` suffix (underscores allowed) caps how far an
acknowledged outlier may grow. Binary files are skipped automatically via an
extension blocklist plus a null-byte sniff.

Usage:
    python3 scripts/check_file_length.py [--mode enforce|warn]
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from dataclasses import dataclass, field
from fnmatch import fnmatch
from pathlib import Path
from typing import Iterable, List, Optional


REPO_ROOT = Path(__file__).resolve().parent.parent

DEFAULT_MAX_LINES = 1000
SNIFF_BYTES = 8192

# Extensions that are never line-counted, even when the content sniff misses.
# (The sniff is the real backstop; this list is a fast path plus documentation.)
BINARY_EXTENSIONS = frozenset(
    {
        ".gif",
        ".png",
        ".jpg",
        ".jpeg",
        ".ico",
        ".bmp",
        ".webp",
        ".avif",
        ".woff",
        ".woff2",
        ".ttf",
        ".otf",
        ".eot",
        ".pdf",
        ".zip",
        ".gz",
        ".tar",
        ".tgz",
        ".bz2",
        ".xz",
        ".7z",
        ".mp4",
        ".mov",
        ".webm",
        ".mp3",
        ".wav",
        ".ogg",
        ".flac",
        ".wasm",
        ".dylib",
        ".so",
        ".dll",
        ".exe",
        ".bin",
        ".dat",
        ".db",
        ".sqlite",
    }
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Enforce a hard line-count ceiling on tracked text files.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="enforce",
        help="warn: report only; enforce: fail on violations (default: enforce).",
    )
    parser.add_argument(
        "--max-lines",
        type=int,
        default=DEFAULT_MAX_LINES,
        help="Maximum allowed line count (default: 1000).",
    )
    parser.add_argument(
        "--top",
        type=int,
        default=25,
        help="Number of longest files to print (default: 25).",
    )
    parser.add_argument(
        "--allow",
        action="append",
        default=[],
        help=(
            "Glob patterns to allow even when above the limit. "
            "May be specified multiple times."
        ),
    )
    parser.add_argument(
        "--allowlist-file",
        type=Path,
        default=None,
        help=(
            "Path to a newline-delimited allowlist of glob patterns. "
            "Defaults to scripts/file_length_allowlist.txt when present."
        ),
    )
    parser.add_argument(
        "--report-json",
        type=Path,
        default=None,
        help="Optional path to write JSON summary.",
    )
    parser.add_argument(
        "--quiet",
        action="store_true",
        help="Print only the summary plus violations (for gate scripts).",
    )
    return parser.parse_args()


@dataclass
class AllowRule:
    pattern: str
    max_lines: Optional[int] = None


@dataclass
class FileEntry:
    path: str
    lines: int


@dataclass
class Evaluation:
    max_lines: int
    entries: List[FileEntry] = field(default_factory=list)
    over_unallowed: List[FileEntry] = field(default_factory=list)
    over_allowed: List[FileEntry] = field(default_factory=list)
    stale_rules: List[str] = field(default_factory=list)
    scanned_text: int = 0
    skipped_binary: int = 0
    skipped_missing: int = 0


def list_tracked_files(repo_root: Path) -> List[Path]:
    proc = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=repo_root,
        check=True,
        capture_output=True,
    )
    raw = proc.stdout.decode("utf-8", errors="ignore")
    return [repo_root / Path(p) for p in raw.split("\0") if p]


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
    max_lines: Optional[int] = None

    if "=" in text:
        pattern_text, _, size_text = text.partition("=")
        pattern = pattern_text.strip()
        size_compact = size_text.replace("_", "").strip()
        if not pattern:
            raise ValueError("Allowlist entry is missing a pattern before '='")
        if size_compact:
            try:
                max_lines = int(size_compact)
            except ValueError as exc:
                raise ValueError(
                    f"Invalid line limit '{size_text}' in allowlist entry"
                ) from exc
        else:
            raise ValueError("Allowlist entry must specify a limit after '='")

    return AllowRule(pattern=pattern, max_lines=max_lines)


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
        file_path = REPO_ROOT / "scripts" / "file_length_allowlist.txt"
    elif not file_path.is_absolute():
        file_path = REPO_ROOT / file_path

    patterns.extend(load_allowlist(file_path))
    return patterns


def is_binary_path(path: Path) -> bool:
    return path.suffix.lower() in BINARY_EXTENSIONS


def looks_binary(path: Path) -> bool:
    try:
        with path.open("rb") as handle:
            return b"\x00" in handle.read(SNIFF_BYTES)
    except FileNotFoundError:
        return False


def count_lines(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="ignore") as handle:
        return sum(1 for _ in handle)


def rule_matches(rule: AllowRule, path: Path, repo_root: Path) -> bool:
    relative_posix = path.relative_to(repo_root).as_posix()
    absolute_posix = path.as_posix()
    normalized = normalize_pattern(rule.pattern)
    return fnmatch(relative_posix, normalized) or fnmatch(
        absolute_posix, rule.pattern.replace("\\", "/")
    )


def matching_rule_indexes(
    path: Path, allow_rules: List[AllowRule], *, repo_root: Path
) -> List[int]:
    return [
        index
        for index, rule in enumerate(allow_rules)
        if rule_matches(rule, path, repo_root)
    ]


def is_allowed(
    path: Path, lines: int, allow_rules: List[AllowRule], *, repo_root: Path
) -> bool:
    for index in matching_rule_indexes(path, allow_rules, repo_root=repo_root):
        cap = allow_rules[index].max_lines
        if cap is None or lines <= cap:
            return True
    return False


def evaluate(
    files: Iterable[Path],
    allow_rules: List[AllowRule],
    *,
    max_lines: int,
    repo_root: Path,
) -> Evaluation:
    result = Evaluation(max_lines=max_lines)
    matched_rules: set[int] = set()
    file_list = list(files)

    for path in file_list:
        if is_binary_path(path) or looks_binary(path):
            result.skipped_binary += 1
            continue
        try:
            lines = count_lines(path)
        except FileNotFoundError:
            # File removed since ls-files snapshot; ignore.
            result.skipped_missing += 1
            continue
        except OSError:
            result.skipped_missing += 1
            continue

        result.scanned_text += 1
        try:
            rel = path.relative_to(repo_root).as_posix()
        except ValueError:
            rel = path.as_posix()
        entry = FileEntry(path=rel, lines=lines)
        result.entries.append(entry)

        if lines <= max_lines:
            continue

        matched = matching_rule_indexes(path, allow_rules, repo_root=repo_root)
        matched_rules.update(matched)
        if is_allowed(path, lines, allow_rules, repo_root=repo_root):
            result.over_allowed.append(entry)
        else:
            result.over_unallowed.append(entry)

    # A rule that matched no over-limit file may still match an under-limit
    # tracked file (a cap with headroom). Only flag rules matching nothing.
    for index, rule in enumerate(allow_rules):
        if index in matched_rules:
            continue
        if not any(rule_matches(rule, path, repo_root) for path in file_list):
            result.stale_rules.append(rule.pattern)

    result.entries.sort(key=lambda item: item.lines, reverse=True)
    result.over_unallowed.sort(key=lambda item: item.lines, reverse=True)
    result.over_allowed.sort(key=lambda item: item.lines, reverse=True)
    return result


def emit_report(result: Evaluation, top_count: int, *, quiet: bool = False) -> None:
    print(
        "File length summary: "
        f"scanned_text={result.scanned_text}, "
        f"skipped_binary={result.skipped_binary}, "
        f"skipped_missing={result.skipped_missing}, "
        f">{result.max_lines}_unallowed={len(result.over_unallowed)}, "
        f">{result.max_lines}_allowed={len(result.over_allowed)}, "
        f"stale_allowlist={len(result.stale_rules)}"
    )

    if result.over_unallowed:
        print("\nFiles exceeding the line budget (no allowlist cover):")
        print("| Lines | Path |")
        print("| ---: | --- |")
        for item in result.over_unallowed:
            print(f"| {item.lines} | {item.path} |")

    if result.over_allowed and not quiet:
        print("\nFiles exceeding the line budget (allowlisted, capped):")
        print("| Lines | Path |")
        print("| ---: | --- |")
        for item in result.over_allowed:
            print(f"| {item.lines} | {item.path} |")

    if top_count > 0 and result.entries and not quiet:
        shown = result.entries[:top_count]
        print(f"\nTop {len(shown)} longest tracked text files:")
        print("| Lines | Path |")
        print("| ---: | --- |")
        for item in shown:
            print(f"| {item.lines} | {item.path} |")

    if result.stale_rules:
        print("\nStale allowlist entries (match no tracked file, remove them):")
        for pattern in result.stale_rules:
            print(f"- {pattern}")


def write_json_report(report_path: Path, result: Evaluation, mode: str) -> None:
    if not report_path.is_absolute():
        report_path = REPO_ROOT / report_path
    report_path.parent.mkdir(parents=True, exist_ok=True)
    payload = {
        "summary": {
            "mode": mode,
            "max_lines": result.max_lines,
            "scanned_text": result.scanned_text,
            "skipped_binary": result.skipped_binary,
            "skipped_missing": result.skipped_missing,
            "over_unallowed": len(result.over_unallowed),
            "over_allowed": len(result.over_allowed),
            "stale_allowlist": len(result.stale_rules),
        },
        "over_unallowed": [
            {"path": item.path, "lines": item.lines}
            for item in result.over_unallowed
        ],
        "over_allowed": [
            {"path": item.path, "lines": item.lines}
            for item in result.over_allowed
        ],
        "stale_allowlist": result.stale_rules,
    }
    report_path.write_text(json.dumps(payload, indent=2), encoding="utf-8")


def main() -> int:
    args = parse_args()
    try:
        allow_rules = resolve_allowlist_patterns(args.allow, args.allowlist_file)
    except ValueError as exc:
        print(f"Invalid allowlist entry: {exc}", file=sys.stderr)
        return 2

    try:
        files = list_tracked_files(REPO_ROOT)
    except subprocess.CalledProcessError as exc:
        print(f"Failed to list tracked files: {exc}", file=sys.stderr)
        return 2

    result = evaluate(files, allow_rules, max_lines=args.max_lines, repo_root=REPO_ROOT)
    emit_report(result, args.top, quiet=args.quiet)

    if args.report_json is not None:
        write_json_report(args.report_json, result, args.mode)

    failures = len(result.over_unallowed) + len(result.stale_rules)
    if args.mode == "enforce" and failures:
        print(
            "\nFile length enforcement failed.\n\n"
            "Remediation:\n"
            "1. Split oversized files into focused submodules (one responsibility per file).\n"
            "2. Preserve the public API surface by re-exporting from mod.rs/index where needed.\n"
            "3. Generated, vendored, or lockfile content belongs in "
            "scripts/file_length_allowlist.txt as `glob=MAX_LINES` with a rationale "
            "comment; never allowlist first-party source without a split plan.\n"
            "4. Remove stale allowlist entries that no longer match tracked files.\n"
            "5. Re-run: python3 scripts/check_file_length.py --mode enforce "
            f"--max-lines {args.max_lines}",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and failures:
        print(
            "\nWarning mode: file length findings reported but not blocking."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
