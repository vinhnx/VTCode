#!/usr/bin/env python3
"""Validate local markdown links in maintained docs and agent instruction files.

Checks two things:
1. Every relative markdown link outside code spans and fenced code blocks resolves to an
   existing path, in every maintained markdown file (the same selection as
   `check_markdown.py`) plus the per-crate maintainer notes.
2. Per-module `AGENTS.md` files keep short lines, so the "about 30 short lines" budget
   cannot be met by packing many rules into one line.
"""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import sys


SCRIPTS_DIR = Path(__file__).resolve().parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))
from check_markdown import is_maintained_markdown  # noqa: E402  (shared file selection)

REPO_ROOT = SCRIPTS_DIR.parent
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
MAINTAINER_NOTES_DIR = "docs/development/maintainer-notes"
MAX_MODULE_AGENTS_LINE_CHARS = 600

MARKDOWN_LINK_RE = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
URI_SCHEME_RE = re.compile(r"^[a-zA-Z][a-zA-Z0-9+.-]*:")
FENCE_RE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
INLINE_CODE_RE = re.compile(r"(`+).*?\1")


def prose_lines(content: str):
    """Yield lines outside fenced code blocks with inline code spans blanked out.

    Links shown as examples inside code are not navigable and must not be checked.
    """
    fence: str | None = None
    for line in content.splitlines():
        match = FENCE_RE.match(line)
        if fence is None and match:
            fence = match.group(1)[0] * len(match.group(1))
            continue
        if fence is not None:
            if line.strip().startswith(fence):
                fence = None
            continue
        yield INLINE_CODE_RE.sub("", line)


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


def resolve_target(root: Path, doc_path: Path, target: str) -> Path:
    if target.startswith("/"):
        return root / target.lstrip("/")

    return (doc_path.parent / target).resolve()


def tracked_maintained_markdown(root: Path) -> list[str]:
    """Return tracked maintained markdown; untracked, vendored, and fixture files are skipped."""
    try:
        result = subprocess.run(
            ["git", "ls-files", "--", "*.md"],
            cwd=root,
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return []
    return [
        line for line in result.stdout.splitlines()
        if line and is_maintained_markdown(line) and (root / line).is_file()
    ]


def instruction_docs(root: Path) -> list[str]:
    notes_dir = root / MAINTAINER_NOTES_DIR
    notes = sorted(
        str(path.relative_to(root)) for path in notes_dir.glob("*.md")
    ) if notes_dir.is_dir() else []
    return sorted(set(tracked_maintained_markdown(root)) | set(notes))


def broken_links(root: Path, relative_docs: list[str]) -> list[tuple[str, str]]:
    broken: list[tuple[str, str]] = []
    for relative_doc in relative_docs:
        doc_path = root / relative_doc
        content = doc_path.read_text(encoding="utf-8")
        for line in prose_lines(content):
            for match in MARKDOWN_LINK_RE.finditer(line):
                target = normalize_target(match.group(1))
                if target is None:
                    continue
                if not resolve_target(root, doc_path, target).exists():
                    broken.append((relative_doc, target))
    return broken


def overlong_module_agents_lines(root: Path, relative_docs: list[str]) -> list[tuple[str, int, int]]:
    """Return (path, line number, length) for module `AGENTS.md` lines over the budget."""
    overlong: list[tuple[str, int, int]] = []
    for relative_doc in relative_docs:
        if relative_doc == "AGENTS.md" or not relative_doc.endswith("/AGENTS.md"):
            continue
        lines = (root / relative_doc).read_text(encoding="utf-8").splitlines()
        for number, line in enumerate(lines, start=1):
            if len(line) > MAX_MODULE_AGENTS_LINE_CHARS:
                overlong.append((relative_doc, number, len(line)))
    return overlong


def main(root: Path = REPO_ROOT) -> int:
    missing_entrypoints = [doc for doc in ENTRYPOINT_DOCS if not (root / doc).exists()]
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

    instruction = instruction_docs(root)
    docs = sorted(set(ENTRYPOINT_DOCS) | set(instruction))
    broken = broken_links(root, docs)
    if broken:
        broken_text = "\n".join(f"- {doc} -> {target}" for doc, target in broken)
        print(
            "Found broken local markdown links in maintained docs:\n"
            f"{broken_text}\n\n"
            "Remediation:\n"
            "1. Fix each link target, or update/remove stale links.\n"
            "2. Keep references relative to the source markdown file when possible.\n"
            "3. Re-run: python3 scripts/check_docs_links.py",
            file=sys.stderr,
        )
        return 1

    overlong = overlong_module_agents_lines(root, instruction)
    if overlong:
        overlong_text = "\n".join(f"- {doc}:{number} ({length} chars)" for doc, number, length in overlong)
        print(
            f"Found module AGENTS.md lines over {MAX_MODULE_AGENTS_LINE_CHARS} characters:\n"
            f"{overlong_text}\n\n"
            "Remediation:\n"
            "1. Keep the module map, rules, and gotchas as short lines.\n"
            f"2. Move long contracts and incident detail to {MAINTAINER_NOTES_DIR}/<crate>.md and link it.\n"
            "3. Re-run: python3 scripts/check_docs_links.py",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
