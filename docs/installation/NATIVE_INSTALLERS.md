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

1. Detects OS (macOS/Linux) and architecture (x86_64/aarch64)
2. Fetches latest version from GitHub API (or uses specified version)
3. Downloads release tarball from GitHub Releases
4. Extracts and installs to `$VTCode_INSTALL_DIR/vtcode`
5. Copies the optional `ghostty-vt/` sidecar when present in the release archive
6. Adds to PATH if needed (updates `.bashrc`, `.zshrc`, or `.profile`)

If the `ghostty-vt/` sidecar is missing or cannot be installed, VT Code still installs successfully and falls back to `legacy_vt100`.

### Supported Platforms

- macOS 10.15+ (Intel & Apple Silicon)
- Linux x86_64, aarch64
- WSL (use install.ps1 for native Windows)

---

## Windows (PowerShell)

```powershell
# Latest version
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

See `install.ps1` for details.

The Windows installer also copies an optional `ghostty-vt\` sidecar directory when present in the release archive. Failure to install the sidecar is non-fatal; VT Code falls back to `legacy_vt100`.
