# Installer Development Guide

Information for VT Code maintainers and contributors.

## Overview

VT Code provides native installers for three platforms:
- **Shell script** (macOS, Linux) - `scripts/install.sh`
- **PowerShell script** (Windows) - `scripts/install.ps1`
- **Homebrew formula** - `homebrew/vtcode.rb`

## Platform Detection

### Shell Script
- Uses `uname -s` for OS (Darwin, Linux, etc.)
- Uses `uname -m` for arch (x86_64, arm64, aarch64, armv7l)
- Maps to release binary: `ARCH-PLATFORM` (e.g., `x86_64-apple-darwin`)

### PowerShell Script
- Uses `[Environment]::Is64BitProcess` for 64-bit detection
- Uses WMI `Get-WmiObject Win32_Processor` for ARM64 detection
- Maps to: `x86_64-pc-windows-msvc`

## Release Binaries

Required for installation. Must be generated for all platforms:

```
vtcode-v{VERSION}-{PLATFORM}.tar.gz  (macOS, Linux, etc.)
vtcode-v{VERSION}-{PLATFORM}.zip     (Windows)
```

### Supported Platforms
- `aarch64-apple-darwin` (macOS ARM64/M1/M2)
- `x86_64-apple-darwin` (macOS Intel)
- `aarch64-unknown-linux-gnu` (Linux ARM64)
- `x86_64-unknown-linux-gnu` (Linux x86_64)
- `armv7-unknown-linux-gnueabihf` (Linux ARMv7)
- `x86_64-pc-windows-msvc` (Windows)

## GitHub Releases Setup

Installers expect binaries at:
```
https://github.com/vinhnx/vtcode/releases/download/v{VERSION}/{BINARY}
```

### GitHub Actions Workflow

Example CI/CD for cross-platform builds:

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

jobs:
  build:
    strategy:
      matrix:
        include:
          - os: macos-latest
            target: x86_64-apple-darwin
          - os: macos-latest
            target: aarch64-apple-darwin
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
          - os: windows-latest
            target: x86_64-pc-windows-msvc
    
    runs-on: ${{ matrix.os }}
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Build
        run: cargo build --release --target ${{ matrix.target }}
      
      - name: Package
        run: |
          if [ "$RUNNER_OS" = "Windows" ]; then
            7z a vtcode-v${{ github.ref_name }}-${{ matrix.target }}.zip \
              target/${{ matrix.target }}/release/vtcode.exe
          else
            tar -czf vtcode-v${{ github.ref_name }}-${{ matrix.target }}.tar.gz \
              -C target/${{ matrix.target }}/release vtcode
          fi
      
      - name: Upload
        uses: softprops/action-gh-release@v1
        with:
          files: vtcode-v*
```

## Testing Installers

### Shell Script

#### On macOS
```bash
bash scripts/install.sh
# Verify
vtcode --version
```

#### On Linux (local)
```bash
bash scripts/install.sh
vtcode --version
```

#### In Docker
```bash
# Test on Linux
docker run -it --rm ubuntu:latest bash -c \
  'apt-get update && apt-get install -y curl && \
   bash <(curl -fsSL file:///path/to/install.sh)'
```

### PowerShell Script

#### On Windows 10/11
```powershell
.\scripts\install.ps1
vtcode --version
```

#### Via PowerShell Core (cross-platform)
```powershell
pwsh -File scripts/install.ps1
vtcode --version
```

### Homebrew Formula

```bash
# Test locally
brew install --verbose --formula ./homebrew/vtcode.rb
vtcode --version

# Uninstall
brew uninstall vtcode
```

## Updating for New Releases

### 1. Create GitHub Release

1. Create release on GitHub with tag `v{VERSION}`
2. Upload binary artifacts for all platforms
3. Note the SHA256 hashes

### 2. Update Homebrew Formula

Edit `homebrew/vtcode.rb`:

```ruby
class Vtcode < Formula
  version "X.Y.Z"
  
  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "HASH_HERE"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "HASH_HERE"
    end
  end
  
  on_linux do
    if Hardware::CPU.arm? && Hardware::CPU.is_64_bit?
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "HASH_HERE"
    else
      url "https://github.com/vinhnx/vtcode/releases/download/v#{version}/vtcode-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "HASH_HERE"
    end
  end
