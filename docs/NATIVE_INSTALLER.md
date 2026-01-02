# Native Installer for VT Code

The native installer provides a fast, dependency-free way to install VT Code on macOS, Linux, and Windows. It downloads pre-built binaries directly from GitHub Releases.

## Features

✨ **Fast & Lightweight**
- Self-contained binary (no Node.js dependency)
- Quick download from GitHub CDN
- Automatic checksum verification

✨ **Cross-Platform**
- macOS (Intel & Apple Silicon)
- Linux (x86_64)
- Windows (PowerShell)

✨ **Auto-Updates**
- Built-in version checking
- Automatic update notifications
- One-command update installation

## Installation

### macOS & Linux

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

Or with custom installation directory:

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s -- --dir ~/.local/bin
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

Or with custom installation directory:

```powershell
$params = @{ InstallDir = "C:\Program Files\vtcode" }
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### Manual Installation

You can also download binaries directly from [GitHub Releases](https://github.com/vinhnx/vtcode/releases):

1. Download the binary for your platform
2. Extract it: `tar -xzf vtcode-vX.X.X-platform.tar.gz` (or `unzip` on Windows)
3. Move to PATH: `mv vtcode /usr/local/bin/`
4. Make executable: `chmod +x /usr/local/bin/vtcode`

## Installation Directory

By default, the installer places VT Code in `~/.local/bin/`. Make sure this directory is in your PATH:

```bash
# Add to ~/.bashrc, ~/.zshrc, or equivalent
export PATH="$HOME/.local/bin:$PATH"
```

If you prefer a system-wide installation:

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | sudo bash -s -- --dir /usr/local/bin

# Note: Windows installation to Program Files may require administrator privileges
```

## Updating VT Code

The native binary includes auto-update checking. When a new version is available, you'll see a notification. To update manually:

```bash
vtcode update
```

Or reinstall using the installer script:

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

## Verifying Installation

Check that VT Code is properly installed:

```bash
vtcode --version
vtcode --help
```

Test basic functionality:

```bash
vtcode ask "What is Rust?"
```

## Uninstalling

To remove VT Code:

```bash
rm ~/.local/bin/vtcode
# Or if installed elsewhere:
rm /path/to/vtcode
```

Clean up configuration files (optional):

```bash
# macOS/Linux
rm -rf ~/.config/vtcode ~/.cache/vtcode

# Windows
rmdir "%APPDATA%\vtcode" /s /q
```

## Troubleshooting

### "Command not found: vtcode"

The installation directory is not in your PATH. Add it:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Then add this line to your shell configuration file (`~/.bashrc`, `~/.zshrc`, etc.) to make it permanent.

### "Permission denied" or "Failed to install"

You may not have write permissions to the target directory. Try:

```bash
# Install to user directory (no sudo needed)
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Or use a writable directory
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s -- --dir ~/bin
```

### "Checksum mismatch" error

This usually indicates a corrupted download. The installer will automatically retry. If it persists:

1. Check your internet connection
2. Try downloading again after a few minutes
3. Report the issue on GitHub if it continues

### Windows: "Cannot be loaded because running scripts is disabled"

This is a Windows PowerShell execution policy issue. Try:

```powershell
# Temporarily allow script execution
powershell -ExecutionPolicy Bypass -Command "irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex"
```

## Platform Support

| Platform | Support | Architecture |
|----------|---------|--------------|
| macOS 10.15+ | ✅ | Intel & Apple Silicon |
| Ubuntu 20.04+ | ✅ | x86_64 |
| Debian 10+ | ✅ | x86_64 |
| Alpine Linux | ❌ | Use from source or Docker |
| Windows 10+ | ✅ | x86_64 (PowerShell/WSL) |
| Windows 11 | ✅ | x86_64 (PowerShell/WSL) |

## How It Works

1. **Detection**: Identifies your operating system and architecture
2. **Fetch**: Gets the latest release info from GitHub API
3. **Download**: Downloads the binary for your platform from GitHub CDN
4. **Verify**: Checks SHA256 checksum to ensure integrity
5. **Extract**: Unpacks the binary from the archive
6. **Install**: Copies binary to your installation directory
7. **Verify**: Tests that the binary works correctly

## Security

- **Checksum Verification**: All binaries are verified against SHA256 checksums published by GitHub
- **HTTPS Downloads**: All downloads use secure HTTPS connections
- **No Root Required**: Installation doesn't require sudo (unless installing to system directories)
- **Binary Integrity**: Binaries are code-signed (macOS) and signed (Windows)

## Alternative Installation Methods

- **Homebrew**: `brew install vinhnx/tap/vtcode`
- **Cargo**: `cargo install vtcode`
- **Docker**: Available via container image
- **From Source**: Build from repository

## Development

### Testing the Installer

```bash
# Test bash installer
bash scripts/install.sh --help

# Test PowerShell installer (Windows)
powershell -ExecutionPolicy Bypass -Command ".\scripts\install.ps1 -Help"

# Test with dry-run in CI
INSTALL_DIR=/tmp/test ./scripts/install.sh
```

### Building Release Binaries

Binaries are built automatically via GitHub Actions on release:

```bash
./scripts/release.sh --patch
```

This triggers the `build-release.yml` workflow which:
- Builds binaries for all platforms
- Generates SHA256 checksums
- Uploads to GitHub Releases
- Publishes release notes

## See Also

- [Installation Guide](../README.md#installation)
- [GitHub Releases](https://github.com/vinhnx/vtcode/releases)
- [Contributing Guide](CONTRIBUTING.md)
