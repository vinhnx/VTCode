<div align="center">

<h1>VT Code</h1>

<p align="center">
  <a href="https://crates.io/crates/vtcode">
    <img alt="crates.io" src="https://img.shields.io/crates/v/vtcode.svg?style=for-the-badge&label=crates.io&logo=rust" />
  </a>
  <a href="https://docs.rs/vtcode">
    <img alt="docs.rs" src="https://img.shields.io/docsrs/vtcode.svg?style=for-the-badge&label=docs.rs&logo=docsdotrs" />
  </a>
  <a href="https://www.npmjs.com/package/vtcode">
    <img alt="npm" src="https://img.shields.io/npm/v/vtcode.svg?style=for-the-badge&label=npm&logo=npm" />
  </a>
</p>

<p align="center"><code>cargo install vtcode</code><br />or <code>brew install vinhnx/tap/vtcode</code><br />or <code>npm install -g vtcode</code></p>

<p align="center"><strong>VT Code</strong> is a Rust-based terminal coding agent that pairs a modern TUI with deep, semantic code understanding powered by tree-sitter and ast-grep, and fully <a href="https://docs.rs/vtcode-core/latest/vtcode_core/config/index.html"><b>configurable</b></a> for steering the Agent.
</br>
</br>Built for developers who demand precision, security, and efficiency in everyday coding workflows.</p>

<p align="center">
  <img src="resources/vhs/demo.gif" alt="Demo" />
</br>
  <a href="https://ratatui.rs/">
    <img alt="Built With Ratatui" src="https://ratatui.rs/built-with-ratatui/badge.svg?style=for-the-badge&label=ratatui.rs&logo=ratatui" />
  </a>
</p>

</div>

---

## VT Code

VT Code is a research-preview semantic coding agent. While the features are fully built and complete, you are in control of how the agent operates on workspace with various configs, tool-use policy and advanced shell-commands safe guard.

