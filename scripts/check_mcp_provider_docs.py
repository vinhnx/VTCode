#!/usr/bin/env python3
"""Fail when a new third-party MCP provider guide or example is added.

Policy: docs/CONTRIBUTING.md#out-of-scope-contributions. The project no longer
accepts per-vendor MCP setup documentation because those guides rot as
endpoints, auth flows, and tool catalogs change, and vendor-authored docs are
indistinguishable from promotion.

This is a deterministic gate on purpose. The same rule also appears as review
guidance in `.coderabbit.yaml`, but LLM review is advisory and, on the Free plan,
does not run on pull requests at all. A repository policy needs an owner whose
verdict does not depend on plan tier or model output.

Scope is deliberately narrow so it cannot block legitimate work:

- Provider guides and examples that already exist are grandfathered. Editing or
  improving them is allowed; only *new* provider identities are rejected.
- The generic MCP surface stays open: `docs/guides/mcp-integration.md`,
  `docs/guides/mcp-interop.md`, and the `mcp` docs directory are not matched.
- Removing a stale guide is always allowed.

Usage: python3 scripts/check_mcp_provider_docs.py [--root PATH]
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent

GUIDE_DIR = "docs/guides"
EXAMPLE_DIR = "docs/examples"
GUIDE_SUFFIX = "-mcp.md"
EXAMPLE_SUFFIX = "-mcp.toml"

REMOVAL_HINT = (
    "Remediation:\n"
    "1. Move the setup guide to the vendor's own documentation site.\n"
    "2. Point users at the generic [[mcp.providers]] configuration in\n"
    "   docs/guides/mcp-integration.md instead.\n"
    "3. See docs/CONTRIBUTING.md#out-of-scope-contributions for the policy."
)


def tracked_provider_docs(root: Path) -> set[str]:
    """Provider guide/example paths that already exist in the committed tree.

    `git ls-tree HEAD` is used instead of `git ls-files` on purpose: `ls-files`
    also lists staged additions, which would let a newly `git add`-ed provider
    guide grandfather itself and slip through the pre-commit path.
    """
    return {
        _provider_key(path) for path in _committed_paths(root) if _provider_key(path)
    }


def tracked_provider_docs(root: Path, base: str | None = None) -> set[str]:
    """Provider guide/example paths that already exist in the comparison base.

    The baseline must come from the *base* revision, not `HEAD`. A provider guide
    committed on the branch is in `HEAD`, so using HEAD would grandfather the very
    addition this gate exists to reject, and a clean checkout of that branch would
    pass. `git ls-tree` is used instead of `git ls-files` for the same reason it
    replaced it earlier: `ls-files` also lists staged additions.
    """
    return {
        _provider_key(path)
        for path in _committed_paths(root, base)
        if _provider_key(path)
    }


def _resolve_base(root: Path, base: str | None) -> str | None:
    """The revision to compare against, or None when it cannot be determined.

    Order: an explicit base, the merge-base with the upstream default branch, then
    the root commit. Falling back to the root commit keeps the gate strict: every
    provider doc present at that point is treated as pre-existing, so a new one is
    still rejected even in a shallow or detached checkout.
    """
    if base:
        return base
    for candidate in ("origin/HEAD", "origin/main", "main"):
        result = subprocess.run(
            ["git", "merge-base", candidate, "HEAD"],
            cwd=root,
            capture_output=True,
        )
        if result.returncode == 0:
            return result.stdout.decode("utf-8", errors="ignore").strip() or None
    # No usable upstream: use the root commit so additions still stand out.
    result = subprocess.run(
        ["git", "rev-list", "--max-parents=0", "HEAD"],
        cwd=root,
        capture_output=True,
    )
    if result.returncode != 0:
        return None
    roots = result.stdout.decode("utf-8", errors="ignore").split()
    return roots[-1] if roots else None


def _committed_paths(root: Path, base: str | None = None) -> list[str]:
    revision = _resolve_base(root, base)
    if revision is None or not _has_head(root):
        return []
    result = subprocess.run(
        ["git", "ls-tree", "-r", "--name-only", "-z", revision],
        cwd=root,
        check=True,
        capture_output=True,
    )
    return [p for p in result.stdout.decode("utf-8", errors="ignore").split("\0") if p]


def _provider_key(relative: str) -> str | None:
    """Return the provider identity a path represents, or None if out of scope.

    Only a *new provider identity* is rejected. Guides and examples that already
    exist keep their key, so editing them is unaffected.
    """
    path = Path(relative)
    parent = path.parent.as_posix()
    name = path.name

    if parent == GUIDE_DIR and name.endswith(GUIDE_SUFFIX):
        provider = name[: -len(GUIDE_SUFFIX)]
    elif parent == EXAMPLE_DIR and name.endswith(EXAMPLE_SUFFIX):
        provider = name[: -len(EXAMPLE_SUFFIX)]
    else:
        return None

    return provider if provider else None


def find_violations(
    root: Path, staged_only: bool = False, base: str | None = None
) -> list[str]:
    """Provider docs added relative to the comparison point."""
    existing = tracked_provider_docs(root, base)

    if staged_only:
        added = _added_paths_staged(root)
    else:
        added = _added_paths_worktree(root)

    violations: list[str] = []
    for relative in added:
        key = _provider_key(relative)
        if key and key not in existing:
            violations.append(relative)
    return sorted(violations)


def _added_paths_staged(root: Path) -> list[str]:
    # `--no-renames` matters: with rename detection on, renaming `sem-mcp.md` to
    # `specstream-mcp.md` is reported as R, not A, so the destination would be
    # missed and a new provider identity would slip through.
    result = subprocess.run(
        [
            "git",
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "--diff-filter=A",
            "-z",
        ],
        cwd=root,
        check=True,
        capture_output=True,
    )
    return [p for p in result.stdout.decode("utf-8", errors="ignore").split("\0") if p]


def _added_paths_worktree(root: Path) -> list[str]:
    """Paths added in the working tree, compared against the base when it exists."""
    args = ["git", "diff", "--name-only", "--no-renames", "--diff-filter=A", "-z"]
    base = _resolve_base(root, None)
    if base is not None and _has_head(root):
        args.append(base)
    result = subprocess.run(args, cwd=root, check=True, capture_output=True)
    added = [p for p in result.stdout.decode("utf-8", errors="ignore").split("\0") if p]
    # `git diff HEAD` omits untracked files; include them so a new untracked
    # example cannot bypass the gate before it is staged.
    result = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"],
        cwd=root,
        check=True,
        capture_output=True,
    )
    untracked = [
        p for p in result.stdout.decode("utf-8", errors="ignore").split("\0") if p
    ]
    return added + untracked


def _is_git_repository(root: Path) -> bool:
    """True when `root` is inside a git checkout, including a linked worktree.

    `.git` is a directory in a normal clone but a file in a worktree, so the
    presence check must accept either.
    """
    if not (root / ".git").exists():
        return False
    result = subprocess.run(
        ["git", "rev-parse", "--git-dir"],
        cwd=root,
        capture_output=True,
    )
    return result.returncode == 0


def _has_head(root: Path) -> bool:
    result = subprocess.run(
        ["git", "rev-parse", "--verify", "--quiet", "HEAD"],
        cwd=root,
        capture_output=True,
    )
    return result.returncode == 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=REPO_ROOT,
        help="Repository root (defaults to this checkout).",
    )
    parser.add_argument(
        "--staged",
        action="store_true",
        help="Check staged changes only instead of the working tree.",
    )
    parser.add_argument(
        "--base",
        default=None,
        help=(
            "Revision treated as pre-existing (default: merge-base with "
            "origin/HEAD, origin/main, or main)."
        ),
    )
    args = parser.parse_args(argv)

    if not _is_git_repository(args.root):
        print(
            f"check_mcp_provider_docs: {args.root} is not a git repository.\n"
            "This gate compares additions against tracked files.",
            file=sys.stderr,
        )
        return 1

    violations = find_violations(args.root, staged_only=args.staged, base=args.base)
    if not violations:
        return 0

    listing = "\n".join(f"- {path}" for path in violations)
    print(
        "New third-party MCP provider documentation is out of scope:\n"
        f"{listing}\n\n"
        f"{REMOVAL_HINT}",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
