# VT Code Distribution Strategy

This document describes how VT Code distributes binaries and manages updates using free resources.

## Overview

VT Code uses a **GitHub-native distribution model** that leverages free infrastructure:

- **Storage**: GitHub Releases (free, unlimited)
- **CDN**: GitHub's CDN (fast.github.com, free)
- **Checksums**: SHA256 published in each release
- **API**: GitHub REST API (generous free tier)
- **Signatures**: Code signing (macOS notarization, Windows signing via GitHub Actions)

## Release Workflow

```
Local Release Command
        ↓
cargo-release (version bump + git tags)
        ↓
GitHub Actions: build-release.yml
        ↓
Cross-compile 4 platforms
        ↓
Upload binaries to GitHub Releases
        ↓
GitHub Actions: native-installer.yml
        ↓
Generate & publish checksums.txt
        ↓
Users download via installer script
```

## Platforms & Targets

VT Code is distributed for these platforms:

| Platform | Target Triple | Format | Installer |
|----------|---|---|---|
| macOS Intel | `x86_64-apple-darwin` | `.tar.gz` | `install.sh` |
| macOS ARM64 | `aarch64-apple-darwin` | `.tar.gz` | `install.sh` |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `.tar.gz` | `install.sh` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `.zip` | `install.ps1` |

See `.github/workflows/build-release.yml` for build configuration.

## Installation Methods

### 1. Native Installer (Recommended)

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Windows
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

**Advantages**:
- No dependencies (no Node.js, no cargo)
- Fast (direct binary download)
- Auto-update capable
- Works offline after installation

**How it works**:
1. Fetches latest release from GitHub API
2. Downloads binary for detected platform
3. Verifies SHA256 checksum
4. Installs to `~/.local/bin/` (or custom directory)

### 2. Homebrew

```bash
brew install vinhnx/tap/vtcode
```

Managed via `homebrew/vtcode.rb`. Formula is auto-updated during releases with correct SHA256 checksums.

### 3. Cargo (from source)

```bash
cargo install vtcode
```

Requires Rust toolchain. Slower but allows customization.

### 4. Manual (Direct Download)

Download binaries directly from [GitHub Releases](https://github.com/vinhnx/vtcode/releases).

## Auto-Update Mechanism

VT Code includes built-in update checking (when available):

```bash
vtcode update  # Check for and install updates
```

**How it works**:
1. Checks GitHub API for latest release (non-blocking, rate-limited)
2. Compares remote version with `env!("CARGO_PKG_VERSION")`
3. Notifies user if update available
4. Downloads and installs automatically (optional confirmation)
5. Respects 24-hour check interval to avoid API hammering

See `src/updater.rs` for implementation.

## Checksum Verification

All distributed binaries are verified against SHA256 checksums:

1. **Generation**: GitHub Actions computes checksums after building
2. **Publication**: `checksums.txt` uploaded to GitHub Release assets
3. **Verification**: Installer downloads and verifies before installation
4. **Format**: Standard `sha256sum` format for compatibility

Example `checksums.txt`:
```
abc123...  vtcode-v0.58.4-x86_64-apple-darwin.tar.gz
def456...  vtcode-v0.58.4-aarch64-apple-darwin.tar.gz
ghi789...  vtcode-v0.58.4-x86_64-unknown-linux-gnu.tar.gz
jkl012...  vtcode-v0.58.4-x86_64-pc-windows-msvc.zip
```

## Release Process

### 1. Create Release

```bash
./scripts/release.sh --patch
```

This:
- Bumps version in Cargo.toml
- Creates git tag (v0.58.5)
- Pushes to GitHub
- Triggers GitHub Actions workflows

### 2. Build Binaries (Automatic via GitHub Actions)

The `build-release.yml` workflow:
- Runs on `release: [published]` event
- Builds for 4 platforms in parallel
- Uploads binaries as release assets
- Takes ~10-15 minutes

### 3. Generate Checksums (Automatic via GitHub Actions)

The `native-installer.yml` workflow:
- Runs after release is published
- Downloads all binaries
- Generates SHA256 checksums
- Uploads `checksums.txt` to release assets

### 4. Update Homebrew Formula

The release script automatically:
- Computes SHA256 of macOS binaries
- Updates `homebrew/vtcode.rb`
- Commits and pushes formula changes

## API Rate Limits

Free GitHub API tier provides:

- **60 requests/hour** (unauthenticated)
- **5000 requests/hour** (authenticated)

The installer uses:
1. One API call to fetch latest release
2. One HTTP request to download binary
3. Optional: One API call for checksums

**Total per installation: ~2 API calls** - well within limits.

For update checking, we implement:
- 24-hour check interval caching
- Silently skip on connection errors
- No retries (fail fast)

## Storage Usage

GitHub Releases provides:
- **Unlimited file storage**
- **Unlimited downloads**
- **CDN acceleration** (GitHub's fast.github.com)

Current usage:
- ~20MB per release × 4 platforms = ~80MB per version
- Very sustainable for many years of releases

## Cost Analysis

**Free Resources Used**:
- ✅ GitHub Releases (binary hosting)
- ✅ GitHub Actions (CI/CD, code signing)
- ✅ GitHub CDN (file downloads)
- ✅ GitHub API (metadata fetching)
- ✅ Raw GitHub content (install script delivery)

**Zero Cost**:
- No S3 or external storage
- No dedicated distribution server
- No payment for downloads
- No third-party services

## Security Considerations

### Binary Integrity

- **SHA256 verification**: All installers verify checksums before installation
- **Code signing**: macOS notarization, Windows signature
- **HTTPS only**: All downloads over encrypted connections

### Update Safety

- **Version validation**: Semantic versioning with `semver` crate
- **Checksum verification**: Before applying any update
- **No auto-restart**: Updates require user confirmation
- **No background execution**: All operations visible to user

## Future Improvements

Potential enhancements (still using free resources):

1. **Self-hosted update checks**: Cache version info locally to reduce API calls
2. **Differential updates**: Download only changed files (requires new infrastructure)
3. **Signed checksums**: GPG sign `checksums.txt` for verification
4. **Release notes**: Embed in binary or fetch on demand
5. **Telemetry**: Optional, privacy-respecting usage tracking

## Testing

### Test Installer Locally

```bash
# macOS/Linux
bash scripts/install.sh --help
INSTALL_DIR=/tmp/test-vtcode ./scripts/install.sh

# Windows PowerShell
powershell -ExecutionPolicy Bypass -Command ".\scripts\install.ps1 -Help"
```

### Test Release Workflow

```bash
# Dry-run release
./scripts/release.sh --patch --dry-run

# Verify checksums
sha256sum -c checksums.txt
```

### Test Update Checking

```bash
# Trigger update check (sets version to check against)
vtcode --check-updates
```

## Implementation Files

- **Installer Scripts**:
  - `scripts/install.sh` - macOS/Linux installer
  - `scripts/install.ps1` - Windows installer

- **Updater Code**:
  - `src/updater.rs` - Version checking and update logic

- **GitHub Actions**:
  - `.github/workflows/build-release.yml` - Cross-platform binary builds
  - `.github/workflows/native-installer.yml` - Checksum generation

- **Documentation**:
  - `docs/NATIVE_INSTALLER.md` - User installation guide
  - `docs/DISTRIBUTION_STRATEGY.md` - This file

## See Also

- [Release Guide](docs/RELEASE_GUIDE.md)
- [Installation Guide](README.md#installation)
- [GitHub Actions Workflows](.github/workflows/)
