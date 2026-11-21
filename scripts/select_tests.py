#!/usr/bin/env python3
"""Select the cargo packages whose tests a pull-request diff can reach.

Selection is the transitive reverse-dependency closure: a changed package runs
its own tests, and so does every package that depends on it through any normal,
dev, or build edge (dev/build edges count because integration tests link dev
dependencies and proc-macro rebuilds invalidate downstream builds). Selection
only narrows PR feedback; pushes to main and manual dispatches always run the
full workspace suite.

Inputs that rerun everything (fail-safe direction — over-selection never hides
a test; under-selection would):

- any ``Cargo.toml`` (manifest re-resolution changes the graph itself)
- ``Cargo.lock`` (any dependency change can reach any package)
- ``rust-toolchain.toml`` (compiler change invalidates every crate)
- ``.cargo/config.toml`` / ``.cargo/config`` (build flags apply everywhere)
- ``.github/workflows/ci.yml`` (the gate definition itself changed)
- ``.config/nextest.toml`` (test profile, filters, or groups changed)

Usage:
    python3 scripts/select_tests.py --base <sha>   # PR mode, prints "-p a -p b"
    python3 scripts/select_tests.py --explain-rerun-all
    python3 scripts/select_tests.py --json --base <sha>  # machine-readable

The closure logic (``build_reverse_graph`` / ``reachable_from`` /
``select_for_files``) is pure and covered by scripts/tests/test_select_tests.py.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys

# Files that force the full workspace suite regardless of the diff.
RERUN_ALL_PATHS = frozenset(
    {
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        ".cargo/config",
        ".github/workflows/ci.yml",
        ".config/nextest.toml",
    }
)

DependencyKind = str | None


def load_metadata() -> dict:
    """Return parsed ``cargo metadata --locked --no-deps`` output."""
    output = subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        text=True,
    )
    return json.loads(output)


def package_root(manifest_path: str) -> str:
    return os.path.dirname(manifest_path)


def build_reverse_graph(packages: list[dict]) -> dict[str, set[str]]:
    """Map each package to the set of packages that depend on it directly.

    Dev and build edges count like normal edges: a changed dev dependency must
    re-run the dependents' integration tests, and a changed build dependency
    (proc macro) invalidates every downstream build.
    """
    dependents: dict[str, set[str]] = {}
    for package in packages:
        name = package["name"]
        for dependency in package.get("dependencies", []):
            # Metadata dependency names are crate names; workspace packages are
            # named identically, so the plain name match is the graph edge.
            dependents.setdefault(dependency["name"], set()).add(name)
    return dependents


def reachable_from(changed: set[str], reverse_graph: dict[str, set[str]]) -> set[str]:
    """Return the changed packages plus everything that transitively depends on them."""
    reached: set[str] = set()
    frontier = list(changed)
    while frontier:
        name = frontier.pop()
        if name in reached:
            continue
        reached.add(name)
        frontier.extend(reverse_graph.get(name, ()))
    return reached


def rerun_all_reason(changed_files: list[str]) -> str | None:
    """Return which rerun-everything input the diff touched, if any."""
    for path in changed_files:
        normalized = path.replace(os.sep, "/")
        if normalized == "Cargo.lock" or normalized.endswith("/Cargo.lock"):
            return f"lockfile changed: {normalized}"
        if normalized == "rust-toolchain.toml" or normalized.endswith("/rust-toolchain.toml"):
            return f"toolchain pin changed: {normalized}"
        if normalized.endswith("/Cargo.toml") or normalized == "Cargo.toml":
            return f"manifest changed: {normalized}"
        if normalized in RERUN_ALL_PATHS:
            return f"gate input changed: {normalized}"
    return None


def files_to_packages(
    changed_files: list[str],
    packages: list[dict],
    repo_root: str,
) -> set[str]:
    """Map each changed file to its owning package (nearest manifest ancestor)."""
    roots = {package_root(p["manifest_path"]): p["name"] for p in packages}
    selected: set[str] = set()
    for path in changed_files:
        directory = os.path.dirname(os.path.join(repo_root, os.fsdecode(path)))
        while True:
            name = roots.get(directory)
            if name is not None:
                selected.add(name)
                break
            parent = os.path.dirname(directory)
            if parent == directory or len(parent) < len(repo_root):
                # Outside every package (docs/, scripts/, .github/): no tests
                # to reach from this file.
                break
            directory = parent
    return selected


def select_for_files(changed_files: list[str], packages: list[dict], repo_root: str) -> tuple[set[str], str | None]:
    """Pure selection core: (reachable package names, rerun-all reason)."""
    reason = rerun_all_reason(changed_files)
    if reason is not None:
        return {p["name"] for p in packages}, reason
    changed = files_to_packages(changed_files, packages, repo_root)
    if not changed:
        return set(), None
    reverse_graph = build_reverse_graph(packages)
    return reachable_from(changed, reverse_graph), None


def format_package_args(selected: set[str]) -> str:
    """Render the selection as nextest ``-p`` arguments, sorted for stable logs."""
    return " ".join(f"-p {name}" for name in sorted(selected))


def changed_files_for_base(base: str, repo_root: str) -> list[str]:
    """Changed files between the base snapshot and the current checkout.

    Two-dot diff on purpose: if main moved past the recorded base, its changes
    also appear here, widening selection. Over-selection never hides a test.
    Deleted paths are kept (``--no-renames``) so a moved file selects both its
    old and new package. Git runs with ``cwd=repo_root`` so returned paths are
    repo-root-relative regardless of the invocation directory; callers join
    them onto ``repo_root``.
    """
    output = subprocess.check_output(
        ["git", "diff", "--no-renames", "--name-only", "-z", f"{base}..HEAD"],
        cwd=repo_root,
    )
    files = [path for path in output.decode(errors="replace").split("\0") if path]
    # Untracked files are invisible to git diff but are part of the change in
    # local runs; in CI the checkout is clean, so this list is empty there.
    untracked = subprocess.check_output(
        ["git", "ls-files", "-z", "--others", "--exclude-standard"],
        cwd=repo_root,
    )
    files += [path for path in untracked.decode(errors="replace").split("\0") if path]
    return files


def explain_rerun_all() -> None:
    print("Inputs that rerun the full workspace suite on PRs:")
    for path in sorted(RERUN_ALL_PATHS):
        print(f"  {path}")
    print("  any Cargo.toml (manifest re-resolution)")
    print("  any Cargo.lock (dependency graph may reach any package)")
    print("  any rust-toolchain.toml (compiler change reaches every crate)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", help="base git sha to diff against (PR mode)")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of -p args")
    parser.add_argument(
        "--explain-rerun-all",
        action="store_true",
        help="list the inputs that force the full suite",
    )
    args = parser.parse_args()

    if args.explain_rerun_all:
        explain_rerun_all()
        return 0

    if not args.base:
        parser.error("--base is required (use --explain-rerun-all to inspect rerun inputs)")

    metadata = load_metadata()
    packages = metadata["packages"]
    repo_root = os.path.commonpath([package_root(p["manifest_path"]) for p in packages])

    changed = changed_files_for_base(args.base, repo_root)
    selected, reason = select_for_files(changed, packages, repo_root)

    if args.json:
        print(
            json.dumps(
                {
                    "base": args.base,
                    "changed_file_count": len(changed),
                    "rerun_all_reason": reason,
                    "packages": sorted(selected),
                }
            )
        )
        return 0

    if reason is not None:
        print(f"::notice::Full test suite selected — {reason}", file=sys.stderr)
    print(format_package_args(selected))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
