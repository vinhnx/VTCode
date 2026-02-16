#!/usr/bin/env python3
"""Validate local markdown links in core documentation entrypoints."""

from __future__ import annotations

from pathlib import Path
import re
import sys


REPO_ROOT = Path(__file__).resolve().parent.parent
ENTRYPOINT_DOCS = (
    "AGENTS.md",
    "README.md",
    "docs/README.md",
    "docs/INDEX.md",
    "docs/harness/INDEX.md",
    "docs/harness/QUALITY_SCORE.md",
    "docs/harness/TECH_DEBT_TRACKER.md",
    "docs/harness/ARCHITECTURAL_INVARIANTS.md",
)

MARKDOWN_LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
URI_SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")


def normalize_target(raw_target: str) -> str | None:
    target = raw_target.strip()
    if not target:
        return None

    if target.startswith("<") and target.endswith(">"):
        target = target[1:-1].strip()

    if not target or target.startswith("#") or URI_SCHEME_RE.match(target):
        return None

    target = target.split("#", 1)[0].strip()
    if not target:
        return None

    return target.replace("%20", " ")


def resolve_target(doc_path: Path, target: str) -> Path:
    if target.startswith("/"):
        return REPO_ROOT / target.lstrip("/")

    return (doc_path.parent / target).resolve()


def main() -> int:
    missing_entrypoints: list[str] = []
    broken_links: list[tuple[str, str]] = []

    for relative_doc in ENTRYPOINT_DOCS:
        doc_path = REPO_ROOT / relative_doc
        if not doc_path.exists():
            missing_entrypoints.append(relative_doc)
            continue

        content = doc_path.read_text(encoding="utf-8")
        for match in MARKDOWN_LINK_RE.finditer(content):
            target = normalize_target(match.group(1))
            if target is None:
                continue

            resolved = resolve_target(doc_path, target)
            if not resolved.exists():
                broken_links.append((relative_doc, target))

    if missing_entrypoints:
        missing_text = "\n".join(f"- {path}" for path in missing_entrypoints)
        print(
            "Missing required documentation entrypoint files:\n"
            f"{missing_text}\n\n"
            "Remediation:\n"
            "1. Restore or create the missing docs listed above.\n"
            "2. Re-run: python3 scripts/check_docs_links.py",
            file=sys.stderr,
        )
        return 1

    if broken_links:
        broken_text = "\n".join(f"- {doc} -> {target}" for doc, target in broken_links)
        print(
            "Found broken local markdown links in core docs entrypoints:\n"
            f"{broken_text}\n\n"
            "Remediation:\n"
            "1. Fix each link target, or update/remove stale links.\n"
            "2. Keep references relative to the source markdown file when possible.\n"
            "3. Re-run: python3 scripts/check_docs_links.py",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
