#!/usr/bin/env bash
# Shared formula transformation only; callers own commits and publication.

update_homebrew_formula_file() {
	python3 - "$@" <<'PYTHON_SCRIPT'
import re
import sys
from pathlib import Path

if len(sys.argv) != 7:
    sys.exit("Expected formula path, version, and all four Homebrew checksums")

formula_path = Path(sys.argv[1])
version = sys.argv[2]
if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?", version):
    sys.exit("Invalid Homebrew version")

targets = (
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-gnu",
)
checksums = dict(zip(targets, sys.argv[3:]))
for target, checksum in checksums.items():
    if not re.fullmatch(r"[0-9a-fA-F]{64}", checksum):
        sys.exit(f"Missing or invalid Homebrew checksum for {target}")

content = formula_path.read_text()
content, count = re.subn(
    r'(?m)^(\s*version\s+")[^"]*(")',
    lambda match: f'{match.group(1)}{version}{match.group(2)}',
    content,
)
if count != 1:
    sys.exit("Expected exactly one Homebrew version declaration")
for target, checksum in checksums.items():
    content, count = re.subn(
        rf'({re.escape(target)}\.tar\.gz"\s+sha256\s+")[^"]*(")',
        lambda match: f'{match.group(1)}{checksum}{match.group(2)}',
        content,
    )
    if count != 1:
        sys.exit(f"Expected exactly one Homebrew checksum for {target}")

# Validation completes before any bytes are changed.
formula_path.write_text(content)
PYTHON_SCRIPT
}
