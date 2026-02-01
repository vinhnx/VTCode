# VT Code Release Automation

Automated release workflow with GitHub CLI integration.

## Quick Start

```bash
# Patch release (0.74.2 → 0.74.3)
./scripts/release.sh --patch

# Minor release
./scripts/release.sh --minor

# Major release
./scripts/release.sh --major

# Specific version
./scripts/release.sh 0.75.0

# Dry run (no actual changes)
./scripts/release.sh --patch --dry-run
```

## What It Automates

### Step 1: Build Binaries
- Builds `x86_64-apple-darwin` and `aarch64-apple-darwin` binaries
- Creates `.tar.gz` archives and SHA256 checksums
- Skippable with `--skip-binaries`

### Step 2: Generate Changelog
- Creates changelog from git commits
- Updates `CHANGELOG.md`
- Commits changelog with bot credentials

### Step 3: Cargo Release
- Bumps version in `Cargo.toml`
- Publishes to `crates.io`
- Creates git tag (`v0.74.3`)
- Pushes to remote
- Skippable with `--skip-crates`

### Step 3.5: Create GitHub Release
- Creates GitHub release for the tag
- Includes changelog as release notes
- Automatically detects if release exists

### Step 4: Upload Binaries
- Packages macOS binaries (x86_64, aarch64)
- Creates SHA256 checksums
- Uploads to GitHub Release with `gh release upload`

### Step 5: Update Homebrew
- Updates Homebrew formula with new checksums
- Commits and pushes to `vinhnx/tap/vtcode`

### Step 6: docs.rs Rebuild
- Triggers documentation build on docs.rs
- Skippable with `--skip-docs`

## Prerequisites

```bash
# GitHub CLI (required for release automation)
brew install gh

# Authenticate with GitHub
gh auth login

# Install cargo-release
cargo install cargo-release

# Optional: Node.js for advanced changelog features
brew install node
```

## GitHub Authentication

### Initial Setup
```bash
gh auth login
# Select: GitHub.com → HTTPS → Y (authenticate via web) → Y (authorize git)
```

### Verify Authentication
```bash
gh auth status
```

### Refresh Scopes (if needed)
```bash
gh auth refresh -h github.com -s workflow
```

## Common Workflows

### Local Build + Release
```bash
# Full release with all steps
./scripts/release.sh --patch
```

### Dry Run (Safe Testing)
```bash
# Test without making changes
./scripts/release.sh --patch --dry-run
```

### Crates.io Only (No Binaries)
```bash
# Skip binary building and upload
./scripts/release.sh --patch --skip-binaries
```

### Binaries Only (No Crates)
```bash
# Skip crates.io publishing
./scripts/release.sh --patch --skip-crates
```

## Troubleshooting

### "GitHub CLI is not authenticated"
```bash
gh auth login
```

### "cargo-release is not installed"
```bash
cargo install cargo-release
```

### Binaries Fail to Build
- Ensure `rustup target add x86_64-apple-darwin aarch64-apple-darwin`
- Check disk space for build artifacts

### GitHub Release Upload Fails
- Verify GitHub CLI auth: `gh auth status`
- Check internet connectivity
- Ensure release was created: `gh release view v0.74.3`

### Homebrew Formula Update Fails
- Requires push access to `vinhnx/homebrew-tap`
- Verify SSH/HTTPS credentials

## Manual Steps (If Automation Fails)

```bash
# Create GitHub release manually
gh release create v0.74.3 \
  --title "VT Code v0.74.3" \
  --notes "$(cat CHANGELOG.md)"

# Upload binaries manually
gh release upload v0.74.3 \
  target/x86_64-apple-darwin/release/vtcode \
  target/aarch64-apple-darwin/release/vtcode
```

## Environment Variables

```bash
# Use custom GitHub token
export GITHUB_TOKEN="ghp_..."

# Dry run to preview all steps
export DRY_RUN=true
```

## Notes

- Release script checks for clean git working tree
- Requires being on `main` branch
- Changelog is auto-generated from commits since last tag
- Release notes are automatically populated from changelog
- Binary uploads are clobbered if release artifacts already exist

## For CI/CD

To run releases in GitHub Actions:

```bash
gh workflow run release.yml -f tag=v0.74.3
```

This triggers `.github/workflows/release.yml` which builds for all platforms (macOS, Linux, Windows).
