#!/usr/bin/env python3
"""
Quick Validate - Fast validation of SKILL.md files

Usage:
    quick_validate.py <skill-path> [--strict]

Examples:
    quick_validate.py skills/public/my-skill
    quick_validate.py skills/public/my-skill --strict
    quick_validate.py .  # Validate current directory
"""

import argparse
import re
import sys
from pathlib import Path

# Validation limits
MAX_NAME_LEN = 64
MAX_DESCRIPTION_LEN = 1024
MIN_BODY_LEN = 50


def parse_frontmatter(content):
    """
    Parse YAML frontmatter from SKILL.md content.

    Returns:
        tuple: (metadata_dict, body_content, error_message)
    """
    frontmatter_match = re.match(r"^---\s*\n(.*?)\n---\s*\n", content, re.DOTALL)
    if not frontmatter_match:
        return None, content, "Missing YAML frontmatter (must start with ---)"

    frontmatter_text = frontmatter_match.group(1)
    body = content[frontmatter_match.end() :]

    metadata = {}
    current_key = None
    current_value_lines = []

    for line in frontmatter_text.split("\n"):
        # Check for new key
        key_match = re.match(r"^(\w+[\w-]*):\s*(.*)", line)
        if key_match:
            # Save previous key if any
            if current_key:
                value = "\n".join(current_value_lines).strip()
                # Remove surrounding quotes
                if (value.startswith('"') and value.endswith('"')) or (
                    value.startswith("'") and value.endswith("'")
                ):
                    value = value[1:-1]
                metadata[current_key] = value

            current_key = key_match.group(1)
            current_value_lines = [key_match.group(2)]
        elif current_key and line.strip():
            # Continuation of multi-line value
            current_value_lines.append(line.strip())

    # Save last key
    if current_key:
        value = "\n".join(current_value_lines).strip()
        if (value.startswith('"') and value.endswith('"')) or (
            value.startswith("'") and value.endswith("'")
        ):
            value = value[1:-1]
        metadata[current_key] = value

    return metadata, body, None


def validate_skill(skill_path, strict=False):
    """
    Validate a SKILL.md file.

    Args:
        skill_path: Path to skill directory or SKILL.md file
        strict: Enable strict validation (warnings become errors)

    Returns:
        tuple: (is_valid, errors, warnings)
    """
    skill_path = Path(skill_path).resolve()

    # Find SKILL.md
    if skill_path.is_file() and skill_path.name == "SKILL.md":
        skill_md_path = skill_path
        skill_dir = skill_path.parent
    elif skill_path.is_dir():
        skill_md_path = skill_path / "SKILL.md"
        skill_dir = skill_path
    else:
        return False, [f"Invalid path: {skill_path}"], []

    if not skill_md_path.exists():
        return False, [f"SKILL.md not found in {skill_dir}"], []

    errors = []
    warnings = []

    try:
        content = skill_md_path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return False, ["SKILL.md is not valid UTF-8 text"], []

    # Parse frontmatter
    metadata, body, parse_error = parse_frontmatter(content)
    if parse_error:
        return False, [parse_error], []

    # Validate 'name' field
    if "name" not in metadata:
        errors.append("Missing required field: 'name'")
    elif not metadata["name"]:
        errors.append("Field 'name' is empty")
    elif len(metadata["name"]) > MAX_NAME_LEN:
        errors.append(f"Field 'name' too long: {len(metadata['name'])} chars (max {MAX_NAME_LEN})")
    elif not re.match(r"^[a-z0-9][a-z0-9-]*[a-z0-9]$|^[a-z0-9]$", metadata["name"]):
        warnings.append(
            f"Field 'name' should be lowercase hyphen-case (got: '{metadata['name']}')"
        )

    # Validate 'description' field
    if "description" not in metadata:
        errors.append("Missing required field: 'description'")
    elif not metadata["description"]:
        errors.append("Field 'description' is empty")
    elif "[TODO" in metadata["description"]:
        errors.append("Field 'description' contains TODO placeholder")
    elif len(metadata["description"]) > MAX_DESCRIPTION_LEN:
        errors.append(
            f"Field 'description' too long: {len(metadata['description'])} chars "
            f"(max {MAX_DESCRIPTION_LEN})"
        )
    elif len(metadata["description"]) < 20:
        warnings.append("Field 'description' is very short - consider adding more detail")

    # Validate body content
    body_stripped = body.strip()
    if not body_stripped:
        errors.append("SKILL.md body is empty")
    elif len(body_stripped) < MIN_BODY_LEN:
        warnings.append(f"SKILL.md body is very short ({len(body_stripped)} chars)")

    # Check for TODO placeholders in body
    todo_count = body.count("[TODO")
    if todo_count > 0:
        warnings.append(f"Body contains {todo_count} TODO placeholder(s)")

    # Check for markdown structure
    if not re.search(r"^##\s+", body, re.MULTILINE):
        warnings.append("Body has no ## headings - consider adding structure")

    # Check resources directories if they exist
    for resource_type in ["scripts", "references", "assets"]:
        resource_dir = skill_dir / resource_type
        if resource_dir.exists() and resource_dir.is_dir():
            files = list(resource_dir.iterdir())
            if not files:
                warnings.append(f"Empty {resource_type}/ directory - remove if unused")
            elif all(f.name.startswith(".") for f in files):
                warnings.append(f"{resource_type}/ contains only hidden files")

    # In strict mode, warnings become errors
    if strict:
        errors.extend(warnings)
        warnings = []

    is_valid = len(errors) == 0
    return is_valid, errors, warnings


def print_results(skill_path, is_valid, errors, warnings, verbose=True):
    """Print validation results."""
    skill_name = Path(skill_path).resolve().name

    if is_valid and not warnings:
        print(f"[OK] {skill_name}: Valid")
        return

    if is_valid:
        print(f"[OK] {skill_name}: Valid (with warnings)")
    else:
        print(f"[FAIL] {skill_name}: Invalid")

    if verbose:
        for error in errors:
            print(f"   [ERROR] {error}")
        for warning in warnings:
            print(f"   [WARN] {warning}")


def main():
    parser = argparse.ArgumentParser(
        description="Quick validation of SKILL.md files.",
    )
    parser.add_argument(
        "skill_path",
        nargs="?",
        default=".",
        help="Path to skill directory or SKILL.md file (default: current directory)",
    )
    parser.add_argument(
        "--strict",
        "-s",
        action="store_true",
        help="Strict mode: treat warnings as errors",
    )
    parser.add_argument(
        "--quiet",
        "-q",
        action="store_true",
        help="Quiet mode: only show pass/fail",
    )
    args = parser.parse_args()

    is_valid, errors, warnings = validate_skill(args.skill_path, strict=args.strict)
    print_results(args.skill_path, is_valid, errors, warnings, verbose=not args.quiet)

    if is_valid:
        sys.exit(0)
    else:
        sys.exit(1)


if __name__ == "__main__":
    main()
