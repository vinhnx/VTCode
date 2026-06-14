#!/usr/bin/env python3
"""
List Curated Skills - List available skills from curated repositories

Usage:
    list-curated-skills.py [--source <url>] [--category <category>] [--json]

Examples:
    list-curated-skills.py
    list-curated-skills.py --category productivity
    list-curated-skills.py --json
"""

import argparse
import json
import sys
from urllib.request import urlopen, Request
from urllib.error import URLError, HTTPError

# Default curated skills catalog URL
DEFAULT_CATALOG_URL = "https://raw.githubusercontent.com/vtcode-ai/skills-catalog/main/catalog.json"

# Fallback embedded catalog for offline use
EMBEDDED_CATALOG = {
    "version": "1.0",
    "skills": [
        {
            "name": "skill-creator",
            "description": "Create new skills from scratch with proper structure",
            "category": "development",
            "source": "builtin",
            "repo": None,
        },
        {
            "name": "skill-installer",
            "description": "Install skills from GitHub repositories",
            "category": "development",
            "source": "builtin",
            "repo": None,
        },
    ],
}


def fetch_catalog(catalog_url):
    """
    Fetch skills catalog from URL.

    Returns:
        dict: Catalog data or None if fetch failed
    """
    try:
        req = Request(catalog_url, headers={"User-Agent": "vtcode-skill-installer/1.0"})
        with urlopen(req, timeout=10) as response:
            return json.loads(response.read().decode("utf-8"))
    except (URLError, HTTPError, json.JSONDecodeError) as e:
        print(f"[WARN] Could not fetch catalog: {e}", file=sys.stderr)
        return None


def filter_skills(skills, category=None, search=None):
    """Filter skills by category and/or search term."""
    result = skills

    if category:
        category_lower = category.lower()
        result = [s for s in result if s.get("category", "").lower() == category_lower]

    if search:
        search_lower = search.lower()
        result = [
            s
            for s in result
            if search_lower in s.get("name", "").lower()
            or search_lower in s.get("description", "").lower()
        ]

    return result


def format_skill_table(skills):
    """Format skills as a readable table."""
    if not skills:
        return "No skills found."

    # Calculate column widths
    name_width = max(len(s.get("name", "")) for s in skills)
    name_width = max(name_width, 4)  # Minimum width for "Name"

    cat_width = max(len(s.get("category", "")) for s in skills)
    cat_width = max(cat_width, 8)  # Minimum width for "Category"

    lines = []

    # Header
    header = f"{'Name':<{name_width}}  {'Category':<{cat_width}}  Description"
    lines.append(header)
    lines.append("-" * len(header))

    # Skills
    for skill in skills:
        name = skill.get("name", "")
        category = skill.get("category", "")
        description = skill.get("description", "")

        # Truncate description if too long
        max_desc = 60
        if len(description) > max_desc:
            description = description[: max_desc - 3] + "..."

        lines.append(f"{name:<{name_width}}  {category:<{cat_width}}  {description}")

    return "\n".join(lines)


def get_categories(skills):
    """Get unique categories from skills list."""
    categories = set()
    for skill in skills:
        if cat := skill.get("category"):
            categories.add(cat)
    return sorted(categories)


def main():
    parser = argparse.ArgumentParser(
        description="List available skills from curated repositories.",
    )
    parser.add_argument(
        "--source",
        help=f"Catalog URL (default: {DEFAULT_CATALOG_URL})",
        default=DEFAULT_CATALOG_URL,
    )
    parser.add_argument(
        "--category",
        "-c",
        help="Filter by category",
    )
    parser.add_argument(
        "--search",
        "-s",
        help="Search skills by name or description",
    )
    parser.add_argument(
        "--categories",
        action="store_true",
        help="List available categories",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Output as JSON",
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="Use embedded catalog (offline mode)",
    )
    args = parser.parse_args()

    # Fetch catalog
    if args.offline:
        catalog = EMBEDDED_CATALOG
        print("[INFO] Using embedded catalog (offline mode)", file=sys.stderr)
    else:
        catalog = fetch_catalog(args.source)
        if catalog is None:
            print("[INFO] Falling back to embedded catalog", file=sys.stderr)
            catalog = EMBEDDED_CATALOG

    skills = catalog.get("skills", [])

    # List categories mode
    if args.categories:
        categories = get_categories(skills)
        if args.json:
            print(json.dumps(categories, indent=2))
        else:
            print("Available categories:")
            for cat in categories:
                count = len([s for s in skills if s.get("category") == cat])
                print(f"  {cat} ({count} skills)")
        return

    # Filter skills
    filtered = filter_skills(skills, category=args.category, search=args.search)

    # Output
    if args.json:
        print(json.dumps(filtered, indent=2))
    else:
        print(f"Found {len(filtered)} skill(s)")
        if args.category:
            print(f"Category: {args.category}")
        if args.search:
            print(f"Search: {args.search}")
        print()
        print(format_skill_table(filtered))

        # Show install hint
        if filtered:
            print()
            print("To install a skill:")
            print("  scripts/install-skill-from-github.py <owner>/<repo> [--skill <name>]")


if __name__ == "__main__":
    main()
