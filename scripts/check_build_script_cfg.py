#!/usr/bin/env python3
"""Flag host-target `cfg` checks in build scripts.

`cfg!` and `#[cfg(...)]` inside a ``build.rs`` are evaluated for the *host*
(the machine running Cargo), not for the target being compiled. Using them to
choose compiler or linker flags silently breaks cross-compilation: a Unix host
building for ``windows-msvc`` sees ``cfg!(windows) == false`` and passes the
GCC-style ``-std=c++11`` flag to MSVC, while a Windows host building for a GNU
target sees the opposite. Build scripts must read ``CARGO_CFG_TARGET_OS``,
``CARGO_CFG_TARGET_ENV``, and friends from the environment instead.

This gate scans tracked ``build.rs`` files for target-looking predicates in
``cfg!``, ``#[cfg(...)]`` and ``#[cfg_attr(...)]``. Host-independent predicates
(``feature``, ``test``, ``debug_assertions``, ``docsrs``) are allowed.

Usage:
    python3 scripts/check_build_script_cfg.py [--mode warn|enforce]
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# Predicate starts we care about. `CARGO_CFG_TARGET_OS` is deliberately NOT
# matched: reading it from the environment is the correct pattern. The word
# boundary keeps similarly named macros (`mycfg!`) from matching; `cfg_attr`
# has the same host-vs-target semantics as `cfg`.
PREDICATE_START_RE = re.compile(r"\bcfg!\s*\(|#\s*\[\s*cfg(?:_attr)?\s*\(")

# A Rust char literal, including escapes and unicode escapes. Used to tell a
# real `'x'` apart from a lifetime (`'a`, `'static`): masking from a lifetime's
# quote to the next quote would blank the rest of the file and hide findings.
CHAR_LITERAL_RE = re.compile(r"'(?:\\u\{[0-9A-Fa-f_]+\}|\\.|[^'\\\n])'")

# Target-looking cfg keys. `windows`/`unix` are included because they are the
# common shorthand forms of `target_family`.
TARGET_TOKEN_RE = re.compile(
    r"\b(?:target_os|target_arch|target_env|target_family|target_abi|"
    r"target_vendor|target_endian|target_pointer_width|windows|unix)\b"
)


@dataclass(frozen=True)
class Finding:
    path: str
    line: int
    column: int
    snippet: str

    def render(self) -> str:
        return (
            f"{self.path}:{self.line}:{self.column}: "
            f"host-target cfg check in build script: {self.snippet}\n"
            "    Read CARGO_CFG_TARGET_* from the environment instead."
        )


def mask_noise(text: str) -> str:
    """Blank out comments and string/char literals, keeping positions stable.

    Only the shape of the code matters for the scan; replacing masked content
    with spaces preserves offsets and line numbers so findings point at the
    real source location.
    """
    out = list(text)
    i = 0
    n = len(text)

    def blank(start: int, end: int) -> None:
        for j in range(start, end):
            if out[j] != "\n":
                out[j] = " "

    while i < n:
        c = text[i]
        if c == "/" and i + 1 < n and text[i + 1] == "/":
            end = text.find("\n", i)
            end = n if end < 0 else end
            blank(i, end)
            i = end
        elif c == "/" and i + 1 < n and text[i + 1] == "*":
            close = text.find("*/", i + 2)
            end = n if close < 0 else close + 2
            blank(i, end)
            i = end
        elif c == '"':
            start = i
            i += 1
            while i < n:
                if text[i] == "\\":
                    i += 2
                    continue
                if text[i] == '"':
                    i += 1
                    break
                i += 1
            blank(start, min(i, n))
        elif c == "'":
            literal = CHAR_LITERAL_RE.match(text, i)
            if literal:
                blank(i, literal.end())
                i = literal.end()
            else:
                # Lifetime, not a char literal. Leave it as code: treating it
                # as an unterminated literal would mask the rest of the file.
                i += 1
        else:
            i += 1

    return "".join(out)


def predicate_body(text: str, start: int) -> str:
    """Return the characters inside the parentheses of a cfg predicate."""
    open_paren = text.find("(", start)
    if open_paren < 0:
        return ""
    depth = 0
    for i in range(open_paren, len(text)):
        if text[i] == "(":
            depth += 1
        elif text[i] == ")":
            depth -= 1
            if depth == 0:
                return text[open_paren + 1 : i]
    return text[open_paren + 1 :]


def scan_text(text: str, path: str = "<memory>") -> list[Finding]:
    """Find target-looking cfg predicates in build-script source text."""
    masked = mask_noise(text)
    findings: list[Finding] = []
    for match in PREDICATE_START_RE.finditer(masked):
        body = predicate_body(masked, match.start())
        if not TARGET_TOKEN_RE.search(body):
            continue
        line = masked.count("\n", 0, match.start()) + 1
        column = match.start() - (masked.rfind("\n", 0, match.start()) + 1) + 1
        snippet = " ".join(text[match.start() : match.end() + len(body) + 1].split())
        findings.append(Finding(path=path, line=line, column=column, snippet=snippet))
    return findings


def list_build_scripts(repo_root: Path) -> list[Path]:
    """List tracked build.rs files (root and nested) in a repository."""
    result = subprocess.run(
        ["git", "ls-files"],
        cwd=repo_root,
        check=True,
        capture_output=True,
        text=True,
    )
    scripts = []
    for rel in result.stdout.splitlines():
        if rel != "build.rs" and not rel.endswith("/build.rs"):
            continue
        path = repo_root / rel
        # `git ls-files` also lists tracked files deleted in the working tree;
        # reading those would crash the gate with a traceback.
        if path.is_file():
            scripts.append(path)
    return scripts


def scan_repo(repo_root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for script in list_build_scripts(repo_root):
        rel = script.relative_to(repo_root).as_posix()
        findings.extend(scan_text(script.read_text(encoding="utf-8"), rel))
    return findings


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("warn", "enforce"), default="warn")
    parser.add_argument("--repo-root", type=Path, default=REPO_ROOT)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    findings = scan_repo(args.repo_root)
    if findings:
        for finding in findings:
            print(finding.render())
        print(
            f"\n{len(findings)} host-target cfg check(s) in build scripts.",
            file=sys.stderr,
        )
        return 1 if args.mode == "enforce" else 0
    print("No host-target cfg checks in build scripts.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
