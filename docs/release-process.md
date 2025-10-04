# Release Process Documentation

This document explains how to create releases for the VTCode project, including changelog generation.

## Release Process

1. Ensure all changes are committed to the `main` branch
2. Run the release script:
   ```bash
   ./scripts/release.sh --minor|--major|--patch
   ```
3. The script will:
   - Bump versions in all workspace crates
   - Update dependencies between workspace crates
   - Create git commits and tags
   - Push changes to the remote repository
   - Publish to crates.io (unless --skip-crates is used)

## Changelog Generation

Changelog generation happens automatically when tags are pushed:

1. The release script creates a git tag (e.g., `v0.20.0`)
2. GitHub Actions workflow (`.github/workflows/release.yml`) is triggered
3. The workflow runs `changelogithub` which:
   - Analyzes git commits since the last tag
   - Groups commits by type (feat, fix, etc.) based on conventional commits
   - Updates `CHANGELOG.md` with new entries
   - Creates a GitHub Release with the changelog content

## Pre-release Versions

For pre-release versions, use:
```bash
./scripts/release.sh --pre-release
# or
./scripts/release.sh --pre-release-suffix beta.1
```

## Configuration

- Changelog generation is configured in `.github/changelogithub.config.js`
- Release process is configured in `release.toml`
- The release script is in `scripts/release.sh`

## Conventional Commits

For changelog generation to work properly, commit messages should follow conventional commit format:
- `feat: description` - New features (become "Features" in changelog)
- `fix: description` - Bug fixes (become "Bug Fixes" in changelog)
- `docs: description` - Documentation changes
- etc.