#!/usr/bin/env python3
"""Verify the dev-profile framework overrides still match the pinned Kit graph.

The GPUI Kit installation guide (https://gpui-kit.com/docs/installation/, CC BY
4.0) recommends optimizing the rendering, layout, and text crates at opt-level 3
in Debug builds. VT Code already optimizes every dependency at opt-level 1, so
these overrides lift only that framework stack; application code stays
debuggable and proc macros remain a compiler-only concern.

A Kit upgrade that renames or drops one of these crates turns its override into
a silent no-op, so this gate checks both the manifest and Cargo.lock.

Usage:
    python3 scripts/check_dev_profile.py            # check this checkout
    python3 scripts/check_dev_profile.py --root DIR
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST_REL = Path("Cargo.toml")
LOCK_REL = Path("Cargo.lock")
# Adapted from the guide for the pinned 0.10.0 graph: gpui-base and the macOS
# platform backends are still separate crates there, while gpui-fast is in the
# lockfile but not enabled.
FRAMEWORK_PACKAGES = (
    "gpui-base",
    "gpui-component",
    "gpui-kit",
    "gpui-kit-assets",
    "gpui-pre",
    "gpui-pre-apple",
    "gpui-pre-macos",
    "gpui-pre-platform",
    "rustybuzz",
    "taffy",
    "ttf-parser",
)
EXPECTED_OPT_LEVEL = 3


def check(root: Path) -> list[str]:
    """Return human-readable findings; an empty list means the profile is in sync."""
    try:
        manifest = tomllib.loads((root / MANIFEST_REL).read_text(encoding="utf-8"))
        overrides = manifest["profile"]["dev"]["package"]
        lock = tomllib.loads((root / LOCK_REL).read_text(encoding="utf-8"))
        locked_names = {package["name"] for package in lock["package"]}
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as exc:
        return [f"cannot read Cargo.toml or Cargo.lock profile inputs: {exc}"]

    if not isinstance(overrides, dict):
        return [
            f"Cargo.toml: profile.dev.package is {type(overrides).__name__}, expected a table"
        ]

    findings: list[str] = []
    for name in FRAMEWORK_PACKAGES:
        entry = overrides.get(name)
        if not isinstance(entry, dict) or entry.get("opt-level") != EXPECTED_OPT_LEVEL:
            findings.append(
                f"Cargo.toml: profile.dev.package.{name} must set "
                f"opt-level = {EXPECTED_OPT_LEVEL} (found {entry!r})"
            )
        if name not in locked_names:
            findings.append(
                f"Cargo.lock: no package named {name}; the dev-profile "
                "override is a no-op after a dependency change"
            )
    return findings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    findings = check(args.root)
    if findings:
        for finding in findings:
            print(f"ERROR: {finding}", file=sys.stderr)
        return 1
    print("Dev profile optimizes the pinned GPUI framework crates.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
