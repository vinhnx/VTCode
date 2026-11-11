# Installation Guide

VT Code supports multiple installation methods. Choose the one that works best for you.

## Quick Install

VT Code is self-contained: it ships its own search engine and needs no external search binaries.

### macOS & Linux

```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

Windows release builds are currently best-effort and may lag behind macOS/Linux. The release pipeline still depends on
`cross`, Docker, and GitHub Actions capacity for Windows packaging, and those build minutes can be costly, so Windows
artifacts may not appear in every release.

### Homebrew (macOS & Linux)

```bash
brew trust vinhnx/tap
brew install vinhnx/tap/vtcode

# Desktop app (macOS 15+, Apple Silicon)
brew install --cask vinhnx/tap/vtcode-desktop

# Optional after brew install
vtcode dependencies install search-tools
```

`brew trust` is required once per machine: Homebrew refuses to load formulae and casks from an untrusted third-party
tap. The cask is separate from the formula — installing it gives you the GUI, while `brew install vinhnx/tap/vtcode`
gives you the `vtcode` CLI, and either works alone.

### Cargo (Rust)

```bash
cargo install vtcode

# Optional after any install method
vtcode dependencies install search-tools
```

## Installation Methods

| Method               | Platforms         | Command                                                   | Notes                                 |
| -------------------- | ----------------- | --------------------------------------------------------- | ------------------------------------- |
| **Native Installer** | macOS, Linux, WSL | See Quick Install above                                   | Recommended, auto-detects platform    |
| **Homebrew**         | macOS, Linux      | `brew trust vinhnx/tap && brew install vinhnx/tap/vtcode` | Package manager, easy updates         |
| **Homebrew Cask**    | macOS 15+         | `brew install --cask vinhnx/tap/vtcode-desktop`           | Desktop app, Apple Silicon            |
| **Cargo**            | All               | `cargo install vtcode`                                    | Build from source, latest dev version |

Windows release artifacts may lag behind macOS/Linux while the build pipeline is being finalized.

## Desktop App (macOS 15+)

> [!WARNING]
> **One-time macOS security step required.** Release builds are ad hoc signed, not notarized, so macOS blocks the
> first launch. Allow it once, then open the app again:
>
> 1. Open the app from Applications so macOS records the block
> 2. Open **System Settings › Privacy & Security**
> 3. Click **Open Anyway** next to the VT Code message
> 4. Open the app again — from Finder, or run `vtcode desktop`
>
> **Open Anyway only appears for about an hour after a blocked launch.** If it is gone, open the app again to
> re-trigger the block. You only do this once; macOS 15+ removed the old right-click › Open shortcut for
> unnotarized apps, and notarized releases skip the step entirely.

The optional native macOS GUI installs from a prebuilt release asset — no build toolchain needed. Pick one install
method per machine, because each manages updates differently.

```bash
# Homebrew cask (Apple Silicon) — update with `brew upgrade --cask vinhnx/tap/vtcode-desktop`
brew trust vinhnx/tap
brew install --cask vinhnx/tap/vtcode-desktop

# Or let the native installer manage it: it asks during CLI install (default: install)
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Or install/update on demand; updates reinstall in place
vtcode desktop --install

