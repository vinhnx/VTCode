# Release Process Documentation

This document explains how to create releases for the VT Code project, including changelog generation.

## Release Process

1. Ensure all changes are committed to the `main` branch
2. Run the release script:
    ```bash
    ./scripts/release.sh --minor|--major|--patch
    ```
3. The script will:
    - Bump versions in all workspace crates
    - Update dependencies between workspace crates
    - Generate changelog using git-cliff
    - Create git commits and tags
    - Push changes to the remote repository
    - Publish to crates.io (unless --skip-crates is used)

## Changelog Generation

Changelog generation is handled by [git-cliff](https://git-cliff.org):

1. The release script calls git-cliff with the project configuration
2. git-cliff analyzes git commits since the last tag
3. Commits are grouped by type (feat, fix, etc.) based on conventional commits
4. CHANGELOG.md is updated with new entries
5. Release notes are generated for GitHub Releases

For detailed documentation, see [Changelog Generation Guide](./development/CHANGELOG_GENERATION.md).

## Pre-release Versions

For pre-release versions, use:

```bash
./scripts/release.sh --pre-release
# or
./scripts/release.sh --pre-release-suffix beta.1
```

## Configuration

-   Changelog generation is configured in `cliff.toml`
-   Release process is configured in `release.toml`
-   The release script is in `scripts/release.sh`

## Conventional Commits

For changelog generation to work properly, commit messages should follow conventional commit format:

-   `feat: description` - New features (become "Features" in changelog)
-   `fix: description` - Bug fixes (become "Bug Fixes" in changelog)
-   `docs: description` - Documentation changes
-   etc.
