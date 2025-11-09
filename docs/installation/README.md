# Installation Guide

VT Code supports multiple installation methods. Choose the one that works best for you.

## Quick Install

### macOS & Linux
```bash
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash
```

### Windows (PowerShell)
```powershell
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex
```

### Homebrew (macOS & Linux)
```bash
brew install vtcode
```

### Cargo (Rust)
```bash
cargo install vtcode
```

### npm (Node.js)
```bash
npm install -g @vinhnx/vtcode
```

## Installation Methods

| Method | Platforms | Command | Notes |
|--------|-----------|---------|-------|
| **Native Installer** | macOS, Linux, WSL | See Quick Install above | Recommended, auto-detects platform |
| **Homebrew** | macOS, Linux | `brew install vtcode` | Package manager, easy updates |
| **Cargo** | All | `cargo install vtcode` | Build from source, latest dev version |
| **npm** | All | `npm install -g @vinhnx/vtcode` | JavaScript package manager |
| **npx** | All | `npx @vinhnx/vtcode` | No installation, run directly |

## After Installation

### 1. Verify it works
```bash
vtcode --version
```

### 2. Set your API key
```bash
export OPENAI_API_KEY="sk-..."
```

### 3. Launch VT Code
```bash
vtcode
```

## Supported AI Providers

- **OpenAI** (OPENAI_API_KEY)
- **Anthropic** (ANTHROPIC_API_KEY)
- **Google Gemini** (GEMINI_API_KEY)
- **xAI** (XAI_API_KEY)
- **DeepSeek** (DEEPSEEK_API_KEY)
- **OpenRouter** (OPENROUTER_API_KEY)
- **Ollama** (local, no API key)

Set the corresponding environment variable for your chosen provider.

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

### Still stuck?

- Open an issue: https://github.com/vinhnx/vtcode/issues
- Check docs: https://github.com/vinhnx/vtcode/docs
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
Remove-Item "$env:LOCALAPPDATA\VTCode\vtcode.exe"
# or (if in Program Files)
Remove-Item "C:\Program Files\VTCode\vtcode.exe"
```

## Installation Paths

### macOS & Linux
- `/usr/local/bin/vtcode` (standard)
- `/opt/local/bin/vtcode` (Homebrew ARM64)
- `~/.local/bin/vtcode` (user fallback)

### Windows
- `C:\Program Files\VTCode\vtcode.exe` (system-wide, requires admin)
- `%LOCALAPPDATA%\VTCode\vtcode.exe` (user-scoped)

The native installers automatically select the best location and add it to PATH.

## Additional Resources

- **[Detailed Native Installers Guide](./NATIVE_INSTALLERS.md)** - Technical details and advanced options
- **[Quick Reference](./QUICK_REFERENCE.md)** - One-liner commands
- **[GitHub Releases](https://github.com/vinhnx/vtcode/releases)** - Download binaries manually
- **[Documentation](https://github.com/vinhnx/vtcode/docs)** - Full documentation
