#!/usr/bin/env python3
"""Remove redundant version keys from workspace-internal vtcode-* dependencies."""

import os
import re

# Pattern to match vtcode-* dependencies with version keys
# Matches: vtcode-foo = { workspace = true,  version = "..." }
# Or: vtcode-foo = { version = "...", workspace = true }
VERSION_PATTERN = re.compile(
    r'(vtcode-[a-zA-Z0-9_-]+)\s*=\s*\{([^}]*?)\s*version\s*=\s*"[^"]*"\s*,?\s*([^}]*?)\}'
)

# Simpler approach: remove `version = "..."` from vtcode-* dep lines
# Matches the version line inside a vtcode-* dependency block
DEP_PATTERN = re.compile(
    r"((?:vtcode-[a-zA-Z0-9_-]+)\s*=\s*\{)([^}]+)(\})", re.MULTILINE | re.DOTALL
)


def fix_cargo_toml(filepath: str) -> int:
    """Remove version keys from vtcode-* workspace deps. Returns replacements count."""
    try:
        with open(filepath, "r", encoding="utf-8") as f:
            content = f.read()
    except Exception:
        return 0

    original = content

    def remove_version(match: re.Match) -> str:
        dep_line = match.group(1)
        body = match.group(2)
        closing = match.group(3)

        # Only process vtcode-* deps
        if not dep_line.strip().startswith("vtcode-"):
            return match.group(0)

        # Remove version = "..." entries (with optional trailing comma)
        cleaned = re.sub(r'\s*version\s*=\s*"[^"]*"', "", body)
        # Clean up double commas/spaces that might result
        cleaned = re.sub(r",\s*,", ",", cleaned)
        cleaned = re.sub(r",\s*\}", "}", cleaned)
        cleaned = re.sub(r"\{\s*,", "{", cleaned)

        return f"{dep_line}{cleaned}{closing}"

    content = DEP_PATTERN.sub(remove_version, content)

    if content != original:
        count = len(DEP_PATTERN.findall(original))
        with open(filepath, "w", encoding="utf-8") as f:
            f.write(content)
        return count
    return 0


def main():
    total_files = 0
    total_replacements = 0

    for root, dirs, files in os.walk("crates"):
        if "target" in dirs:
            dirs.remove("target")

        for fname in files:
            if fname != "Cargo.toml":
                continue
            filepath = os.path.join(root, fname)
            count = fix_cargo_toml(filepath)
            if count > 0:
                print(f"  {count:4d} deps cleaned -> {filepath}")
                total_files += 1
                total_replacements += count

    print(
        f"\nTotal: {total_files} files updated, {total_replacements} dependencies cleaned."
    )


if __name__ == "__main__":
    main()