# Or together with a CLI update
vtcode update --with-desktop
```

The cask places the app in `/Applications`; the installer and `vtcode desktop --install` use `~/Applications` unless
you pick `/Applications` when asked. The cask currently ships Apple Silicon builds only. Intel Macs and releases
without a prebuilt desktop asset can still install the app from source with `vtcode desktop --install`, or download
the release zip directly.

Mixing update paths on one machine is the main source of confusion: updating a cask-managed bundle with
`vtcode update --with-desktop` leaves Homebrew's version record stale. Keep using `brew upgrade --cask` for cask
installs (see [Update System](../guides/UPDATE_SYSTEM.md#desktop-app-macos)).

See the [desktop workspace guide](../development/desktop-workspace.md) for requirements and the
[installer guide](./NATIVE_INSTALLERS.md) for flags (`--with-desktop` / `--without-desktop`, `DESKTOP_INSTALL_DIR`).

## After Installation

### 1. Verify it works

```bash
vtcode --version
```

### 2. Set your API key

```bash
export OPENAI_API_KEY="sk-..."
```

### 3. Initialize your project

```bash
cd your-project
vtcode init    # scaffolds vtcode.toml, .vtcode/, and AGENTS.md
```

### 4. Launch VT Code

```bash
vtcode
```

## Supported AI Providers

- **GitHub Copilot** (requires `copilot` CLI; see
  [GitHub Copilot Auth](../guides/oauth-authentication.md#github-copilot-managed-auth))
- **OpenAI** (OPENAI_API_KEY)
- **Anthropic** (ANTHROPIC_API_KEY)
- **Google Gemini** (GEMINI_API_KEY)
- **Meta AI** (MODEL_API_KEY or META_API_KEY; see [Meta AI Provider](../providers/meta.md))
- **NVIDIA NIM** (NVIDIA_API_KEY; see [NVIDIA NIM Provider](../providers/nvidia.md))
- **Merge Gateway** (MERGE_GATEWAY_API_KEY; see [Merge Gateway Provider](../providers/merge-gateway.md))
- **xAI** (XAI_API_KEY)
- **DeepSeek** (DEEPSEEK_API_KEY)
- **OpenRouter** (OPENROUTER_API_KEY)
- **Vercel AI Gateway** (AI_GATEWAY_API_KEY; see [Vercel AI Gateway Provider](../providers/vercel-ai-gateway.md))
- **Ollama** (local, no API key)
- **LM Studio** (local, no API key by default)

Set the corresponding environment variable for your chosen provider. For GitHub Copilot, install the `copilot` CLI and
authenticate using `copilot login`.

## Troubleshooting

### Command not found after installation

**macOS/Linux:**

```bash
# Refresh your shell
source ~/.bashrc    # bash
source ~/.zshrc     # zsh
```

**Windows:** Restart PowerShell or Command Prompt.

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

**Windows:** Run PowerShell as Administrator.

### Download failed

1. Check internet connection: `curl https://api.github.com`
2. Verify GitHub is accessible
3. Try again in a fresh terminal
4. Check [GitHub status](https://www.githubstatus.com/)

### Still stuck?

- Open an issue: <https://github.com/vinhnx/vtcode/issues>
- Check docs: <https://github.com/vinhnx/vtcode/docs>
- See [detailed guide](./NATIVE_INSTALLERS.md)

## Uninstall

### Native Installer (Shell)

```bash
rm /usr/local/bin/vtcode
# or
rm ~/.local/bin/vtcode
```

### Homebrew

```bash
# CLI formula
brew uninstall vinhnx/tap/vtcode

# Desktop app cask
brew uninstall --cask vinhnx/tap/vtcode-desktop

# Optional: also remove desktop preferences and saved window state
brew uninstall --cask --zap vinhnx/tap/vtcode-desktop
```

`--zap` removes only the GUI's own state (`~/.vtcode/desktop` and the app's preferences). CLI sessions, credentials,
and configuration live under `~/Library/Application Support/com.vinhnx.vtcode` and are left untouched.

### Cargo

```bash
cargo uninstall vtcode
```

### Windows (Uninstall)

```powershell
Remove-Item "$env:LOCALAPPDATA\VT Code\vtcode.exe"
# or (if in Program Files)
Remove-Item "C:\Program Files\VT Code\vtcode.exe"
```

### Desktop App (macOS, installed by the app or shell installer)

```bash
rm -rf ~/Applications/VT\ Code.app
# or, for a system-wide install
sudo rm -rf /Applications/VT\ Code.app
```

If you installed the app with the Homebrew cask, uninstall it with `brew uninstall --cask vinhnx/tap/vtcode-desktop`
instead so Homebrew's install record stays consistent.

## Installation Paths

### macOS & Linux Paths

- `/usr/local/bin/vtcode` (standard)
- `/opt/homebrew/bin/vtcode` (Homebrew formula, Apple Silicon)
- `/opt/local/bin/vtcode` (Homebrew ARM64)
- `~/.local/bin/vtcode` (user fallback)
- `~/Applications/VT Code.app` (desktop app, user install)
- `/Applications/VT Code.app` (desktop app; Homebrew cask and system-wide install)

### Windows Paths

- `C:\Program Files\VT Code\vtcode.exe` (system-wide, requires admin)
- `%LOCALAPPDATA%\VT Code\vtcode.exe` (user-scoped)

The native installers automatically select the best location and add it to PATH.

## Additional Resources

- **[Detailed Native Installers Guide](./NATIVE_INSTALLERS.md)** - Technical details and advanced options
- **[Quick Reference](./QUICK_REFERENCE.md)** - One-liner commands
- **[GitHub Releases](https://github.com/vinhnx/vtcode/releases)** - Download binaries manually
- **[Documentation](https://github.com/vinhnx/vtcode/docs)** - Full documentation
