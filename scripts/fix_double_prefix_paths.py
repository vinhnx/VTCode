#!/usr/bin/env python3
"""Fix double-prefix crate paths from accidental re-run of update script."""

import os

REPLACEMENTS = [
    ("crates/codegen/", "crates/codegen/"),
    ("crates/common/", "crates/common/"),
]

SCAN_DIRS = [
    "crates",
    "docs",
    "scripts",
    ".github",
    "src",
    "tests",
]

EXTENSIONS = {
    ".rs",
    ".md",
    ".toml",
    ".py",
    ".sh",
    ".json",
}


def should_process(filepath: str) -> bool:
    _, ext = os.path.splitext(filepath)
    return ext in EXTENSIONS


def update_file(filepath: str, dry_run: bool = False) -> int:
    try:
        with open(filepath, "r", encoding="utf-8") as f:
            content = f.read()
    except Exception:
        return 0

    original = content
    for old, new in REPLACEMENTS:
        content = content.replace(old, new)

    if content != original:
        count = sum(original.count(old) for old, _ in REPLACEMENTS)
        if not dry_run:
            with open(filepath, "w", encoding="utf-8") as f:
                f.write(content)
        return count
    return 0


def main():
    total_files = 0
    total_replacements = 0

    for scan_dir in SCAN_DIRS:
        if not os.path.isdir(scan_dir):
            continue
        for root, dirs, files in os.walk(scan_dir):
            if "target" in dirs:
                dirs.remove("target")
            if ".git" in dirs:
                dirs.remove(".git")
            if ".vtcode" in dirs:
                dirs.remove(".vtcode")

            for fname in files:
                filepath = os.path.join(root, fname)
                if should_process(filepath):
                    count = update_file(filepath, dry_run=False)
                    if count > 0:
                        print(f"  {count:4d} replacements -> {filepath}")
                        total_files += 1
                        total_replacements += count

    print(
        f"\nTotal: {total_files} files updated, {total_replacements} replacements made."
    )


if __name__ == "__main__":
    main()
