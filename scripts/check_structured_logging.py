#!/usr/bin/env python3
"""Enforce structured logging in production Rust library code."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_ALLOWLIST = Path("scripts/structured_logging_allowlist.txt")
PRINTLN_RE = re.compile(r"(?<![A-Za-z0-9_])(?:eprintln|println)!")
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\([^]]*\btest\b")
TEST_ATTR_RE = re.compile(r"#\s*\[\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*::\s*)*test\b")
MOD_RE = re.compile(r"\bmod\s+\w+\s*\{")
MOD_TESTS_RE = re.compile(r"\bmod\s+\w*tests\w*\s*\{")
FN_RE = re.compile(r"\b(?:async\s+)?fn\s+\w+\s*\([^)]*\)")
RAW_STRING_PREFIX_RE = re.compile(r"(?:br|rb|r)(?P<hashes>#{0,255})\"")


@dataclass
class AllowRule:
    pattern: str
    line: int | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate production Rust code uses tracing instead of println!/eprintln!."
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="enforce",
        help="warn: report only; enforce: fail on findings.",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=DEFAULT_ALLOWLIST,
        help=f"Allowlist file with path or path:line rules (default: {DEFAULT_ALLOWLIST}).",
    )
    parser.add_argument(
        "--max-findings",
        type=int,
        default=200,
        help="Maximum number of findings to print (default: 200).",
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
    return [REPO_ROOT / Path(rel) for rel in raw.split("\0") if rel]


def is_test_path(path: Path) -> bool:
    rel = path.relative_to(REPO_ROOT).as_posix()
    return (
        rel.startswith("fuzz/")
        or rel.startswith("target/")
        or rel.startswith("tests/")
        or "/tests/" in rel
        or "/benches/" in rel
        or "/examples/" in rel
        or rel.endswith("/tests.rs")
        or rel.endswith("tests.rs")
        or rel.endswith("_test.rs")
        or rel.endswith("_tests.rs")
    )


def is_ignored_path(path: Path) -> bool:
    rel = path.relative_to(REPO_ROOT).as_posix()
    return (
        rel == "build.rs"
        or rel.startswith("src/")
        or rel.endswith("/build.rs")
        or rel.endswith("/src/main.rs")
        or "/src/bin/" in rel
        or is_test_path(path)
    )


def parse_allow_rules(path: Path) -> list[AllowRule]:
    if not path.is_absolute():
        path = REPO_ROOT / path
    if not path.exists():
        raise FileNotFoundError(path)

    rules: list[AllowRule] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue

        token = line.split("|", 1)[0].strip()
        if not token:
            continue

        if ":" in token:
            prefix, _, suffix = token.rpartition(":")
            if suffix.isdigit() and prefix:
                rules.append(AllowRule(pattern=prefix, line=int(suffix)))
                continue

        rules.append(AllowRule(pattern=token))

    return rules


def is_allowlisted(rel_path: str, line_no: int, rules: list[AllowRule]) -> bool:
    for rule in rules:
        if not fnmatch(rel_path, rule.pattern):
            continue
        if rule.line is None or rule.line == line_no:
            return True
    return False


def raw_string_prefix_length(source: str, index: int) -> int:
    if index > 0 and (source[index - 1].isalnum() or source[index - 1] == "_"):
        return 0

    match = RAW_STRING_PREFIX_RE.match(source, index)
    if match is None:
        return 0
    return len(match.group(0))


def strip_non_code(source: str) -> str:
    out: list[str] = []
    i = 0
    block_depth = 0
    in_line_comment = False
    in_string = False
    raw_string_hashes = ""

    while i < len(source):
        ch = source[i]
        nxt = source[i + 1] if i + 1 < len(source) else ""

        if in_line_comment:
            if ch == "\n":
                out.append("\n")
                in_line_comment = False
            else:
                out.append(" ")
            i += 1
            continue

        if block_depth > 0:
            if ch == "/" and nxt == "*":
                out.extend((" ", " "))
                block_depth += 1
                i += 2
                continue
            if ch == "*" and nxt == "/":
                out.extend((" ", " "))
                block_depth -= 1
                i += 2
                continue
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue

        if in_string:
            if ch == "\\" and i + 1 < len(source):
                out.extend((" ", " "))
                i += 2
                continue
            if ch == '"':
                out.append(" ")
                in_string = False
            else:
                out.append("\n" if ch == "\n" else " ")
            i += 1
            continue

        if raw_string_hashes:
            if ch == '"' and source.startswith(raw_string_hashes, i + 1):
                out.append(" ")
                out.extend(" " for _ in raw_string_hashes)
                i += 1 + len(raw_string_hashes)
                raw_string_hashes = ""
                continue
            out.append("\n" if ch == "\n" else " ")
            i += 1
            continue

        raw_prefix_len = raw_string_prefix_length(source, i)
        if raw_prefix_len > 0:
            raw_string_hashes = source[i + raw_prefix_len - 1 : i + raw_prefix_len - 1]
            quote_index = i + raw_prefix_len - 1
            hashes_start = i + 1
            if source[i] == "b":
                hashes_start += 1
            if source[hashes_start] == "r":
                hashes_start += 1
            raw_string_hashes = source[hashes_start:quote_index]
            out.extend(" " for _ in range(raw_prefix_len))
            i += raw_prefix_len
            continue

        if ch == "/" and nxt == "/":
            out.extend((" ", " "))
            in_line_comment = True
            i += 2
            continue

        if ch == "/" and nxt == "*":
            out.extend((" ", " "))
            block_depth = 1
            i += 2
            continue

        if ch == '"':
            out.append(" ")
            in_string = True
            i += 1
            continue

        out.append(ch)
        i += 1

    return "".join(out)


def scan_file(path: Path, rules: list[AllowRule]) -> list[tuple[str, int, str]]:
    rel_path = path.relative_to(REPO_ROOT).as_posix()
    source = path.read_text(encoding="utf-8", errors="ignore")
    stripped = strip_non_code(source)
    raw_lines = source.splitlines()
    code_lines = stripped.splitlines()
    findings: list[tuple[str, int, str]] = []

    pending_cfg_test = False
    pending_test_fn = False
    in_test_module = False
    test_module_depth = 0
    in_test_fn = False
    test_fn_depth = 0

    for idx, raw_line in enumerate(raw_lines, start=1):
        code_line = code_lines[idx - 1] if idx - 1 < len(code_lines) else ""
        brace_delta = code_line.count("{") - code_line.count("}")
        stripped_code = code_line.strip()

        if in_test_module:
            test_module_depth += brace_delta
            if test_module_depth <= 0:
                in_test_module = False
            continue

        if in_test_fn:
            test_fn_depth += brace_delta
            if test_fn_depth <= 0:
                in_test_fn = False
            continue

        if CFG_TEST_RE.search(code_line):
            pending_cfg_test = True

        if MOD_TESTS_RE.search(code_line):
            in_test_module = True
            test_module_depth = brace_delta
            pending_cfg_test = False
            continue

        if pending_cfg_test and MOD_RE.search(code_line):
            in_test_module = True
            test_module_depth = brace_delta
            pending_cfg_test = False
            continue

        if TEST_ATTR_RE.search(code_line):
            pending_test_fn = True

        if pending_test_fn and FN_RE.search(code_line):
            in_test_fn = True
            test_fn_depth = brace_delta
            pending_test_fn = False
            pending_cfg_test = False
            continue

        if pending_cfg_test and FN_RE.search(code_line):
            in_test_fn = True
            test_fn_depth = brace_delta
            pending_cfg_test = False
            continue

        if pending_cfg_test and stripped_code and not stripped_code.startswith("#["):
            pending_cfg_test = False

        if PRINTLN_RE.search(code_line) and not is_allowlisted(rel_path, idx, rules):
            findings.append((rel_path, idx, raw_line.strip()))

    return findings


def main() -> int:
    args = parse_args()

    try:
        allow_rules = parse_allow_rules(args.allowlist)
    except FileNotFoundError as exc:
        allowlist_path = Path(exc.args[0]).relative_to(REPO_ROOT)
        print(
            f"Missing structured logging allowlist: {allowlist_path}\n\n"
            "Remediation:\n"
            "1. Create scripts/structured_logging_allowlist.txt.\n"
            "2. Add scoped entries in the format: path[:line] | rationale.\n"
            "3. Re-run: python3 scripts/check_structured_logging.py",
            file=sys.stderr,
        )
        return 1

    findings: list[tuple[str, int, str]] = []
    scanned = 0
    ignored = 0

    for path in list_tracked_rust_files():
        if is_ignored_path(path):
            ignored += 1
            continue
        scanned += 1
        findings.extend(scan_file(path, allow_rules))

    print(
        "structured logging scan summary: "
        f"scanned_files={scanned}, ignored_files={ignored}, "
        f"allow_rules={len(allow_rules)}, findings={len(findings)}, mode={args.mode}"
    )

    if findings:
        print("\nFindings (path:line | source):")
        for rel_path, line_no, source in findings[: args.max_findings]:
            print(f"- {rel_path}:{line_no} | {source}")

        if len(findings) > args.max_findings:
            remaining = len(findings) - args.max_findings
            print(f"- ... {remaining} additional finding(s) not shown")

    if args.mode == "enforce" and findings:
        print(
            "\nStructured logging enforcement failed.\n\n"
            "Remediation:\n"
            "1. Replace println!/eprintln! with tracing macros in library code.\n"
            "2. If direct terminal output is intentional, add a scoped rule to scripts/structured_logging_allowlist.txt.\n"
            "3. Re-run: python3 scripts/check_structured_logging.py",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and findings:
        print(
            "\nWarning mode: findings reported but not blocking. "
            "Promote to --mode enforce after cleanup."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
