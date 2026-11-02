# Native Installers

## macOS & Linux (Shell)

### Install

```bash
# Latest version
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# With installer options
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s -- --with-completions
```

### Options

- **Flags**: pass installer flags after `--` (see `./install.sh --help` for all flags)
- **Version**: the installer always resolves the latest compatible release; pinned-version installs are not supported
- **Install dir**: `INSTALL_DIR=/usr/local/bin bash install.sh`
- **Desktop (macOS 15+ only)**: the installer asks before installing it when run interactively (default: install).
  It installs from the prebuilt `vtcode-desktop-<version>-<target>.zip` release asset when one exists for the host, so
  no build toolchain is needed; only releases without a desktop asset fall back to a from-source build (best-effort: a
  missing asset or toolchain only warns). `--with-desktop` installs without prompting and fails if unavailable;
  `--without-desktop` skips without prompting. The from-source fallback needs Xcode command line tools, Rust, and git,
  and takes several minutes. Once the CLI is installed, `vtcode desktop --install` installs or updates the app anytime.
- **Desktop location**: fresh installs ask where to put the app — `~/Applications` (default, no sudo) or
  `/Applications` (all users, elevates with sudo when not user-writable). Non-interactive runs use `~/Applications`;
  setting `DESKTOP_INSTALL_DIR` beforehand selects the location without prompting.

### How It Works

1. Detects OS (macOS/Linux) and architecture
2. Fetches the latest compatible release from the GitHub API
3. Downloads release tarball from GitHub Releases
4. Extracts and installs to `$INSTALL_DIR/vtcode`
5. Prints the `export PATH` line to add when the install dir is not on PATH
6. Asks about the desktop app on macOS 15+ when interactive (best-effort install from the prebuilt release asset),
   unless `--with-desktop`/`--without-desktop` was passed

### Supported Platforms

CLI:

- macOS 10.15+ (Intel & Apple Silicon)
- Linux x86_64
- WSL (use install.ps1 for native Windows)

Desktop app (macOS only, offered by default): macOS 15+ on Apple Silicon or Intel. It installs from a prebuilt
release asset (no toolchain needed); only releases without a desktop asset fall back to a from-source build needing
Xcode command line tools, Rust 1.99.1+, and git (`--without-desktop` skips it). See
[desktop system requirements](../development/desktop-workspace.md#system-requirements).

Linux `aarch64` hosts are detected by the installer, but native `aarch64` release artifacts are not published by default
in current release workflow.

---

## Windows (PowerShell)

```powershell
# Latest version
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

See `install.ps1` for details.

Windows release builds are currently not guaranteed on every release because the Windows pipeline still depends on
`cross`, Docker, and GitHub Actions minutes, which are costlier than the Linux path.

The Windows installer currently ships only the VT Code binary.
