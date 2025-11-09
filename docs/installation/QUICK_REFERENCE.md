# Quick Reference

## Install

```bash
# macOS & Linux
curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash

# Windows (PowerShell)
irm https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.ps1 | iex

# Homebrew
brew install vtcode

# Cargo
cargo install vtcode

# npm
npm install -g @vinhnx/vtcode
```

## Quick Start

```bash
export OPENAI_API_KEY="sk-..."
vtcode
```

## Uninstall

```bash
# Shell installer
rm /usr/local/bin/vtcode

# Homebrew
brew uninstall vtcode

# Cargo
cargo uninstall vtcode

# npm
npm uninstall -g @vinhnx/vtcode
```

## Verify

```bash
vtcode --version
```

## API Keys

```bash
export OPENAI_API_KEY="..."       # OpenAI
export ANTHROPIC_API_KEY="..."    # Anthropic
export GEMINI_API_KEY="..."       # Google Gemini
export XAI_API_KEY="..."          # xAI
export DEEPSEEK_API_KEY="..."     # DeepSeek
export OPENROUTER_API_KEY="..."   # OpenRouter
```

## Resources

- Docs: https://github.com/vinhnx/vtcode/docs
- Issues: https://github.com/vinhnx/vtcode/issues
- [Full Installation Guide](./README.md)
- [Technical Details](./NATIVE_INSTALLERS.md)
