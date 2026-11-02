#!/usr/bin/env python3
"""Derive the crates.io publish order from the workspace dependency graph.

`scripts/publish_extracted_crates.sh` lists the crates that are published to
crates.io, but the order they must be published in is a property of the
workspace dependency graph, not of that list. A hand-maintained sequence
silently rots whenever a dependency edge is added: `vtcode-commons` gaining a
dependency on `vtcode-exec-events` is exactly how the 0.180.4 release aborted
with "publish order violates dependency graph".

This helper resolves the graph from `cargo metadata` and returns a
topological order, so the publish order always follows the graph.

Usage:
    publish_order.py plan --crates CRATE [CRATE ...]
    publish_order.py deps --crates CRATE [CRATE ...]

`plan` prints the publish order (one crate per line) and fails when the
publish list is inconsistent with the workspace graph.
`deps` prints the workspace dependencies of the given crates as
`crate<TAB>dependency` lines.
"""

from __future__ import annotations

import argparse
import heapq
import json
import subprocess
import sys


def load_metadata(metadata_path: str | None) -> dict:
    """Return `cargo metadata --no-deps` output, optionally from a cached file."""
    if metadata_path:
        with open(metadata_path, encoding="utf-8") as handle:
            return json.load(handle)
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        print(
            f"ERROR: 'cargo metadata --no-deps' failed: {result.stderr.strip()}",
            file=sys.stderr,
        )
        raise SystemExit(1)
    return json.loads(result.stdout)


def workspace_packages(metadata: dict) -> dict[str, dict]:
    """Map workspace member name to its metadata package entry."""
    return {package["name"]: package for package in metadata["packages"]}


def package_dependencies(package: dict, packages: dict[str, dict]) -> set[str]:
    """Workspace members the package depends on (normal, build, and dev deps).

    Dev dependencies count: `cargo publish` resolves them while verifying the
    package, so they must exist on crates.io before the dependent is published.
    """
    names: set[str] = set()
    for dependency in package.get("dependencies", []):
        name = dependency["name"]
        if name != package["name"] and name in packages:
            names.add(name)
    return names


def topological_order(
    crates: list[str], dependencies: dict[str, set[str]]
) -> list[str]:
    """Order crates so every dependency precedes its dependents.

    Independent crates keep their declared order, which keeps the published
    sequence stable across releases.
    """
    rank = {crate: index for index, crate in enumerate(crates)}
    dependents: dict[str, list[str]] = {crate: [] for crate in crates}
    for crate, deps in dependencies.items():
        for dep in deps:
            dependents[dep].append(crate)

    remaining = {crate: len(dependencies[crate]) for crate in crates}
    ready = [rank[crate] for crate in crates if remaining[crate] == 0]
    heapq.heapify(ready)

    order: list[str] = []
    while ready:
        crate = crates[heapq.heappop(ready)]
        order.append(crate)
        for dependent in dependents[crate]:
            remaining[dependent] -= 1
            if remaining[dependent] == 0:
                heapq.heappush(ready, rank[dependent])
    return order


def command_plan(args: argparse.Namespace) -> int:
    packages = workspace_packages(load_metadata(args.metadata))

    errors: list[str] = []
    for crate in args.crates:
        if crate not in packages:
            errors.append(f"'{crate}' is not a workspace member")
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    publish_set = set(args.crates)
    dependencies = {
        crate: package_dependencies(packages[crate], packages) for crate in args.crates
    }
    for crate in args.crates:
        for dep in sorted(dependencies[crate]):
            if dep not in publish_set:
                errors.append(
                    f"'{crate}' depends on workspace member '{dep}' "
                    "which is not in the publish list"
                )
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    order = topological_order(args.crates, dependencies)
    if len(order) != len(args.crates):
        cyclic = [crate for crate in args.crates if crate not in set(order)]
        print(
            f"ERROR: dependency cycle among publish crates: {', '.join(cyclic)}",
            file=sys.stderr,
        )
        return 1

    print("\n".join(order))
    return 0


def command_deps(args: argparse.Namespace) -> int:
    packages = workspace_packages(load_metadata(args.metadata))

    for crate in args.crates:
        if crate not in packages:
            print(f"ERROR: '{crate}' is not a workspace member", file=sys.stderr)
            return 1

    for crate in args.crates:
        for dep in sorted(package_dependencies(packages[crate], packages)):
            print(f"{crate}\t{dep}")
    return 0


def main(argv: list[str] | None = None) -> int:
    # `--metadata` is accepted before or after the subcommand so callers can
    # pass it in whichever position reads naturally. SUPPRESS keeps the
    # subparser from overwriting a value the top-level parser already set.
    common = argparse.ArgumentParser(add_help=False)
    common.add_argument(
        "--metadata",
        default=argparse.SUPPRESS,
        help="path to a cached `cargo metadata --no-deps` JSON document",
    )

    parser = argparse.ArgumentParser(description=__doc__, parents=[common])
    subparsers = parser.add_subparsers(dest="command", required=True)

    plan = subparsers.add_parser(
        "plan", help="print the derived publish order", parents=[common]
    )
    plan.add_argument("--crates", nargs="+", required=True)
    plan.set_defaults(handler=command_plan)

    deps = subparsers.add_parser(
        "deps", help="print workspace dependencies", parents=[common]
    )
    deps.add_argument("--crates", nargs="+", required=True)
    deps.set_defaults(handler=command_deps)

    args = parser.parse_args(argv)
    args.metadata = getattr(args, "metadata", None)
    return args.handler(args)


if __name__ == "__main__":
    sys.exit(main())
