#!/usr/bin/env python3
"""Verify the manual third-party notice header against the vendored GPUI snapshot.

GPUI Kit's FAQ (https://gpui-kit.com/docs/faq, CC BY 4.0) explains that the
`gpui-pre-*` packages are versioned snapshots of Zed's GPUI, published with
Zed's license and notices intact. The manual header in THIRD-PARTY-NOTICES
therefore has to name the exact snapshot VT Code vendored. It is written by
hand, while the vendored crate already carries that identity in its metadata,
so the two can drift silently (they did: 0.3.11 was recorded for a 0.3.12
snapshot). This gate compares them and also checks that the generated notices
still begin with the current header.

Usage:
    python3 scripts/check_third_party_notices.py          # check this checkout
    python3 scripts/check_third_party_notices.py --root DIR
"""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST_REL = Path("patches/gpui-pre-macos/Cargo.toml")
HEADER_REL = Path("scripts/templates/third-party-header.txt")
NOTICES_REL = Path("THIRD-PARTY-NOTICES")
CRATE = "gpui-pre-macos"
SNAPSHOT_LINE = re.compile(
    r"^[ \t]*Snapshot:[ \t]*(\S+)[ \t]+(\S+),[ \t]*zed@([0-9a-f]{7,40})[ \t]*$",
    re.MULTILINE,
)


def vendored_snapshot(root: Path) -> tuple[str, str]:
    """Return (version, zed revision) from the vendored crate, one source of truth."""
    data = tomllib.loads((root / MANIFEST_REL).read_text(encoding="utf-8"))
    package = data["package"]
    return package["version"], package["metadata"]["gpui-pre"]["zed-rev"]


def check(root: Path) -> list[str]:
    """Return human-readable findings; an empty list means the notices agree."""
    try:
        version, revision = vendored_snapshot(root)
        header = (root / HEADER_REL).read_text(encoding="utf-8")
    except (OSError, KeyError, TypeError, tomllib.TOMLDecodeError) as exc:
        return [f"cannot read vendored snapshot metadata or notice header: {exc}"]

    findings: list[str] = []
    matches = [m for m in SNAPSHOT_LINE.findall(header) if m[0] == CRATE]
    if len(matches) != 1:
        findings.append(
            f"{HEADER_REL}: expected exactly one '{CRATE}' Snapshot line, "
            f"found {len(matches)}"
        )
    else:
        _, header_version, header_revision = matches[0]
        if header_version != version:
            findings.append(
                f"{HEADER_REL}: snapshot version {header_version} "
                f"but vendored crate is {version}"
            )
        if header_revision != revision:
            findings.append(
                f"{HEADER_REL}: zed revision {header_revision} "
                f"but vendored crate is {revision}"
            )

    try:
        notices = (root / NOTICES_REL).read_text(encoding="utf-8")
    except OSError as exc:
        findings.append(f"cannot read {NOTICES_REL}: {exc}")
    else:
        if not notices.startswith(header):
            findings.append(
                f"{NOTICES_REL} no longer starts with {HEADER_REL}; "
                "regenerate with scripts/generate-notices.sh"
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
        print(
            "Update scripts/templates/third-party-header.txt to match the "
            "vendored crate, then run scripts/generate-notices.sh.",
            file=sys.stderr,
        )
        return 1
    print("Third-party GPUI snapshot attribution matches the vendored crate.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
