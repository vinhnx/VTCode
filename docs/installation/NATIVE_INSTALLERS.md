# Native Installers

## macOS & Linux (Shell)

### Install

```bash
# Latest version
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Specific version
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s v0.85.0
```

### Options

- **Version**: Pass as argument (`v0.85.0` or `latest`)
- **Install dir**: `VTCode_INSTALL_DIR=/usr/local/bin bash install.sh`

### How It Works

1. Detects OS (macOS/Linux) and architecture
2. Fetches latest version from GitHub API (or uses specified version)
3. Downloads release tarball from GitHub Releases
4. Extracts and installs to `$VTCode_INSTALL_DIR/vtcode`
5. Copies the bundled `ghostty-vt/` runtime library directory from the release archive
6. Adds to PATH if needed (updates `.bashrc`, `.zshrc`, or `.profile`)

If the bundled `ghostty-vt/` runtime library directory is missing or cannot be installed, VT Code still installs successfully and falls back to `legacy_vt100`.

This differs from the optional search tools flow:

- `ripgrep` and `ast-grep` can be installed after the fact with `vtcode dependencies install search-tools`
- Ghostty VT is not managed through `vtcode dependencies install ...`
- Official macOS/Linux release archives are expected to include `ghostty-vt/` next to the VT Code binary
- Fresh configs default to Ghostty and still fall back to `legacy_vt100` if the runtime library is absent.

### Supported Platforms

- macOS 10.15+ (Intel & Apple Silicon)
- Linux x86_64
- WSL (use install.ps1 for native Windows)

Linux `aarch64` hosts are detected by the installer, but native `aarch64` release artifacts are not published by default in current release workflow.

---

## Windows (PowerShell)

```powershell
# Latest version
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

See `install.ps1` for details.

Windows release builds are currently not guaranteed on every release because the Windows pipeline still depends on `cross`, Docker, and GitHub Actions minutes, which are costlier than the Linux path.

The Windows installer currently ships only the VT Code binary. Ghostty VT remains unsupported there, so Windows continues to use `legacy_vt100`.

As on macOS/Linux, Ghostty VT is a packaged runtime-library add-on rather than a VT Code-managed dependency command.