end
```

Get SHA256:
```bash
shasum -a 256 vtcode-v*.tar.gz
shasum -a 256 vtcode-v*.zip
```

### 3. Update Homebrew Tap (if applicable)

If using a personal tap (github.com/vinhnx/homebrew-vtcode):

```bash
cd ~/homebrew-vtcode
cp ../vtcode/homebrew/vtcode.rb Formula/
git add Formula/vtcode.rb
git commit -m "Update VT Code to v{VERSION}"
git push
```

### 4. Verify Installation

Test on each platform:

```bash
# macOS/Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
vtcode --version

# Windows
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
vtcode --version

# Homebrew
brew install vtcode
vtcode --version
```

## Installer Scripts Structure

### Shell Script (`scripts/install.sh`)

Key functions:
- `detect_platform()` - Determine OS and architecture
- `check_existing()` - Check if already installed
- `get_latest_version()` - Query GitHub API
- `download_binary()` - Download and extract
- `determine_install_path()` - Find best installation location
- `install_binary()` - Copy binary and make executable
- `verify_installation()` - Run vtcode --version
- `print_next_steps()` - Show post-install guidance

Error handling: `set -e` (exit on error)

### PowerShell Script (`scripts/install.ps1`)

Key functions:
- `Get-PlatformInfo` - Detect architecture
- `Test-ExistingInstallation` - Check if already installed
- `Get-LatestVersion` - Query GitHub API
- `Get-Binary` - Download binary
- `Expand-Binary` - Extract binary
- `Get-InstallDirectory` - Find best location
- `Install-Binary` - Copy and verify
- `Add-ToPATH` - Configure PATH
- `Test-Installation` - Verify installation
- `Cleanup-TempFiles` - Remove temp files

Error handling: `$ErrorActionPreference = "Stop"`

## Troubleshooting Installer Issues

### Download failures

Check if GitHub API is accessible:
```bash
curl https://api.github.com/repos/vinhnx/vtcode/releases/latest
```

### Platform detection issues

Test detection logic:
```bash
# Shell
uname -s
uname -m

# PowerShell
[Environment]::Is64BitProcess
Get-WmiObject Win32_Processor | Select Architecture
```

### Path configuration

Verify PATH after installation:
```bash
# Shell
echo $PATH

# PowerShell
$env:PATH
```

## Documentation

User-facing docs:
- `docs/installation/README.md` - Main installation guide
- `docs/installation/QUICK_REFERENCE.md` - Quick commands
- `docs/installation/NATIVE_INSTALLERS.md` - Technical details

This file:
- `docs/installation/DEVELOPERS.md` - Maintainer guide

## Common Issues

### Binary not found in release

Ensure binaries are uploaded to GitHub Releases for all platforms. Check:
```bash
curl https://api.github.com/repos/vinhnx/vtcode/releases/latest | jq '.assets'
```

### SHA256 mismatch

Regenerate SHA256:
```bash
shasum -a 256 vtcode-v*.tar.gz
```

Update formula with new hashes.

### Installer script permissions

Make sure installers are executable:
```bash
chmod +x scripts/install.sh scripts/install.ps1
```

### PowerShell execution policy

Users might need to set execution policy:
```powershell
Set-ExecutionPolicy -ExecutionPolicy Bypass -Scope Process
```

The script handles this in comments.

## Future Improvements

1. **Auto-generated SHA256** - Update formula automatically on release
2. **Signature verification** - Sign binaries and verify in installers
3. **Rollback support** - Install/downgrade specific versions
4. **Delta updates** - Only download changed binary segments
5. **Cached downloads** - Distribute via CDN or mirrors
