#!/usr/bin/env python3
"""Warn or enforce against panic macros in Result-returning production functions.

Clippy's `panic_in_result_fn` is the compile-grounded version of this rule, but
it misses `debug_assert!` (which panics here: the release profile keeps debug
assertions on) and `allow-panic-in-tests` does not suppress it: enabling that
lint workspace-wide produces ~500 findings in `Result<()>` test functions and
zero in production. This gate applies the rule to production code with a
brace-aware scan that skips whole test modules by depth instead of truncating
the file, so test code never fires and production code after a test module is
still checked.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

from check_no_unwrap_expect_prod import (
    is_allowlisted,
    is_test_path,
    list_tracked_rust_files,
    parse_allow_rules,
)

REPO_ROOT = Path(__file__).resolve().parent.parent
PANIC_MACRO_RE = re.compile(
    r"\b(?:panic|assert|assert_eq|assert_ne|debug_assert|debug_assert_eq|debug_assert_ne|unreachable|todo|unimplemented)!\s*\("
)
FN_START_RE = re.compile(r"\b(?:async\s+)?fn\s+\w+")
CFG_TEST_RE = re.compile(r"#\s*\[\s*cfg\s*\((?!not\s*\()[^]]*test")
TEST_ATTR_RE = re.compile(r"#\s*\[\s*(?:[^\]]*::)?test\s*\]")
TEST_MOD_RE = re.compile(r"\bmod\s+\w+\s*\{")
PLAIN_TEST_MOD_RE = re.compile(r"\bmod\s+\w*tests\w*\s*\{")
# The return type may follow the parameter list, so TEST_FN_RE accepts `-> …`.
TEST_FN_RE = re.compile(r"\bfn\s+\w+\s*\([^)]*\)(?:\s*->[^{;]+)?\s*\{")
STRING_RE = re.compile(r'"(?:\\.|[^"\\])*"')
CHAR_RE = re.compile(r"'(?:\\.|[^'\\])'")
# Only a leading Result path counts as the return type: `impl Iterator<Item =
# Result<…>>` and `T where T: Into<Result<…>>` must not be treated as Results.
RESULT_TYPE_RE = re.compile(r"^(?:[A-Za-z_]\w*::)*Result\b")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate production Result-returning functions do not panic.",
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
        help="Allowlist shared with the unwrap/expect scan (default: scripts/zen_allowlist.txt).",
    )
    parser.add_argument(
        "--max-findings",
        type=int,
        default=200,
        help="Maximum number of findings to print (default: 200).",
    )
    return parser.parse_args()


def code_only(line: str) -> str:
    """Strip string/char literals and line comments before scanning structure.

    A brace inside a string would otherwise skew the depth counters, and `//`
    inside a URL string would otherwise truncate the line mid-signature.
    """
    return STRING_RE.sub('""', CHAR_RE.sub("''", line)).split("//", 1)[0]


def is_result_signature(signature: str) -> bool:
    before_after = signature.split("->", 1)
    if len(before_after) != 2:
        return False
    return_type = before_after[1].split("where", 1)[0].strip()
    return bool(RESULT_TYPE_RE.match(return_type))


def scan_file(path: Path, rules: list) -> list[tuple[str, int, str]]:
    rel_path = path.relative_to(REPO_ROOT).as_posix()
    findings: list[tuple[str, int, str]] = []

    pending_cfg_test = False
    skip_depth = 0
    pending_mod_brace = False
    pending_test_fn = False
    in_test_fn = False
    test_fn_depth = 0

    signature = ""
    collecting = False
    in_result_fn = False
    result_depth = 0

    def record(line_no: int, raw_line: str) -> None:
        if not is_allowlisted(rel_path, line_no, rules):
            findings.append((rel_path, line_no, raw_line.strip()))

    def enter_skipped_module(code: str) -> None:
        nonlocal skip_depth, pending_mod_brace
        opens = code.count("{") - code.count("}")
        if "{" in code:
            # Fully contained (`mod tests { fn t() {} }`) closes to zero; an
            # unbalanced line leaves the remaining depth to track below.
            skip_depth = max(opens, 0)
        else:
            # Opening brace is on a later line.
            pending_mod_brace = True

    lines = path.read_text(encoding="utf-8", errors="ignore").splitlines()
    for idx, raw_line in enumerate(lines, start=1):
        code = code_only(raw_line)

        if pending_mod_brace:
            if "{" in code:
                pending_mod_brace = False
                skip_depth = code.count("{") - code.count("}")
            continue

        if skip_depth:
            skip_depth += code.count("{") - code.count("}")
            if skip_depth <= 0:
                skip_depth = 0
            continue

        if CFG_TEST_RE.search(raw_line):
            pending_cfg_test = True

        if pending_cfg_test:
            if TEST_MOD_RE.search(code):
                pending_cfg_test = False
                enter_skipped_module(code)
                continue
            # `#[cfg(test)]` directly on a function (not a module).
            if TEST_FN_RE.search(code):
                pending_cfg_test = False
                in_test_fn = True
                test_fn_depth = code.count("{") - code.count("}")
                continue
            if not re.search(r"#\s*\[", raw_line):
                pending_cfg_test = False

        if PLAIN_TEST_MOD_RE.search(code):
            enter_skipped_module(code)
            continue

        if TEST_ATTR_RE.search(raw_line):
            pending_test_fn = True
        if pending_test_fn and TEST_FN_RE.search(code):
            in_test_fn = True
            test_fn_depth = code.count("{") - code.count("}")
            pending_test_fn = False
            continue

        if in_test_fn:
            test_fn_depth += code.count("{") - code.count("}")
            if test_fn_depth <= 0:
                in_test_fn = False
            continue

        if in_result_fn:
            result_depth += code.count("{") - code.count("}")
            if result_depth <= 0:
                in_result_fn = False
            if PANIC_MACRO_RE.search(code):
                record(idx, raw_line)
            continue

        if collecting:
            signature = f"{signature} {code}".strip()
            if "{" in code:
                collecting = False
                if is_result_signature(signature):
                    in_result_fn = True
                    result_depth = code.count("{") - code.count("}")
                    if PANIC_MACRO_RE.search(code):
                        record(idx, raw_line)
                signature = ""
            elif ";" in code:
                # Trait method declaration: no body, nothing to guard.
                collecting = False
                signature = ""
            continue

        if FN_START_RE.search(code):
            signature = code
            collecting = True
            if "{" in code:
                collecting = False
                if is_result_signature(signature):
                    in_result_fn = True
                    result_depth = code.count("{") - code.count("}")
                    if PANIC_MACRO_RE.search(code):
                        record(idx, raw_line)
                signature = ""
            elif ";" in code:
                # Trait method declaration (`fn f() -> Result<()>;`).
                collecting = False
                signature = ""

    return findings


def main() -> int:
    args = parse_args()
    rules = parse_allow_rules(args.allowlist)
    findings: list[tuple[str, int, str]] = []
    scanned = 0
    skipped_missing = 0

    for path in list_tracked_rust_files():
        if is_test_path(path):
            continue
        try:
            findings.extend(scan_file(path, rules))
        except OSError:
            skipped_missing += 1
            continue
        scanned += 1

    print(
        f"panic-in-Result scan summary: scanned_files={scanned}, findings={len(findings)}, "
        f"skipped_missing={skipped_missing}, mode={args.mode}"
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
            "\npanic-in-Result enforcement failed.\n\n"
            "Remediation:\n"
            "1. Return Err(...) from the Result instead of panicking or asserting.\n"
            "2. If the panic is a proven invariant guard, narrow the function's return type\n"
            "   or add a line-scoped entry to scripts/zen_allowlist.txt with rationale.\n"
            "3. Re-run: python3 scripts/check_no_panic_in_result_fn.py --mode enforce",
            file=sys.stderr,
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
