#!/usr/bin/env python3
"""
Skill Packager - Package a skill directory into a distributable .skill file

Usage:
    package_skill.py <skill-path> [--output <path>] [--validate-only]

Examples:
    package_skill.py skills/public/my-skill
    package_skill.py skills/public/my-skill --output releases/
    package_skill.py skills/private/my-api --validate-only
"""

import argparse
import hashlib
import json
import re
import sys
import zipfile
from datetime import datetime, timezone
from pathlib import Path

# File patterns to exclude from packages
EXCLUDE_PATTERNS = [
    "__pycache__",
    ".pyc",
    ".pyo",
    ".git",
    ".gitignore",
    ".DS_Store",
    "Thumbs.db",
    ".env",
    ".venv",
    "node_modules",
    "*.log",
    "*.tmp",
    "*.bak",
    "*.swp",
    ".idea",
    ".vscode",
]

# Required files/directories
REQUIRED_FILES = ["SKILL.md"]


def validate_skill_md(skill_md_path):
    """
    Validate SKILL.md format and content.

    Returns:
        tuple: (is_valid, metadata_dict, error_messages)
    """
    errors = []
    warnings = []
    metadata = {}

    if not skill_md_path.exists():
        return False, {}, ["SKILL.md not found"]

    content = skill_md_path.read_text(encoding="utf-8")

    # Check for YAML frontmatter
    frontmatter_match = re.match(r"^---\s*\n(.*?)\n---\s*\n", content, re.DOTALL)
    if not frontmatter_match:
        errors.append("SKILL.md must start with YAML frontmatter (---)")
        return False, {}, errors

    frontmatter = frontmatter_match.group(1)

    # Parse frontmatter (simple key: value parsing)
    for line in frontmatter.strip().split("\n"):
        if ":" in line:
            key, value = line.split(":", 1)
            key = key.strip()
            value = value.strip()
            # Remove quotes if present
            if value.startswith('"') and value.endswith('"'):
                value = value[1:-1]
            elif value.startswith("'") and value.endswith("'"):
                value = value[1:-1]
            metadata[key] = value

    # Validate required fields
    if "name" not in metadata:
        errors.append("SKILL.md frontmatter must include 'name' field")
    elif not metadata["name"]:
        errors.append("'name' field cannot be empty")
    elif len(metadata["name"]) > 64:
        errors.append(f"'name' field too long ({len(metadata['name'])} chars, max 64)")

    if "description" not in metadata:
        errors.append("SKILL.md frontmatter must include 'description' field")
    elif not metadata["description"]:
        errors.append("'description' field cannot be empty")
    elif "[TODO" in metadata["description"]:
        errors.append("'description' still contains TODO placeholder - please complete it")
    elif len(metadata["description"]) > 1024:
        errors.append(
            f"'description' field too long ({len(metadata['description'])} chars, max 1024)"
        )

    # Check body content
    body = content[frontmatter_match.end() :]
    if not body.strip():
        errors.append("SKILL.md body is empty")
    elif "[TODO" in body:
        warnings.append("SKILL.md body contains TODO placeholders - consider completing them")

    # Check for minimum content
    if len(body.strip()) < 50:
        warnings.append("SKILL.md body is very short - consider adding more detail")

    # Print warnings
    for warning in warnings:
        print(f"[WARN] {warning}")

    is_valid = len(errors) == 0
    return is_valid, metadata, errors


def should_exclude(path, base_path):
    """Check if a path should be excluded from packaging."""
    rel_path = str(path.relative_to(base_path))

    for pattern in EXCLUDE_PATTERNS:
        if pattern.startswith("*"):
            # Wildcard at start
            if rel_path.endswith(pattern[1:]):
                return True
        elif "*" in pattern:
            # Wildcard in middle - simple glob
            parts = pattern.split("*")
            if len(parts) == 2 and rel_path.startswith(parts[0]) and rel_path.endswith(parts[1]):
                return True
        else:
            # Exact match or component match
            if pattern in rel_path.split("/") or rel_path.endswith(pattern):
                return True

    return False


def collect_files(skill_dir):
    """
    Collect all files to include in the package.

    Returns:
        list of (file_path, archive_name) tuples
    """
    files = []
    skill_dir = skill_dir.resolve()

    for path in skill_dir.rglob("*"):
        if path.is_file() and not should_exclude(path, skill_dir):
            archive_name = str(path.relative_to(skill_dir))
            files.append((path, archive_name))

    return sorted(files, key=lambda x: x[1])


