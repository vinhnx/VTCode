#!/usr/bin/env python3
"""Lint tracked, maintained Markdown with the same pinned tool locally and in CI."""

import argparse
from pathlib import Path, PurePosixPath
import subprocess


ROOT = Path(__file__).resolve().parent.parent
EXCLUDED_FILES = {"docs/project/TODO.md"}
EXCLUDED_PARTS = {"fixtures", "snapshots", "embedded_assets_source"}


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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fix", action="store_true", help="Apply supported formatting fixes")
    parser.add_argument("--list", action="store_true", help="Print selected files without reading their contents")
    args = parser.parse_args()
    files = tracked_markdown()
    if args.list:
        print("\n".join(files))
        return 0
    if not files:
        parser.error("No maintained Markdown files selected")
    command = ["npx", "--yes", "--loglevel=error", "markdownlint-cli2@0.23.3", "--config", ".markdownlint-cli2.jsonc"]
    if args.fix:
        command.append("--fix")
    command.extend(":" + path for path in files)
    result = subprocess.run(command, cwd=ROOT, check=False)
    status = "passed" if result.returncode == 0 else f"failed (exit {result.returncode})"
    print(f"Markdown lint {status}: {len(files)} selected files.", flush=True)
    return result.returncode


if __name__ == "__main__":
    raise SystemExit(main())
