#!/usr/bin/env python3
"""Warn or enforce against unwrap/expect usage in non-test Rust code."""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
UNWRAP_EXPECT_RE = re.compile(r"\.(unwrap|expect)\s*\(")


@dataclass
class AllowRule:
    pattern: str
    line: int | None = None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate production Rust code does not use unwrap()/expect().",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail on findings.",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=Path("scripts/zen_allowlist.txt"),
        help="Allowlist file with path or path:line patterns (default: scripts/zen_allowlist.txt).",
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


def parse_allow_rules(path: Path) -> list[AllowRule]:
    if not path.is_absolute():
        path = REPO_ROOT / path
    if not path.exists():
        return []

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

        rules.append(AllowRule(pattern=token, line=None))

    return rules


def is_allowlisted(rel_path: str, line_no: int, rules: list[AllowRule]) -> bool:
    for rule in rules:
        if not fnmatch(rel_path, rule.pattern):
            continue
        if rule.line is None or rule.line == line_no:
            return True
    return False


def scan_file(path: Path, rules: list[AllowRule]) -> list[tuple[str, int, str]]:
    rel_path = path.relative_to(REPO_ROOT).as_posix()
    findings: list[tuple[str, int, str]] = []

    pending_cfg_test = False

    in_test_fn = False
    test_fn_depth = 0
    pending_test_fn = False

    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    for idx, raw_line in enumerate(lines, start=1):
        line = raw_line

        if re.search(r"#\s*\[\s*cfg\s*\([^]]*test", line):
            pending_cfg_test = True

        if pending_cfg_test and re.search(r"\bmod\s+\w+\s*\{", line):
            break

        if re.search(r"\bmod\s+\w*tests\w*\s*\{", line):
            break

        if re.search(r"#\s*\[\s*test\s*\]", line):
            pending_test_fn = True

        if pending_test_fn and re.search(r"\bfn\s+\w+\s*\([^)]*\)\s*\{", line):
            in_test_fn = True
            test_fn_depth = line.count("{") - line.count("}")
            pending_test_fn = False
            continue

        if in_test_fn:
            test_fn_depth += line.count("{") - line.count("}")
            if test_fn_depth <= 0:
                in_test_fn = False
            continue

        code_only = line.split("//", 1)[0]
        if not UNWRAP_EXPECT_RE.search(code_only):
            continue

        if is_allowlisted(rel_path, idx, rules):
            continue

        findings.append((rel_path, idx, raw_line.strip()))

    return findings


def main() -> int:
    args = parse_args()
    allow_rules = parse_allow_rules(args.allowlist)

    findings: list[tuple[str, int, str]] = []
    files = list_tracked_rust_files()

    scanned = 0
    for path in files:
        if is_test_path(path):
            continue
        scanned += 1
        findings.extend(scan_file(path, allow_rules))

    print(
        f"unwrap/expect scan summary: scanned_files={scanned}, findings={len(findings)}, mode={args.mode}"
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
            "\nunwrap/expect enforcement failed.\n\n"
            "Remediation:\n"
            "1. Replace unwrap()/expect() with anyhow::Result + with_context() in production code.\n"
            "2. If an exception is intentional, add a scoped entry to scripts/zen_allowlist.txt with rationale.\n"
            "3. Re-run: python3 scripts/check_no_unwrap_expect_prod.py --mode enforce --allowlist scripts/zen_allowlist.txt",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and findings:
        print(
            "\nWarning mode: findings reported but not blocking. "
            "Promote to --mode enforce after cleanup/allowlisting."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