def compute_checksum(files):
    """Compute SHA256 checksum of all files."""
    hasher = hashlib.sha256()

    for file_path, archive_name in files:
        # Include filename in hash
        hasher.update(archive_name.encode("utf-8"))
        # Include file content
        with open(file_path, "rb") as f:
            while chunk := f.read(8192):
                hasher.update(chunk)

    return hasher.hexdigest()


def create_manifest(skill_name, files, checksum):
    """Create package manifest."""
    return {
        "format_version": "1.0",
        "skill_name": skill_name,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "file_count": len(files),
        "checksum": checksum,
        "files": [archive_name for _, archive_name in files],
    }


def package_skill(skill_dir, output_dir=None, validate_only=False):
    """
    Package a skill directory into a .skill file.

    Args:
        skill_dir: Path to skill directory
        output_dir: Output directory for .skill file (default: skill_dir parent)
        validate_only: Only validate, don't create package

    Returns:
        Path to created .skill file, or None if error/validate-only
    """
    skill_dir = Path(skill_dir).resolve()

    if not skill_dir.exists():
        print(f"[ERROR] Skill directory not found: {skill_dir}")
        return None

    if not skill_dir.is_dir():
        print(f"[ERROR] Path is not a directory: {skill_dir}")
        return None

    skill_name = skill_dir.name
    print(f"Packaging skill: {skill_name}")
    print(f"   Source: {skill_dir}")

    # Check required files
    for required in REQUIRED_FILES:
        required_path = skill_dir / required
        if not required_path.exists():
            print(f"[ERROR] Required file missing: {required}")
            return None

    # Validate SKILL.md
    skill_md_path = skill_dir / "SKILL.md"
    is_valid, metadata, errors = validate_skill_md(skill_md_path)

    if not is_valid:
        print("\n[ERROR] SKILL.md validation failed:")
        for error in errors:
            print(f"   - {error}")
        return None

    print("[OK] SKILL.md validated")
    print(f"   Name: {metadata.get('name', 'N/A')}")
    desc = metadata.get("description", "N/A")
    if len(desc) > 60:
        desc = desc[:57] + "..."
    print(f"   Description: {desc}")

    # Collect files
    files = collect_files(skill_dir)
    print(f"[OK] Found {len(files)} files to package")

    # Compute checksum
    checksum = compute_checksum(files)
    print(f"[OK] Checksum: {checksum[:16]}...")

    if validate_only:
        print("\n[OK] Validation passed - skill is ready to package")
        print("   Files:")
        for _, archive_name in files[:10]:
            print(f"      {archive_name}")
        if len(files) > 10:
            print(f"      ... and {len(files) - 10} more")
        return None

    # Create manifest
    manifest = create_manifest(skill_name, files, checksum)

    # Determine output path
    if output_dir:
        output_dir = Path(output_dir).resolve()
        output_dir.mkdir(parents=True, exist_ok=True)
    else:
        output_dir = skill_dir.parent

    output_path = output_dir / f"{skill_name}.skill"

    # Create zip package
    try:
        with zipfile.ZipFile(output_path, "w", zipfile.ZIP_DEFLATED) as zf:
            # Add manifest first
            manifest_json = json.dumps(manifest, indent=2)
            zf.writestr("MANIFEST.json", manifest_json)

            # Add all files
            for file_path, archive_name in files:
                zf.write(file_path, archive_name)

        print(f"\n[OK] Package created: {output_path}")
        print(f"   Size: {output_path.stat().st_size:,} bytes")
        print(f"   Files: {len(files)}")

        return output_path

    except Exception as e:
        print(f"[ERROR] Failed to create package: {e}")
        return None


def main():
    parser = argparse.ArgumentParser(
        description="Package a skill directory into a distributable .skill file.",
    )
    parser.add_argument("skill_path", help="Path to the skill directory")
    parser.add_argument("--output", "-o", help="Output directory for .skill file")
    parser.add_argument(
        "--validate-only",
        "-v",
        action="store_true",
        help="Only validate the skill, don't create package",
    )
    args = parser.parse_args()

    result = package_skill(
        args.skill_path,
        output_dir=args.output,
        validate_only=args.validate_only,
    )

    if args.validate_only:
        # For validate-only, success means validation passed
        sys.exit(0)
    elif result:
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()
