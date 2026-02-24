# Homebrew Release Status

## Status: FULLY OPERATIONAL ✅

VT Code is available in Homebrew/core. Users can install and update via `brew install vtcode`.

## Current Setup

### Official Formula Location
- **Repository**: Homebrew/core
- **Formula**: `Formula/v/vtcode.rb`
- **URL**: https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/v/vtcode.rb

### Installation
```bash
brew install vtcode
brew upgrade vtcode  # Update to latest
```

### Current Status
- **Latest Release**: 0.58.3
- **Installation Method**: Bottled (pre-built binaries available)
- **Build Source**: From crates.io source
- **Install Analytics**: 343 installations (30 days), 816 (90 days)

## Release Automation

When `./scripts/release.sh` is run, the following happens automatically:

1. **release.yml workflow** (triggered by git tag push):
   - Generates changelog using git-cliff
   - Creates GitHub Release

2. **build-release.yml workflow** (triggered by release published):
   - Builds binaries for all platforms (macOS x86_64/ARM64, Linux x86_64/ARM64, Windows)
   - Uploads to GitHub Release
   - Homebrew/core automatically picks up the new release

No Homebrew formula updates needed - the official formula in Homebrew/core builds from crates.io, which is automatically updated by the release script.

## How to Release

```bash
./scripts/release.sh --patch      # For patch versions
./scripts/release.sh --minor      # For minor versions
./scripts/release.sh --major      # For major versions
```

The release script uses cargo-release to:
1. Bump version in Cargo.toml
2. Update CHANGELOG.md
3. Create and push git tag (e.g., v0.59.0)

GitHub Actions automatically:
1. Generates release notes
2. Builds all platform binaries
3. Uploads to GitHub Releases
4. Homebrew/core detects the new release and updates the formula

## Verification

```bash
brew info vtcode
vtcode --version
```

---
**Last Updated**: 2026-01-02
**Status**: Production Ready ✅
