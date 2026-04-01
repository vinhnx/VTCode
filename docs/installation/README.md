# Installation Guide

VT Code supports multiple installation methods. Choose the one that works best for you.

## Quick Install

The default macOS/Linux native installer also attempts the recommended `ripgrep` + `ast-grep` bundle.
Official macOS/Linux release archives bundle a `ghostty-vt/` runtime library directory for enhanced PTY snapshots. Installation still succeeds without it, and VT Code falls back to `legacy_vt100` automatically when Ghostty assets are unavailable.

### macOS & Linux

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Skip ripgrep + ast-grep if you only want VT Code
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash -s -- --without-search-tools
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### Homebrew (macOS & Linux)

```bash
brew install vtcode

# Optional after brew install
vtcode dependencies install search-tools
```

Homebrew does not fetch Ghostty VT runtime libraries separately. VT Code only uses them when they were already packaged next to the installed binary.
Fresh configs default to Ghostty; missing runtime libraries automatically fall back to `legacy_vt100`.

### Cargo (Rust)

```bash
cargo install vtcode

# Optional after any install method
vtcode dependencies install search-tools
```

`cargo install vtcode` installs the VT Code binary only. If you want Ghostty-backed PTY snapshots for a local build, stage the runtime libraries separately as described below.

### npm (Node.js)

```bash
npm install -g @vinhnx/vtcode --registry=https://npm.pkg.github.com
```

## Installation Methods

| Method               | Platforms         | Command                                                               | Notes                                 |
| -------------------- | ----------------- | --------------------------------------------------------------------- | ------------------------------------- |
| **Native Installer** | macOS, Linux, WSL | See Quick Install above                                               | Recommended, auto-detects platform    |
| **Homebrew**         | macOS, Linux      | `brew install vtcode`                                                 | Package manager, easy updates         |
| **Cargo**            | All               | `cargo install vtcode`                                                | Build from source, latest dev version |
| **npm**              | All               | `npm install -g @vinhnx/vtcode --registry=https://npm.pkg.github.com` | JavaScript package manager            |
| **npx**              | All               | `npx @vinhnx/vtcode`                                                  | No installation, run directly         |

## After Installation

### 1. Verify it works

```bash
vtcode --version
```

### 1a. Ghostty VT runtime libraries

Official macOS/Linux release archives and native installers place a `ghostty-vt/` directory next to the VT Code binary for PTY screen snapshots.

- Native installers copy the bundled runtime libraries automatically.
- Homebrew/Cargo/npm installs may not include Ghostty VT assets.
- VT Code continues to work without the runtime libraries by falling back to `pty.emulation_backend = "legacy_vt100"`.
- VT Code does not currently install Ghostty VT through `vtcode dependencies install ...`; unlike the search tools bundle, Ghostty is supplied as packaged runtime libraries.
- `run.sh` and `run-debug.sh` will auto-bootstrap and stage the runtime libraries locally when they are missing.

For local repository builds, bootstrap and stage them with:

```bash
bash scripts/setup-ghostty-vt-dev.sh "$(rustc -vV | sed -n 's/^host: //p')"
./scripts/run.sh
./scripts/run-debug.sh
```

For packaging details, see [Ghostty VT Packaging](../development/GHOSTTY_VT_PACKAGING.md).

### 2. Set your API key

```bash
export OPENAI_API_KEY="sk-..."
```

### 3. Launch VT Code

```bash
vtcode
```

## Supported AI Providers

-   **GitHub Copilot** (requires `copilot` CLI; see [GitHub Copilot Auth](../guides/oauth-authentication.md#github-copilot-managed-auth))
-   **OpenAI** (OPENAI_API_KEY)
-   **Anthropic** (ANTHROPIC_API_KEY)
-   **Google Gemini** (GEMINI_API_KEY)
-   **xAI** (XAI_API_KEY)
-   **DeepSeek** (DEEPSEEK_API_KEY)
-   **OpenRouter** (OPENROUTER_API_KEY)
-   **Ollama** (local, no API key)
-   **LM Studio** (local, no API key by default)

Set the corresponding environment variable for your chosen provider. For GitHub Copilot, install the `copilot` CLI and authenticate using `copilot login`.

## Troubleshooting

### Command not found after installation

**macOS/Linux:**

```bash
# Refresh your shell
source ~/.bashrc    # bash
source ~/.zshrc     # zsh
```

**Windows:**
Restart PowerShell or Command Prompt.

### Installation fails with "No such file or directory"

This typically indicates a network or CDN caching issue. Try one of:

```bash
# Force fresh download
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Or use GitHub API (always fresh)
curl -fsSL "https://api.github.com/repos/vinhnx/vtcode/contents/scripts/install.sh?ref=main" | jq -r '.content' | base64 -d | bash
```

### Permission denied

**macOS/Linux:**

```bash
chmod +x /usr/local/bin/vtcode
```

**Windows:**
Run PowerShell as Administrator.

### Download failed

1. Check internet connection: `curl https://api.github.com`
2. Verify GitHub is accessible
3. Try again in a fresh terminal
4. Check [GitHub status](https://www.githubstatus.com/)

### Still stuck?

-   Open an issue: https://github.com/vinhnx/vtcode/issues
-   Check docs: https://github.com/vinhnx/vtcode/docs
-   See [detailed guide](./NATIVE_INSTALLERS.md)

## Uninstall

### Native Installer (Shell)

```bash
rm /usr/local/bin/vtcode
# or
rm ~/.local/bin/vtcode
rm -rf /usr/local/bin/ghostty-vt
# or
rm -rf ~/.local/bin/ghostty-vt
```

### Homebrew

```bash
brew uninstall vtcode
```

### Cargo

```bash
cargo uninstall vtcode
```

### npm

```bash
npm uninstall -g @vinhnx/vtcode
```

### Windows (PowerShell)

```powershell
Remove-Item "$env:LOCALAPPDATA\VT Code\vtcode.exe"
# or (if in Program Files)
Remove-Item "C:\Program Files\VT Code\vtcode.exe"
Remove-Item "$env:LOCALAPPDATA\VT Code\ghostty-vt" -Recurse -Force -ErrorAction SilentlyContinue
# or
Remove-Item "C:\Program Files\VT Code\ghostty-vt" -Recurse -Force -ErrorAction SilentlyContinue
```

## Installation Paths

### macOS & Linux

-   `/usr/local/bin/vtcode` (standard)
-   `/opt/local/bin/vtcode` (Homebrew ARM64)
-   `~/.local/bin/vtcode` (user fallback)
-   Optional Ghostty runtime libraries: sibling `ghostty-vt/` directory

### Windows

-   `C:\Program Files\VT Code\vtcode.exe` (system-wide, requires admin)
-   `%LOCALAPPDATA%\VT Code\vtcode.exe` (user-scoped)
-   No bundled Ghostty runtime libraries; Windows uses `legacy_vt100`

The native installers automatically select the best location and add it to PATH.

## Additional Resources

-   **[Detailed Native Installers Guide](./NATIVE_INSTALLERS.md)** - Technical details and advanced options
-   **[Quick Reference](./QUICK_REFERENCE.md)** - One-liner commands
-   **[GitHub Releases](https://github.com/vinhnx/vtcode/releases)** - Download binaries manually
-   **[Documentation](https://github.com/vinhnx/vtcode/docs)** - Full documentation
