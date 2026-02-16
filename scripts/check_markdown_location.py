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
DOCS_TOP_LEVEL_ALLOWLIST = REPO_ROOT / "scripts" / "docs_top_level_allowlist.txt"


def load_docs_top_level_allowlist() -> set[str]:
    if not DOCS_TOP_LEVEL_ALLOWLIST.exists():
        return set()

    allowed: set[str] = set()
    for raw_line in DOCS_TOP_LEVEL_ALLOWLIST.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        normalized = line.replace("\\", "/").lstrip("./")
        allowed.add(normalized)
    return allowed


def main() -> int:
    root_markdown = sorted(REPO_ROOT.glob("*.md"))
    violations = [path.name for path in root_markdown if path.name not in ALLOWED_ROOT_MARKDOWN]
    allowlisted_docs = load_docs_top_level_allowlist()
    docs_top_level = sorted(REPO_ROOT.glob("docs/*.md"))
    docs_top_level_rel = {path.relative_to(REPO_ROOT).as_posix() for path in docs_top_level}
    docs_violations = sorted(path for path in docs_top_level_rel if path not in allowlisted_docs)
    stale_allowlist_entries = sorted(path for path in allowlisted_docs if path not in docs_top_level_rel)

    if not allowlisted_docs:
        print(
            "Missing docs top-level allowlist required by Documentation Location invariant (#7):\n"
            f"- {DOCS_TOP_LEVEL_ALLOWLIST.relative_to(REPO_ROOT)}\n\n"
            "Remediation:\n"
            "1. Create scripts/docs_top_level_allowlist.txt.\n"
            "2. Populate it with allowed docs/*.md paths.\n"
            "3. Re-run: python3 scripts/check_markdown_location.py",
            file=sys.stderr,
        )
        return 1

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

    if docs_violations:
        violation_text = "\n".join(f"- {name}" for name in docs_violations)
        print(
            "Found docs/*.md files not allowlisted by the Documentation Location "
            "invariant (#7):\n"
            f"{violation_text}\n\n"
            "Remediation:\n"
            "1. Move one-off docs into a domain directory (for example docs/features/) "
            "or docs/archive/.\n"
            "2. Keep only stable entrypoint docs at docs/*.md.\n"
            "3. If a docs/*.md file is intentionally top-level, add it to "
            "scripts/docs_top_level_allowlist.txt.\n"
            "4. Re-run: python3 scripts/check_markdown_location.py",
            file=sys.stderr,
        )
        return 1

    if stale_allowlist_entries:
        stale_text = "\n".join(f"- {name}" for name in stale_allowlist_entries)
        print(
            "Found stale entries in scripts/docs_top_level_allowlist.txt:\n"
            f"{stale_text}\n\n"
            "Remediation:\n"
            "1. Remove stale paths from scripts/docs_top_level_allowlist.txt.\n"
            "2. Re-run: python3 scripts/check_markdown_location.py",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