- **Multi-provider agent** with first-class integrations for OpenAI, Anthropic, xAI, DeepSeek, Gemini, and OpenRouter, including auto-failover and cost guards.
- **Semantic code intelligence** using tree-sitter parsers for Rust, Python, JavaScript, TypeScript, Go, and Java, combined with ast-grep powered structural search and refactors.
- **Modern terminal experience** built with Ratatui: mouse support, streaming PTY output, slash commands, customizable with my own [Ciapre](https://github.com/vinhnx/Ciapre.tmTheme) themes, and ANSI-accurate rendering.
- **Workspace aware by default**: Git-aware fuzzy navigation, boundary enforcement, command allowlists, and human-in-the-loop confirmations.
- **Config driven**: every agent behavior is controlled via [vtcode.toml](https://github.com/vinhnx/vtcode/blob/main/vtcode.toml), backed by constants in `vtcode-core/src/config/constants.rs` and up-to-date model IDs in `docs/models.json`.

---

## Quickstart

### Installing and running VT Code

VT Code can be installed using multiple package managers depending on your preference:

**Using Cargo (Rust package manager) - Recommended for Rust developers:**

```shell
cargo install vtcode
```

**Using Homebrew (macOS only):**

```shell
brew install vinhnx/tap/vtcode
```

**Using npm (Node.js package manager) - Cross-platform:**

```shell
npm install -g vtcode
```

After installation with any method, simply run `vtcode` to get started:

```shell
vtcode
```

<details>
<summary>You can also download pre-built binaries from <a href="https://github.com/vinhnx/vtcode/releases/latest">GitHub Releases</a>.</summary>

Available for:

- **macOS**: Apple Silicon (`aarch64-apple-darwin`) and Intel (`x86_64-apple-darwin`)
- **Linux**: x86_64 and ARM64 architectures
- **Windows**: x86_64 architecture

Each archive contains the executable - extract and rename to `vtcode` if needed.

</details>

### Configuration

Set your API key for your preferred provider:

```shell
export OPENAI_API_KEY="your_key_here"
# or
export ANTHROPIC_API_KEY="your_key_here"
# or
export XAI_API_KEY="your_key_here"
# or
export GEMINI_API_KEY="your_key_here"
# or
export OPENROUTER_API_KEY="your_key_here"
```

Alternatively, create a `.env` file in your project directory:

```shell
# .env file
OPENAI_API_KEY=your_openai_key_here
ANTHROPIC_API_KEY=your_anthropic_key_here
XAI_API_KEY=your_anthropic_key_here
GEMINI_API_KEY=your_gemini_key_here
OPENROUTER_API_KEY=your_openrouter_key_here
```

VT Code supports advanced configuration via `vtcode.toml`. See [Configuration](https://docs.rs/vtcode-core/latest/vtcode_core/config/index.html) for details.

### Getting Started

Launch the agent with explicit provider/model flags or rely on the defaults from `vtcode.toml`:

```shell
vtcode --provider openai --model gpt-5-codex
```

Persist your preferred defaults in configuration rather than hardcoding them:

```toml
[agent]
provider = "openai"
default_model = "gpt-5-codex"
```

Model identifiers should always reference `vtcode-core/src/config/constants.rs` and `docs/models.json` to stay aligned with vetted releases.

Sinply spawn `vtcode` agent in your working directory:

 ```shell
    vtcode
```


---

## CLI Usage

- Launch interactive mode with your preferred provider/model:

    ```shell
    vtcode --provider openai --model gpt-5-codex
    ```

- Run a single prompt with streaming output (scripting friendly):

    ```shell
    vtcode ask "Summarize diagnostics in src/lib.rs"
    ```

- Execute a command with tool access disabled (dry run):

    ```shell
    vtcode --no-tools ask "Review recent changes in src/main.rs"
    ```

- When developing locally, the debug script mirrors production defaults:

    ```shell
    ./run-debug.sh
    ```

CLI options are discoverable via `vtcode --help` or `/help` inside the REPL. All defaults live in `vtcode.toml`, including provider fallbacks, tool allowlists, streaming options, and safety policies.

---

## Architecture Overview

VT Code is composed of a reusable core library plus a thin CLI binary:

- `vtcode-core/` contains the agent runtime: provider abstractions (`llm/`), tool registry (`tools/`), configuration loaders, and tree-sitter integrations.
- `src/main.rs` wires the CLI, TUI, and runtime together using `clap` for argument parsing and Ratatui for rendering.
- MCP (Model Context Protocol) tools extend the agent with contextual resources; configuration lives in `vtcode.toml` and enables systems like Serena MCP for journaling and memory.
- Tree-sitter parsers and ast-grep power semantic analysis; both are orchestrated asynchronously with Tokio for responsive command handling.

Design goals prioritize composability, guarded execution, and predictable performance. The architecture document in `docs/ARCHITECTURE.md` dives deeper into module responsibilities and extension hooks.

---

## Core Features

**Multi-Platform Installation**

- **Cargo**: `cargo install vtcode` - Install directly from crates.io
- **Homebrew**: `brew install vinhnx/tap/vtcode` - macOS package manager installation
- **npm**: `npm install -g vtcode` - Node.js package manager installation
- **GitHub Releases**: Pre-built binaries for macOS, Linux, and Windows

**Multi-Provider AI Support**

- OpenAI, Anthropic, xAI, OpenRouter, DeepSeek, and Gemini integration
- Automatic provider selection and failover
- Cost optimization with safety controls
- Support for the latest models including GPT-5, GPT-5 Codex, Grok 4, Grok Code Fast, Claude 4.1 Opus, Claude 4 Sonnet, and Qwen3 Coder Plus

**Enhanced Terminal User Interface**

- Modern TUI with mouse support and text selection
- Real-time terminal command output with ANSI color support and PTY streaming
- Customizable themes with my own [Ciapre](https://github.com/vinhnx/Ciapre) theme palette (or Catppuccin via config)
- Interactive slash commands with auto-suggestions
- Smooth scrolling and navigation controls
- Dedicated status bar with contextual information

**Advanced Code Intelligence**

- Tree-sitter parsing for 6+ languages (Rust, Python, JavaScript, TypeScript, Go, Java)
- Semantic code analysis and pattern recognition
- Intelligent refactoring and optimization suggestions
- Git-aware fuzzy file search backed by the `ignore` and `nucleo-matcher` crates
- Code navigation and symbol lookup

**Performance & Cost Optimization**

- **Prompt Caching**: Automatic and configurable caching of conversation prefixes across providers to reduce latency and token consumption
  - OpenAI: Automatic caching for gpt-5, gpt-5-codex, 4o, 4o mini, o1-preview/mini with `prompt_tokens_details.cached_tokens` reporting
  - Anthropic: Explicit cache control via `cache_control` blocks with 5-minute and 1-hour TTL options
  - Google Gemini: Implicit caching for 2.5 models with explicit cache creation APIs available
  - OpenRouter: Pass-through provider caching with savings reporting via `cache_discount`
  - xAI: Automatic platform-level caching with usage metrics
- Configurable cache settings per provider in `vtcode.toml`
- Quality scoring to determine which responses to cache

**You're in controll**

- Workspace boundary enforcement
- Configurable command allowlists
- Human-in-the-loop controls for safety
- Comprehensive audit logging
- Secure API key management

**Modular Architecture**

- Trait-based tool system for extensibility
- Multi-mode execution (terminal, PTY, streaming)
- Intelligent caching and performance optimization
- Plugin architecture for custom tools
- Configurable agent workflows

---

## Configuration Reference

- All agent knobs live in `vtcode.toml`; never hardcode credentials or model IDs.
- Constants (model aliases, file size limits, defaults) are centralized in `vtcode-core/src/config/constants.rs`.
- The latest provider-specific model identifiers are tracked in `docs/models.json`; update it alongside configuration changes.
- Prompt caching controls are available under the `[prompt_cache]` section with provider-specific overrides for OpenAI, Anthropic, Gemini, OpenRouter, and xAI.
- Safety settings include workspace boundary enforcement, command allow/deny lists, rate limits, and telemetry toggles.

Refer to the guides under [docs.rs](https://docs.rs/vtcode-core/latest/vtcode_core/config/index.html) for deep dives on providers, tools, and runtime profiles.

---

## Development Workflow

- `cargo check` for fast validation; `cargo clippy --workspace --all-targets` to enforce linting.
- Format with `cargo fmt` and run `cargo test` for unit and integration coverage.
- `./run-debug.sh` launches the debug build with live reload-friendly options.
- Benchmarks live in `benches/`, and additional examples belong in `tests/` (avoid ad-hoc scripts).
- Ensure configuration updates are reflected in `docs/project/` and `docs/models.json` when relevant.

---

## Documentation

- [**Getting Started**](docs/user-guide/getting-started.md) - Installation and basic usage
- [**Configuration**](docs/project/) - Advanced configuration options including prompt caching
- [**Architecture**](docs/ARCHITECTURE.md) - Technical architecture details
- [**Advanced Features**](docs/ADVANCED_FEATURES_IMPLEMENTATION.md) - Safety controls and debug mode
- [**Prompt Caching Guide**](docs/tools/PROMPT_CACHING_GUIDE.md) - Comprehensive guide to prompt caching configuration and usage
- [**Prompt Caching Implementation**](docs/providers/PROMPT_CACHING_UPDATE.md) - Detailed documentation on the latest prompt caching changes
- [**vtcode API Reference**](https://docs.rs/vtcode) - Complete API documentation for the main app
- [**vtcode-core API Reference**](https://docs.rs/vtcode-core) - Complete API documentation for the core logic
- [**Contributing**](CONTRIBUTING.md) - Development guidelines

---

## License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.
