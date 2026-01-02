---
name: skill-installer
description: Install VT Code skills into $VTCODE_HOME/skills from a curated list or a GitHub repo path. Use when a user asks to list installable skills, install a curated skill, or install a skill from another repo (including private repos).
metadata:
    short-description: Install curated skills from openai/skills or other repos
---

# Skill Installer

Helps install skills. By default these are from https://github.com/openai/skills/tree/main/skills/.curated, but users can also provide other locations.

Use the helper scripts based on the task:

-   List curated skills when the user asks what is available, or if the user uses this skill without specifying what to do.
-   Install from the curated list when the user provides a skill name.
-   Install from another repo when the user provides a GitHub repo/path (including private repos).

Install skills with the helper scripts.

## Communication

-   Start by confirming the task and running the appropriate command.
-   If something fails, report the exact error and ask what to do next.
-   Say only what is necessary.

## Scripts

### List Curated Skills

Show available skills from the curated list:

```bash
scripts/list-curated-skills.py
```

Optional: specify a different repo or path:

```bash
scripts/list-curated-skills.py --repo owner/repo --path skills/path --ref branch
```

Output formats:

-   Default: numbered list with installation status
-   JSON: `--format json` for programmatic use

### Install from Curated List

Install a skill by name from the curated list:

```bash
scripts/install-skill-from-github.py <skill-name>
```

The skill will be installed to `$VTCODE_HOME/skills/<skill-name>/`.

### Install from Any GitHub Repo

Install skills from any GitHub repository:

```bash
scripts/install-skill-from-github.py owner/repo path/to/skill
```

For private repos, ensure `gh` CLI is authenticated or `GITHUB_TOKEN` is set.

Install multiple skills from the same repo:

```bash
scripts/install-skill-from-github.py owner/repo path/to/skill1 path/to/skill2
```

## Behavior and Options

### Installation Location

Skills are installed to `$VTCODE_HOME/skills/` by default. The `$VTCODE_HOME` environment variable defaults to `~/.vtcode`.

### Private Repositories

For private repositories:

1. Ensure GitHub CLI (`gh`) is installed and authenticated: `gh auth login`
2. Or set the `GITHUB_TOKEN` environment variable

### Overwriting Existing Skills

If a skill with the same name already exists, the installer will:

1. Warn about the existing skill
2. Ask for confirmation before overwriting
3. Back up the existing skill before replacement

### Validation

Before installing, the script validates:

-   SKILL.md exists and has valid frontmatter
-   Skill name follows naming conventions
-   No invalid or dangerous file patterns

## Notes

-   Skills installed from GitHub are placed in the User scope.
-   After installation, the skill is immediately available in the current session.
-   Use `/skills list` to verify installation.
-   Use `/skills load <skill-name>` to activate the skill.
