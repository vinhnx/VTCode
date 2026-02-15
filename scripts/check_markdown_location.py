#!/usr/bin/env python3
"""Ensure markdown documentation follows repository location rules."""

from __future__ import annotations

from pathlib import Path
import sys


REPO_ROOT = Path(__file__).resolve().parent.parent
ALLOWED_ROOT_MARKDOWN = {
    "README.md",
    "AGENTS.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "CHANGELOG.md",
}


def main() -> int:
    root_markdown = sorted(REPO_ROOT.glob("*.md"))
    violations = [path.name for path in root_markdown if path.name not in ALLOWED_ROOT_MARKDOWN]

    if violations:
        violation_text = "\n".join(f"- {name}" for name in violations)
        allowed_text = ", ".join(sorted(ALLOWED_ROOT_MARKDOWN))
        print(
            "Found markdown files in repository root that violate the "
            "Documentation Location invariant (#7):\n"
            f"{violation_text}\n\n"
            "Remediation:\n"
            "1. Move each file into an appropriate subdirectory under docs/.\n"
            "2. Keep only approved root markdown files:\n"
            f"   {allowed_text}\n"
            "3. Update references to moved files.",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
