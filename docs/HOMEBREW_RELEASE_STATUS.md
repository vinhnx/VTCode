# Homebrew Release Status

## Status: FULLY OPERATIONAL ✅

The Homebrew distribution for VT Code is fully functional and automated. Users can install and update VT Code using `brew install vtcode`.

## Current Setup

### Official Formula Location
- **Repository**: Homebrew/core
- **Formula**: `Formula/v/vtcode.rb`
- **URL**: https://github.com/Homebrew/homebrew-core/blob/HEAD/Formula/v/vtcode.rb

### Installation Method
```bash
brew install vtcode
brew upgrade vtcode  # Update to latest
```

### Current Version
- **Latest Release**: 0.58.3
- **Installation Status**: Bottled (pre-built binaries available)
- **Build Method**: From crates.io source

## Automation

### Release Workflow (`.github/workflows/release-on-tag.yml`)

When a git tag matching `v*` is pushed:

1. **create-release job**: Generates GitHub Release with changelog
2. **build-binaries job**: Builds and uploads binaries for all platforms:
   - macOS (x86_64 and ARM64)
   - Linux (x86_64 and ARM64)
   - Windows (x86_64)
3. **update-homebrew job**: Updates the local formula with checksums

### Formula Update Process

The automated workflow:
1. Downloads all released binaries
2. Calculates SHA256 checksums
3. Updates `homebrew/vtcode.rb` with version and checksums
4. Commits and pushes to main branch
5. Homebrew/core will pick up the changes automatically

## Formula Structure

The local formula (`homebrew/vtcode.rb`) includes:

```ruby
class Vtcode < Formula
  version "0.58.3"
  
  on_macos do
    # macOS ARM64 (Apple Silicon)
    # macOS x86_64 (Intel)
  end
  
  on_linux do
    # Linux ARM64
    # Linux x86_64
  end
end
```

### Checksums

Platform-specific SHA256 checksums are automatically updated on each release:
- `aarch64-apple-darwin.tar.gz` - macOS ARM64
- `x86_64-apple-darwin.tar.gz` - macOS x86_64
- `aarch64-unknown-linux-gnu.tar.gz` - Linux ARM64
- `x86_64-unknown-linux-gnu.tar.gz` - Linux x86_64

## How to Release

```bash
# Create and push a release tag
./scripts/release.sh --patch      # For patch versions
./scripts/release.sh --minor      # For minor versions
./scripts/release.sh --major      # For major versions
```

The workflow automatically:
1. Builds all platform binaries
2. Uploads to GitHub Releases
3. Updates the Homebrew formula
4. Commits and pushes formula changes to main

## Verification

To verify the installation works:
```bash
brew info vtcode
vtcode --version
```

## Known Fixes Applied

1. **Python String Interpolation**: Fixed shell variable expansion in Python heredoc
2. **Git Tag Synchronization**: Missing git tags (v0.56.0 through v0.58.0) were created
3. **Workflow YAML**: Corrected indentation and shell variable handling

## Troubleshooting

### Formula Not Updating
1. Check GitHub Actions workflow runs
2. Verify git tag was created: `git tag -l | grep v0.xx.x`
3. Manually trigger release: `git tag v0.xx.x && git push origin v0.xx.x`

### Installation Issues
```bash
# Clear cache and reinstall
brew uninstall vtcode
brew update
brew install vtcode
```

### Verify Checksums
```bash
# Check current formula in Homebrew
brew formula vtcode
```

## Next Steps

The release process is fully automated. For future releases:
1. Run `./scripts/release.sh` with appropriate flag
2. Verify GitHub Actions workflow completes
3. Confirm formula updates in git history
4. Users can install via `brew install vtcode`

---
**Last Updated**: 2026-01-02
**Status**: Production Ready ✅
