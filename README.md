# VT Code

[![humaneval pass@1](docs/benchmarks/reports/benchmark_badge.svg)](docs/benchmarks)

[![MCP](https://img.shields.io/badge/model%20context%20protocol-black?style=for-the-badge&logo=modelcontextprotocol)](https://github.com/vinhnx/vtcode/blob/main/docs/guides/mcp-integration.md) [![zed](https://img.shields.io/badge/agent%20client%20protocol-black?style=for-the-badge&logo=zedindustries)](https://agentclientprotocol.com/overview/agents)

[![crates.io](https://img.shields.io/crates/v/vtcode.svg?style=flat-square&label=crates.io&logo=rust)](https://crates.io/crates/vtcode) [![docs.rs](https://img.shields.io/docsrs/vtcode.svg?style=flat-square&label=docs.rs&logo=docsdotrs)](https://docs.rs/vtcode) [![npm](https://img.shields.io/npm/v/vtcode.svg?style=flat-square&label=npm&logo=npm)](https://www.npmjs.com/package/vtcode)

---

`cargo install vtcode`

or `brew install vinhnx/tap/vtcode` (macOS)

or `npm install -g vtcode`

---

**VT Code** is a Rust-based terminal coding agent with semantic code intelligence via Tree-sitter and ast-grep. It supports multiple LLM providers with automatic failover and efficient context management.

## Key Features

- **Multi-Provider AI**: OpenAI, Anthropic, xAI, DeepSeek, Gemini, Z.AI, Moonshot AI, OpenRouter, Ollama (local)
- **Code Intelligence**: Tree-sitter parsers for Rust, Python, JavaScript/TypeScript, Go, Java
- **Smart Tools**: Built-in code analysis, file operations, terminal commands, and refactoring
- **Editor Integration**: Native support for Zed IDE via Agent Client Protocol (ACP)
- **Security First**: Sandboxed execution with configurable safety policies

## Installation

### Package Managers

```bash
# Cargo (recommended)
cargo install vtcode

# Homebrew (macOS)
brew install vinhnx/tap/vtcode

# NPM
npm install -g vtcode
```

## Quick Start

```bash
# Set your API key
export OPENAI_API_KEY="your_api_key_here"

# Launch VT Code
vtcode

# Or run a single query
vtcode ask "Explain this Rust code"
```

## Configuration

Create `vtcode.toml` in your project root:

```toml
[agent]
provider = "openai"                    # Choose your provider
default_model = "gpt-5"               # Latest model
api_key_env = "OPENAI_API_KEY"        # Environment variable

[tools]
default_policy = "prompt"             # Safety: "allow", "prompt", or "deny"

[tools.policies]
read_file = "allow"                   # Always allow file reading
write_file = "prompt"                 # Prompt before modifications
run_terminal_cmd = "prompt"           # Prompt before commands
```

### Available Providers

Set your API key environment variable:

```bash
export OPENAI_API_KEY="sk-..."           # OpenAI
export ANTHROPIC_API_KEY="sk-ant-..."    # Anthropic
export GEMINI_API_KEY="AIza..."          # Google Gemini
export XAI_API_KEY="xai-..."             # xAI
export DEEPSEEK_API_KEY="sk-..."         # DeepSeek
export ZAI_API_KEY="zai-..."             # Z.AI
export MOONSHOT_API_KEY="sk-..."         # Moonshot AI
export OPENROUTER_API_KEY="sk-or-..."    # OpenRouter
```

## Command Line Interface

### Basic Usage

```bash
# Interactive mode
vtcode

# Single query mode
vtcode ask "your question here"

# With specific provider and model
vtcode --provider openai --model gpt-5 ask "Refactor this code"
```

## Agent Client Protocol

VT Code supports the **[Agent Client Protocol (ACP)](https://agentclientprotocol.com/)** for integration with code editors like Zed.

### ACP Quick Setup

1. **Install VT Code** with `cargo install vtcode`
2. **Configure** your `vtcode.toml` with provider credentials
3. **Register** in Zed's settings:

```jsonc
{
    "agent_servers": {
        "vtcode": {
            "command": "vtcode",
            "args": ["acp"],
            "env": {
                "OPENAI_API_KEY": "your_api_key_here"
            }
        }
    }
}
```

## Development

### Getting Started

```bash
# Clone and build
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
cargo build --release

# Run tests
cargo test
```

## Support VT Code Development

I build VT Code in my free time as a passion [project](https://github.com/vinhnx/vtcode?tab=readme-ov-file#technical-motivation) to research and explore how coding agents work in practice. If you find VT Code useful, please consider supporting my work with a coffee via
[BuyMeACoffee](https://www.buymeacoffee.com/vinhnx):

[![BuyMeACoffee](https://raw.githubusercontent.com/pachadotdev/buymeacoffee-badges/main/bmc-black.svg)](https://www.buymeacoffee.com/vinhnx)

## License

MIT License - see [LICENSE](LICENSE) for full terms.