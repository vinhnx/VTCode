#!/usr/bin/env python3
"""Lint tracked, maintained Markdown with the same pinned tool locally and in CI.

Exit codes: 0 = clean, 1 = lint findings, 2 = usage error or linter toolchain
unavailable (e.g. `npx` not on PATH). Exit 2 keeps a broken environment
distinct from failing content so harnesses do not re-run it as a code fix.
"""

import argparse
from pathlib import Path, PurePosixPath
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
EXCLUDED_FILES = {"docs/project/TODO.md"}
EXCLUDED_PARTS = {"fixtures", "snapshots", "embedded_assets_source"}
MARKDOWNLINT_CLI2_VERSION = "0.23.3"
MISSING_TOOLCHAIN_EXIT = 2


def linter_command() -> list[str]:
    """Build the pinned linter invocation, preferring the local install.

    `npm install` provides `markdownlint-cli2` under `node_modules`, which keeps
    the check offline-capable and avoids a registry fetch on every run. When the
    local package is absent (e.g. CI without a Node setup step), fall back to a
    pinned `npx` fetch so the same linter version still runs.
    """
    pinned = f"markdownlint-cli2@{MARKDOWNLINT_CLI2_VERSION}"
    if (ROOT / "node_modules" / "markdownlint-cli2" / "package.json").is_file():
        return ["npx", "--no-install", "--loglevel=error", pinned, "--config", ".markdownlint-cli2.jsonc"]
    return ["npx", "--yes", "--loglevel=error", pinned, "--config", ".markdownlint-cli2.jsonc"]


def is_maintained_markdown(path: str) -> bool:
    parts = PurePosixPath(path).parts
    return (
        path.endswith(".md")
        and path not in EXCLUDED_FILES
        and not path.startswith(("patches/", ".vtcode/reviews/"))
        and not EXCLUDED_PARTS.intersection(parts)
    )


def tracked_markdown() -> list[str]:
    root = ROOT.resolve()
    result = subprocess.run(
        ["git", "ls-files", "-z"], cwd=root, check=True, stdout=subprocess.PIPE
    )
    return sorted(
        path for path in result.stdout.decode().split("\0")
        if is_maintained_markdown(path)
        and (root / path).is_file()
        and (root / path).resolve() == root / path
    )


def run_linter(command: list[str]) -> int:
    """Run the pinned linter, translating a missing binary into exit 2.

    `npx` may be absent from PATH (GUI-launched processes, minimal CI
    images); an actionable toolchain error keeps that distinct from a lint
    failure (exit 1).
    """
    try:
        result = subprocess.run(command, cwd=ROOT, check=False)
    except FileNotFoundError:
        binary = command[0] if command else "npx"
        print(
            f"error: cannot run the Markdown linter: '{binary}' was not found on PATH. "
            "Install Node.js 22 or newer, run `npm install`, and ensure the "
            "executing process inherits a PATH containing npm binaries.",
            file=sys.stderr,
        )
        return MISSING_TOOLCHAIN_EXIT
    return result.returncode


def apply_path_scope(parser: argparse.ArgumentParser, files: list[str], requested: list[str]) -> list[str]:
    """Restrict the lint selection to requested paths, failing closed.

    Every requested path must already be in the maintained selection; anything
    else is a typo or an out-of-scope file, and silently linting nothing would
    mask it as a pass.
    """
    if not requested:
        return files
    selected = set(files)
    scoped = []
    for raw in requested:
        normalized = PurePosixPath(raw).as_posix()
        if normalized not in selected:
            parser.error(f"'{raw}' is not a tracked, maintained Markdown file")
        if normalized not in scoped:
            scoped.append(normalized)
    return scoped


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fix", action="store_true", help="Apply supported formatting fixes")
    parser.add_argument("--list", action="store_true", help="Print selected files without reading their contents")
    parser.add_argument("paths", nargs="*", help="Lint only these maintained Markdown paths (default: all tracked)")
    args = parser.parse_args()
    files = apply_path_scope(parser, tracked_markdown(), args.paths)
    if args.list:
        print("\n".join(files))
        return 0
    if not files:
        parser.error("No maintained Markdown files selected")
    command = linter_command()
    if args.fix:
        command.append("--fix")
    command.extend(":" + path for path in files)
    exit_code = run_linter(command)
    status = "passed" if exit_code == 0 else f"failed (exit {exit_code})"
    print(f"Markdown lint {status}: {len(files)} selected files.", flush=True)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
