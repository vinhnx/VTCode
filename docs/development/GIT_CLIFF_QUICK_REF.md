# Git-cliff Quick Reference

## Installation

```bash
# Install via Cargo (recommended)
cargo install git-cliff

# Or use Docker (no installation required)
docker run --rm -v "$(pwd):/app" -w /app ghcr.io/orhunp/git-cliff:latest --help
```

## Common Commands

```bash
# Generate changelog for a release (use actual version number)
# Uses GitHub token if available, falls back to git author name
git-cliff --config cliff.toml --tag "0.82.4" --unreleased --output CHANGELOG.md

# Preview unreleased changes for upcoming release
git-cliff --config cliff.toml --tag "0.82.4" --unreleased

# Show last 3 versions
git-cliff --config cliff.toml --latest 3

# Custom range
git-cliff --config cliff.toml v0.80.0..v0.82.0
```

**Note:** 
- **Online mode** (with `GITHUB_TOKEN`): Shows GitHub usernames with @ prefix (e.g., `@vinhnx`, `@contributor`)
- **Offline mode** (without token): Shows full git author name (e.g., `Vinh Nguyen`)
- Use `--tag "<version>"` with `--unreleased` for release changelog

## Release Workflow

```bash
# 1. Dry run (preview changelog)
./scripts/release.sh --patch --dry-run

# 2. Create release (git-cliff used automatically if installed)
./scripts/release.sh --patch

# 3. If git-cliff not installed, falls back to built-in generator
#    Install message shown: "cargo install git-cliff"
```

## Configuration

**File**: `cliff.toml` (project root)

**Key sections**:
- `[changelog]` - Template and formatting
- `[git]` - Commit parsing rules
- `[git.commit_parsers]` - Type-to-section mapping
- `[git.exclude_patterns]` - Commits to skip

## Commit Types

```
feat     → Features
fix      → Bug Fixes
perf     → Performance
refactor → Refactors
security → Security
docs     → Documentation
test     → Tests
build    → Build
ci       → CI
deps     → Dependencies
chore    → (excluded)
```

## Troubleshooting

```bash
# Check installation
git-cliff --version

# Test configuration
git-cliff --config cliff.toml --unreleased

# Validate TOML syntax
git-cliff --config cliff.toml 2>&1 | head -20
```

## Documentation

- **Full Guide**: `docs/development/CHANGELOG_GENERATION.md`
- **Integration Summary**: `GIT_CLIFF_INTEGRATION.md`
- **Official Docs**: https://git-cliff.org/docs/

## Benefits

✅ Rust-native (fits project ecosystem)
✅ No Node.js dependency
✅ Better customization
✅ Faster performance
✅ GitHub Releases integration
✅ Unreleased section support
✅ Active maintenance
