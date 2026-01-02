#!/usr/bin/env python3
"""
Install Skill from GitHub - Download and install skills from GitHub repositories

Usage:
    install-skill-from-github.py <owner/repo> [--skill <name>] [--branch <branch>] [--path <path>]

Examples:
    install-skill-from-github.py vtcode-ai/skills --skill pdf-converter
    install-skill-from-github.py myuser/my-skills --skill custom-tool --branch main
    install-skill-from-github.py org/private-skills --skill internal-tool --path skills/private

For private repositories, set GITHUB_TOKEN environment variable.
"""

import argparse
import json
import os
import shutil
import sys
import tempfile
import zipfile
from io import BytesIO
from pathlib import Path
from urllib.request import urlopen, Request
from urllib.error import URLError, HTTPError

# Default installation paths
DEFAULT_SKILLS_PATH = Path.home() / ".vtcode" / "skills"
SKILL_FILENAME = "SKILL.md"


def get_github_token():
    """Get GitHub token from environment."""
    return os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")


def make_github_request(url, token=None):
    """
    Make a request to GitHub API or raw content.

    Returns:
        bytes: Response content
    """
    headers = {
        "User-Agent": "vtcode-skill-installer/1.0",
        "Accept": "application/vnd.github.v3+json",
    }

    if token:
        headers["Authorization"] = f"token {token}"

    req = Request(url, headers=headers)

    try:
        with urlopen(req, timeout=30) as response:
            return response.read()
    except HTTPError as e:
        if e.code == 404:
            raise ValueError(f"Not found: {url}")
        elif e.code == 401:
            raise ValueError("Authentication required. Set GITHUB_TOKEN environment variable.")
        elif e.code == 403:
            raise ValueError("Access denied. Check your GITHUB_TOKEN permissions.")
        else:
            raise ValueError(f"GitHub API error {e.code}: {e.reason}")
    except URLError as e:
        raise ValueError(f"Network error: {e.reason}")


def list_repo_skills(owner, repo, branch="main", token=None):
    """
    List available skills in a repository.

    Returns:
        list of dict: Skills found in repository
    """
    # Try to get repository contents
    api_url = f"https://api.github.com/repos/{owner}/{repo}/contents?ref={branch}"

    try:
        data = make_github_request(api_url, token)
        contents = json.loads(data)
    except ValueError as e:
        print(f"[WARN] Could not list repository contents: {e}")
        return []

    skills = []

    for item in contents:
        if item.get("type") == "dir":
            # Check if directory contains SKILL.md
            dir_name = item["name"]
            skill_url = f"https://api.github.com/repos/{owner}/{repo}/contents/{dir_name}/{SKILL_FILENAME}?ref={branch}"

            try:
                make_github_request(skill_url, token)
                skills.append({
                    "name": dir_name,
                    "path": dir_name,
                })
            except ValueError:
                # Not a skill directory
                pass

    # Also check root for SKILL.md
    root_skill_url = f"https://api.github.com/repos/{owner}/{repo}/contents/{SKILL_FILENAME}?ref={branch}"
    try:
        make_github_request(root_skill_url, token)
        skills.insert(0, {
            "name": repo,
            "path": ".",
        })
    except ValueError:
        pass

    return skills


def download_skill(owner, repo, skill_path, branch="main", token=None):
    """
    Download a skill from GitHub.

    Returns:
        Path: Temporary directory containing the skill
    """
    # Download repository as zip
    zip_url = f"https://github.com/{owner}/{repo}/archive/refs/heads/{branch}.zip"

    print(f"[INFO] Downloading from {owner}/{repo}...")

    try:
        data = make_github_request(zip_url, token)
    except ValueError as e:
        raise ValueError(f"Failed to download repository: {e}")

    # Extract to temp directory
    temp_dir = tempfile.mkdtemp(prefix="vtcode-skill-")

    try:
        with zipfile.ZipFile(BytesIO(data)) as zf:
            zf.extractall(temp_dir)
    except zipfile.BadZipFile:
        shutil.rmtree(temp_dir)
        raise ValueError("Downloaded file is not a valid zip archive")

    # Find extracted directory (usually repo-branch)
    extracted_dirs = [d for d in Path(temp_dir).iterdir() if d.is_dir()]
    if not extracted_dirs:
        shutil.rmtree(temp_dir)
        raise ValueError("No directories found in downloaded archive")

    extracted_dir = extracted_dirs[0]

    # Find skill within extracted directory
    if skill_path == ".":
        skill_dir = extracted_dir
    else:
        skill_dir = extracted_dir / skill_path

    if not skill_dir.exists():
        shutil.rmtree(temp_dir)
        raise ValueError(f"Skill path not found: {skill_path}")

    if not (skill_dir / SKILL_FILENAME).exists():
        shutil.rmtree(temp_dir)
        raise ValueError(f"No {SKILL_FILENAME} found in {skill_path}")

    return temp_dir, skill_dir


