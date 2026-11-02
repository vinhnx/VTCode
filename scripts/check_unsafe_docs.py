#!/usr/bin/env python3
"""Gate undocumented / unallowed `unsafe` in platform-gated Rust code.

Clippy's `unsafe_code` and `undocumented_unsafe_blocks` lints only see code that
compiles on the checking host. Code behind `#[cfg(target_os = "...")]` for
another platform (for example the Linux Landlock launcher or the Windows
`user32` reduced-motion probe) is skipped locally and only surfaces when the
matching CI job happens to run with `-D warnings`. When that gate is dormant,
the violation sits invisible.

This deterministic, host-runnable check reproduces the two obligations on every
tracked `.rs` file, whether or not it compiles here:

* every file (or its crate root) that emits `unsafe_code` must carry an
  `unsafe_code` allow/expect,
* every `unsafe { ... }` / `unsafe impl` must be preceded by a `SAFETY:` marker, and
* every type-position `unsafe` (e.g. `type Cb = unsafe extern "C" fn()`) must be
  preceded by a contract doc comment (`///`).

The first two rules mirror clippy and are intentionally *more permissive* — a
marker anywhere in the preceding window counts, and `unsafe extern` blocks and
`unsafe trait` need no marker — so clippy-clean host code never trips them. The
third rule covers a class clippy cannot see at all: `unsafe` in type position
fires neither `unsafe_code` nor `undocumented_unsafe_blocks` (and clippy rejects a
`// SAFETY:` marker there as `unnecessary_safety_comment`), so it must carry a
`///` contract doc instead. It exists to catch the drift clippy cannot see; treat
findings as real gaps.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Any construct that fires `rustc`'s `unsafe_code` lint.
UNSAFE_USE_RE = re.compile(r"\bunsafe\s*(?:\{|impl\b|trait\b|extern\b|fn\b)")
# Constructs for which clippy's `undocumented_unsafe_blocks` requires a marker.
# (`unsafe extern` blocks need none; `unsafe trait` uses the separate
# `# Safety` doc-section lint instead.)
UNSAFE_MARKER_RE = re.compile(r"\bunsafe\s*(?:\{|impl\b)")
# `type Cb = unsafe extern "C" fn();` is type position: `unsafe_code` never fires.
TYPE_ALIAS_START_RE = re.compile(r"^\s*(?:pub(?:\s*\([^)]*\))?\s+)?type\s+\w")
ALLOW_RE = re.compile(r"#!?\[[^\]]*?\bunsafe_code\b", re.DOTALL)
SAFETY_MARKER_RE = re.compile(r"(?i)//.*\bsafety\s*:")
# Rule C: type-position `unsafe` fires no lint, so a contract doc comment above
# the `type` statement stands in for the obligation. (A `// SAFETY:` marker is
# rejected by clippy's `unnecessary_safety_comment`, so only `///` counts.)
TYPE_UNSAFE_RE = re.compile(r"\bunsafe\b")
TYPE_CONTRACT_DOC_RE = re.compile(r"^\s*(?:///|/\*\*)")
MARKER_WINDOW = 12

EXCLUDED_PREFIXES = ("fuzz/", "target/", "patches/")
EXCLUDED_SEGMENTS = ("/tests/", "/benches/", "/examples/")


@dataclass
class AllowRule:
    pattern: str
    line: int | None = None


@dataclass
class Finding:
    rel_path: str
    line: int
    reason: str
    source: str


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate every `unsafe` carries an `unsafe_code` allowance and a SAFETY marker.",
    )
    parser.add_argument(
        "--mode",
        choices=("warn", "enforce"),
        default="warn",
        help="warn: report only; enforce: fail on findings (default: warn).",
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=Path("scripts/unsafe_allowlist.txt"),
        help="Allowlist file with path or path:line patterns (default: scripts/unsafe_allowlist.txt).",
    )
    parser.add_argument(
        "--max-findings",
        type=int,
        default=200,
        help="Maximum number of findings to print (default: 200).",
    )
    parser.add_argument(
        "paths",
        nargs="*",
        type=Path,
        help="Optional files/directories to scan (default: tracked *.rs under the repo).",
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


def is_excluded(path: Path) -> bool:
    try:
        rel = path.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return False
    return rel.startswith(EXCLUDED_PREFIXES) or any(
        seg in f"/{rel}" for seg in EXCLUDED_SEGMENTS
    )


def display_path(path: Path) -> str:
    try:
        return path.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def expand_paths(paths: list[Path]) -> list[Path]:
    if not paths:
        return [p for p in list_tracked_rust_files() if not is_excluded(p)]
    files: list[Path] = []
    for raw in paths:
        path = raw if raw.is_absolute() else REPO_ROOT / raw
        if path.is_dir():
            files.extend(sorted(p for p in path.rglob("*.rs") if not is_excluded(p)))
        elif path.suffix == ".rs":
            files.append(path)
    return files


def crate_root(path: Path) -> Path | None:
    for parent in path.parents:
        if parent.name == "src":
            for candidate in ("lib.rs", "main.rs"):
                root = parent / candidate
                if root.exists():
                    return root
            return None
    return None


def has_allowance(text: str) -> bool:
    return ALLOW_RE.search(text) is not None


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
    candidates = {rel_path}
    if rel_path.startswith("crates/"):
        parts = rel_path.split("/", 2)
        if len(parts) > 2:
            candidates.add(parts[2])
    for rule in rules:
        for candidate in candidates:
            if not fnmatch(candidate, rule.pattern):
                continue
            if rule.line is None or rule.line == line_no:
                return True
    return False


def code_only(line: str) -> str:
    return line.split("//", 1)[0]


def first_unsafe_code_line(lines: list[str]) -> int | None:
    """First line that emits `unsafe_code`, ignoring `type`-alias position.

    `unsafe extern "C" fn()` / `unsafe fn()` in a type alias does not fire the
    lint, so the whole `type ... ;` statement is skipped (single- or multi-line).
    """
    in_type_alias = False
    for idx, line in enumerate(lines, start=1):
        code = code_only(line)
        if TYPE_ALIAS_START_RE.match(code):
            in_type_alias = True
        if in_type_alias:
            if ";" in code:
                in_type_alias = False
            continue
        if UNSAFE_USE_RE.search(code):
            return idx
    return None


def iter_type_unsafe_statements(lines: list[str]):
    """Yield (start_line_no, source) for `type` statements that use `unsafe`.

    Matches a single- or multi-line `type ... ;` statement in type position,
    which `unsafe_code` and `undocumented_unsafe_blocks` both ignore. `source`
    is the first source line, for reporting.
    """
    idx = 0
    total = len(lines)
    while idx < total:
        if not TYPE_ALIAS_START_RE.match(code_only(lines[idx])):
            idx += 1
            continue
        start = idx + 1
        parts = []
        while idx < total:
            part = code_only(lines[idx])
            parts.append(part)
            if ";" in part:
                break
            idx += 1
        if TYPE_UNSAFE_RE.search(" ".join(parts)):
            yield start, lines[start - 1].strip()
        idx += 1


def scan_file(path: Path, rules: list[AllowRule]) -> list[Finding]:
    rel_path = display_path(path)
    try:
        text = path.read_text(encoding="utf-8", errors="ignore")
    except OSError:
        return []

    lines = text.splitlines()
    findings: list[Finding] = []

    # Rule A: a file that emits `unsafe_code` needs an allowance in the file or
    # its crate root. Report a single finding per file at the first offending line.
    if not has_allowance(text):
        root = crate_root(path)
        root_allows = (
            root is not None
            and root != path
            and has_allowance(root.read_text(encoding="utf-8", errors="ignore"))
        )
        if not root_allows:
            first = first_unsafe_code_line(lines)
            if first is not None and not is_allowlisted(rel_path, first, rules):
                findings.append(
                    Finding(
                        rel_path,
                        first,
                        "missing `unsafe_code` allowance",
                        lines[first - 1].strip(),
                    )
                )

    # Rule B: each marker-requiring `unsafe` needs a preceding SAFETY marker.
    for idx, raw_line in enumerate(lines):
        if not UNSAFE_MARKER_RE.search(code_only(raw_line)):
            continue
        line_no = idx + 1
        window = lines[max(0, idx - MARKER_WINDOW) : idx]
        if any(SAFETY_MARKER_RE.search(candidate) for candidate in window):
            continue
        if is_allowlisted(rel_path, line_no, rules):
            continue
        findings.append(
            Finding(rel_path, line_no, "missing SAFETY marker", raw_line.strip())
        )

    # Rule C: type-position `unsafe` fires no lint, so require a contract doc.
    for start_line, source in iter_type_unsafe_statements(lines):
        window = lines[max(0, start_line - 1 - MARKER_WINDOW) : start_line - 1]
        if any(TYPE_CONTRACT_DOC_RE.match(candidate) for candidate in window):
            continue
        if is_allowlisted(rel_path, start_line, rules):
            continue
        findings.append(
            Finding(
                rel_path,
                start_line,
                "type-position `unsafe` needs a contract doc",
                source,
            )
        )

    return findings


def main() -> int:
    args = parse_args()
    rules = parse_allow_rules(args.allowlist)
    files = expand_paths(args.paths)

    findings: list[Finding] = []
    for path in files:
        findings.extend(scan_file(path, rules))

    findings.sort(key=lambda f: (f.rel_path, f.line))
    print(
        f"unsafe docs scan summary: scanned_files={len(files)}, findings={len(findings)}, mode={args.mode}"
    )

    if findings:
        print("\nFindings (path:line | reason | source):")
        for finding in findings[: args.max_findings]:
            print(
                f"- {finding.rel_path}:{finding.line} | {finding.reason} | {finding.source}"
            )
        if len(findings) > args.max_findings:
            remaining = len(findings) - args.max_findings
            print(f"- ... {remaining} additional finding(s) not shown")

    if args.mode == "enforce" and findings:
        print(
            "\nunsafe docs enforcement failed.\n\n"
            "Remediation:\n"
            "1. Add an adjacent `// SAFETY:` comment stating the precondition that makes the\n"
            '   `unsafe` operation sound, and/or an `#[allow(unsafe_code, reason = "...")]`\n'
            "   (prefer `#[expect(...)]` on a cfg-gated item) on the enclosing item.\n"
            '2. For type-position `unsafe` (`type Cb = unsafe extern "C" fn()`), add a `///`\n'
            "   contract doc comment above the `type` statement (a `// SAFETY:` marker is\n"
            "   rejected there by `clippy::unnecessary_safety_comment`).\n"
            "3. Platform-gated code is not compiled on the host: run this check locally instead\n"
            "   of relying on `cargo clippy`.\n"
            "4. For a deliberate exception, add a scoped rule to scripts/unsafe_allowlist.txt.\n"
            "5. Re-run: python3 scripts/check_unsafe_docs.py --mode enforce",
            file=sys.stderr,
        )
        return 1

    if args.mode == "warn" and findings:
        print(
            "\nWarning mode: findings reported but not blocking. "
            "Promote to --mode enforce after fixing/allowlisting."
        )

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
