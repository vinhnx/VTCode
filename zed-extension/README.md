# VTCode Zed Extension

An AI coding assistant for the Zed editor - A Rust-based terminal coding agent with semantic code intelligence and multi-provider AI support.

## Features

- **AI Coding Assistant**: Access the VTCode agent directly from Zed
- **Code Analysis**: Analyze your workspace with semantic code intelligence
- **Configuration Support**: Syntax highlighting and autocomplete for `vtcode.toml`
- **Context Awareness**: Leverages Tree-sitter for deep code understanding
- **Multi-Provider AI**: Supports OpenAI, Anthropic, Google, xAI, DeepSeek, and more
- **Security First**: Built-in safeguards with human-in-the-loop controls

## Installation

1. Install the VTCode CLI:
```bash
# Install with Cargo (recommended)
cargo install vtcode

# Or with Homebrew
brew install vtcode

# Or with NPM
npm install -g vtcode-ai
```

2. Install this extension in Zed:
   - Open Extensions in Zed
   - Search for "vtcode"
   - Click "Install"

## Usage

### Ask the Agent

Use the command palette to invoke VTCode commands:
- `vtcode: Ask the Agent` - Send a question to the VTCode agent
- `vtcode: Analyze Workspace` - Run workspace analysis
- `vtcode: Launch Chat` - Open an interactive chat session
- `vtcode: Open Configuration` - Edit your `vtcode.toml`

### Configuration

VTCode reads your workspace `vtcode.toml` configuration file. The extension provides:

- Syntax highlighting for TOML format
- Validation of vtcode.toml structure
- Inline documentation for common settings

## Configuration Options

Set these in your `vtcode.toml`:

```toml
[ai]
provider = "anthropic"  # or "openai", "gemini", etc.
model = "claude-3-5-sonnet-20241022"

[workspace]
analyze_on_startup = false
max_tokens = 8000

[security]
human_in_the_loop = true
tool_policies_enabled = true
```

## Requirements

- Zed editor
- VTCode CLI installed and in your PATH
- A workspace with `vtcode.toml` file

## Getting Started

1. Create a `vtcode.toml` in your workspace root
2. Configure your AI provider and preferences
3. Open the command palette and search for "vtcode" commands
4. Start collaborating with the AI agent

## Support

For issues and feature requests, visit the [VTCode GitHub repository](https://github.com/vinhnx/vtcode).

## License

MIT License - See LICENSE file for details