def validate_skill(skill_dir):
    """
    Basic validation of downloaded skill.

    Returns:
        tuple: (is_valid, skill_name, errors)
    """
    skill_md = skill_dir / SKILL_FILENAME
    if not skill_md.exists():
        return False, None, [f"{SKILL_FILENAME} not found"]

    content = skill_md.read_text(encoding="utf-8")

    # Parse frontmatter
    import re

    frontmatter_match = re.match(r"^---\s*\n(.*?)\n---\s*\n", content, re.DOTALL)
    if not frontmatter_match:
        return False, None, ["Missing YAML frontmatter"]

    frontmatter = frontmatter_match.group(1)
    name = None

    for line in frontmatter.split("\n"):
        if line.startswith("name:"):
            name = line.split(":", 1)[1].strip().strip("\"'")
            break

    if not name:
        return False, None, ["Missing 'name' field in frontmatter"]

    return True, name, []


def install_skill(skill_dir, install_path, skill_name, force=False):
    """
    Install skill to target directory.

    Returns:
        Path: Installation path
    """
    target_dir = install_path / skill_name

    if target_dir.exists():
        if force:
            print(f"[WARN] Overwriting existing skill: {skill_name}")
            shutil.rmtree(target_dir)
        else:
            raise ValueError(
                f"Skill already exists: {target_dir}\n"
                "Use --force to overwrite."
            )

    # Copy skill files
    shutil.copytree(skill_dir, target_dir)

    return target_dir


def main():
    parser = argparse.ArgumentParser(
        description="Install skills from GitHub repositories.",
    )
    parser.add_argument(
        "repo",
        help="GitHub repository in owner/repo format",
    )
    parser.add_argument(
        "--skill",
        "-s",
        help="Skill name/path within repository (default: list available)",
    )
    parser.add_argument(
        "--branch",
        "-b",
        default="main",
        help="Git branch (default: main)",
    )
    parser.add_argument(
        "--path",
        "-p",
        help=f"Installation path (default: {DEFAULT_SKILLS_PATH})",
    )
    parser.add_argument(
        "--force",
        "-f",
        action="store_true",
        help="Overwrite existing skill",
    )
    parser.add_argument(
        "--list",
        "-l",
        action="store_true",
        help="List skills in repository without installing",
    )
    args = parser.parse_args()

    # Parse repository
    if "/" not in args.repo:
        print("[ERROR] Repository must be in owner/repo format")
        sys.exit(1)

    owner, repo = args.repo.split("/", 1)
    token = get_github_token()

    if token:
        print("[INFO] Using GitHub token for authentication")

    # List mode
    if args.list or not args.skill:
        print(f"[INFO] Listing skills in {owner}/{repo}...")
        skills = list_repo_skills(owner, repo, args.branch, token)

        if not skills:
            print("[INFO] No skills found in repository")
            print("   Make sure the repository contains directories with SKILL.md files")
            sys.exit(0)

        print(f"\nFound {len(skills)} skill(s):\n")
        for skill in skills:
            print(f"  {skill['name']}")

        print("\nTo install:")
        print(f"  install-skill-from-github.py {args.repo} --skill <name>")
        sys.exit(0)

    # Install mode
    skill_path = args.skill
    install_path = Path(args.path) if args.path else DEFAULT_SKILLS_PATH

    # Ensure install path exists
    install_path.mkdir(parents=True, exist_ok=True)

    temp_dir = None
    try:
        # Download skill
        temp_dir, skill_dir = download_skill(owner, repo, skill_path, args.branch, token)

        # Validate
        is_valid, skill_name, errors = validate_skill(skill_dir)
        if not is_valid:
            print(f"[ERROR] Invalid skill: {', '.join(errors)}")
            sys.exit(1)

        print(f"[OK] Validated skill: {skill_name}")

        # Install
        installed_path = install_skill(skill_dir, install_path, skill_name, args.force)
        print(f"[OK] Installed to: {installed_path}")

        print("\nSkill installed successfully!")
        print(f"   Name: {skill_name}")
        print(f"   Path: {installed_path}")

    except ValueError as e:
        print(f"[ERROR] {e}")
        sys.exit(1)

    finally:
        # Cleanup temp directory
        if temp_dir and Path(temp_dir).exists():
            shutil.rmtree(temp_dir)


if __name__ == "__main__":
    main()
