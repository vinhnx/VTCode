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

<p align="center">
  <strong>Terminal Coding Agent with Multi-Provider AI & Semantic Code Intelligence</strong>
</p>

<p align="center"><code>cargo install vtcode</code><br />or <code>brew install vinhnx/tap/vtcode</code><br />or <code>npm install -g vtcode</code></p>

<p align="center"><strong>VT Code</strong> is a sophisticated Rust-based terminal coding agent featuring a modern TUI, multi-provider AI support, semantic code understanding via tree-sitter, MCP integration, and advanced prompt caching.
</br>
</br>Built for developers who demand precision, security, performance, and extensibility in their coding workflows.</p>

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

VT Code is a production-ready terminal coding agent with a modular architecture supporting multiple LLM providers and advanced code intelligence capabilities.

**Key Features:**

- **Multi-Provider AI Support** - First-class integrations for OpenAI, Anthropic, xAI, DeepSeek, Gemini, and OpenRouter with auto-failover and intelligent routing
- **Semantic Code Intelligence** - Tree-sitter parsers for 6+ languages (Rust, Python, JavaScript, TypeScript, Go, Java) with ast-grep powered structural search and refactoring
- **Modern Terminal UI** - Built with Ratatui featuring mouse support, streaming PTY output, slash commands, and customizable themes (Ciapre and Catppuccin)
- **MCP Integration** - Model Context Protocol support for enhanced context awareness and external tool integration
- **Advanced Prompt Caching** - Multi-provider caching system reducing latency and API costs with configurable quality thresholds
- **Modular Tools System** - Trait-based architecture with mode-specific execution (terminal, PTY, streaming) and intelligent caching
- **Workspace Safety** - Git-aware navigation, boundary enforcement, command allowlists, and human-in-the-loop controls
- **Fully Configurable** - Every behavior controlled via `vtcode.toml`, with constants in `vtcode-core/src/config/constants.rs` and model IDs in `docs/models.json`

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
XAI_API_KEY=your_xai_key_here
GEMINI_API_KEY=your_gemini_key_here
OPENROUTER_API_KEY=your_openrouter_key_here
```

VT Code supports comprehensive configuration via `vtcode.toml` with sections for:

- **Agent settings**: Provider selection, model routing, reasoning effort, prompt refinement
- **Security controls**: Human-in-the-loop mode, workspace boundaries, command allowlists
- **Prompt caching**: Multi-provider caching with quality thresholds and cleanup settings
- **MCP integration**: External tool configuration and context providers
- **UI preferences**: Theme selection, chat surface, and display options

See the [Configuration Guide](docs/config/) for detailed documentation.

### Getting Started

Launch the agent with explicit provider/model flags or rely on the defaults from `vtcode.toml`:

```shell
vtcode --provider openrouter --model x-ai/grok-4-fast:free
```

The default configuration uses OpenRouter with `x-ai/grok-4-fast:free`. Customize your setup in `vtcode.toml`:

```toml
[agent]
provider = "openrouter"
default_model = "x-ai/grok-4-fast:free"

[router.models]
simple = "x-ai/grok-4-fast:free"
standard = "x-ai/grok-4-fast:free"
complex = "x-ai/grok-4-fast:free"
codegen_heavy = "x-ai/grok-4-fast:free"
retrieval_heavy = "x-ai/grok-4-fast:free"
```

All model identifiers are tracked in `vtcode-core/src/config/constants.rs` and `docs/models.json` for consistency with vetted releases.

Simply run `vtcode` in your working directory to start:

```shell
vtcode
```

---

## CLI Usage

**Interactive Mode:**

```shell
vtcode --provider openrouter --model x-ai/grok-4-fast:free
```

**Single Prompt Mode (Scripting Friendly):**

```shell
vtcode ask "Summarize diagnostics in src/lib.rs"
```

**Dry Run (No Tool Access):**

```shell
vtcode --no-tools ask "Review recent changes in src/main.rs"
```

**Development Mode:**

```shell
./run-debug.sh  # Debug build with live reload
./run.sh         # Production build
```

**Available Commands:**

- `vtcode --help` - Show all CLI options
- `vtcode ask "prompt"` - Single prompt mode with streaming
- `vtcode --no-tools` - Disable tool execution for safety
- `vtcode --config path` - Use custom configuration file

All behavior is controlled via `vtcode.toml` including provider routing, tool policies, caching settings, and safety controls.

---

## Architecture Overview

VT Code features a modular, trait-based architecture designed for extensibility and performance:

**Core Components:**

- **`vtcode-core/`** - Reusable library containing:
  - Provider abstractions (`llm/`) with multi-provider support and failover
  - Modular tools system (`tools/`) with trait-based composability
  - Configuration management with TOML-based settings
  - Tree-sitter integrations for semantic code analysis
  - Advanced prompt caching with multi-provider support

- **`src/main.rs`** - Thin CLI binary that orchestrates:
  - Command-line argument parsing with `clap`
  - Terminal UI rendering with Ratatui
  - Runtime coordination and session management

**Key Architectural Features:**

- **Modular Tools System** - Trait-based design with `Tool`, `ModeTool`, and `CacheableTool` traits supporting multiple execution modes (terminal, PTY, streaming)
- **MCP Integration** - Model Context Protocol support for external tools and context providers (Serena MCP for memory/journaling)
- **Multi-Provider AI** - Unified interface for OpenAI, Anthropic, xAI, DeepSeek, Gemini, and OpenRouter with intelligent routing
- **Advanced Caching** - Prompt caching across all providers with quality scoring and configurable cleanup
- **Async-First Design** - Tokio-based async operations with responsive command handling and real-time streaming

The [Architecture Guide](docs/ARCHITECTURE.md) provides detailed information about module responsibilities and extension points.

---

## Core Features

**Multi-Platform Installation**

- **Cargo**: `cargo install vtcode` - Install directly from crates.io
- **Homebrew**: `brew install vinhnx/tap/vtcode` - macOS package manager
- **npm**: `npm install -g vtcode` - Node.js package manager
- **GitHub Releases**: Pre-built binaries for macOS, Linux, and Windows

**Multi-Provider AI Support**

- **6 Major Providers**: OpenAI, Anthropic, xAI, OpenRouter, DeepSeek, and Gemini
- **Intelligent Routing**: Automatic provider selection based on task complexity and availability
- **Auto-Failover**: Seamless fallback when providers are unavailable
- **Latest Models**: Support for GPT-5, GPT-5 Codex, Grok 4, Claude 4.1, Gemini 2.5, and more
- **Cost Optimization**: Built-in cost guards and usage monitoring

**Advanced Terminal User Interface**

- **Modern TUI**: Built with Ratatui featuring mouse support and text selection
- **Real-time Output**: Streaming PTY execution with ANSI color support
- **Customizable Themes**: Ciapre theme palette plus Catppuccin support
- **Interactive Commands**: Slash commands with auto-completion and suggestions
- **Responsive Design**: Smooth scrolling, navigation controls, and contextual status bar
- **Multi-Surface Support**: Auto, alternate, and inline chat surfaces

**Semantic Code Intelligence**

- **Tree-sitter Integration**: Native parsing for 6+ languages (Rust, Python, JavaScript, TypeScript, Go, Java)
- **AST-Grep Power**: Structural search and refactoring capabilities
- **Git-Aware Navigation**: Fuzzy file search with `.gitignore` and `.ignore` support
- **Semantic Analysis**: Pattern recognition and intelligent code suggestions
- **Symbol Navigation**: Cross-file symbol lookup and definition finding

**Advanced Prompt Caching System**

- **Multi-Provider Caching**: OpenAI, Anthropic, Gemini, OpenRouter, and xAI support
- **Quality-Based Caching**: Configurable quality thresholds for cache decisions
- **Automatic Cleanup**: TTL-based cache management with size limits
- **Provider-Specific Features**:
  - OpenAI: Automatic caching with usage reporting
  - Anthropic: Cache control blocks with configurable TTL
  - Gemini: Implicit caching for 2.5 models
  - OpenRouter: Pass-through caching with savings metrics
- **Performance Impact**: Significant latency and token cost reduction

**Security & Safety Controls**

- **Workspace Boundaries**: Strict path validation and boundary enforcement
- **Command Allowlists**: Configurable safe command execution
- **Human-in-the-Loop**: Confirmation dialogs for destructive operations
- **Audit Logging**: Comprehensive operation logging and tracking
- **API Key Security**: Environment variable management with validation

**Modular Tools Architecture**

- **Trait-Based Design**: `Tool`, `ModeTool`, and `CacheableTool` traits for extensibility
- **Multi-Mode Execution**: Terminal, PTY, and streaming execution modes
- **Intelligent Caching**: Strategic caching for performance optimization
- **Plugin System**: Easy integration of custom tools and capabilities
- **MCP Integration**: External tool support via Model Context Protocol

---

## Configuration Reference

**Configuration Philosophy:**

- All agent behavior is controlled via `vtcode.toml` - never hardcode settings
- Model identifiers and constants are centralized in `vtcode-core/src/config/constants.rs`
- Latest model information is tracked in `docs/models.json` for consistency
- Provider-specific settings are organized by functionality (agent, security, caching, MCP)

**Key Configuration Sections:**

- **`[agent]`** - Core agent settings: provider selection, model routing, reasoning effort, prompt refinement
- **`[router.models]`** - Task-specific model assignments (simple, standard, complex, codegen_heavy, retrieval_heavy)
- **`[security]`** - Safety controls: human-in-the-loop mode, workspace boundaries, command allowlists
- **`[prompt_cache]`** - Multi-provider caching with quality thresholds, cleanup settings, and provider overrides
- **`[automation.full_auto]`** - Autonomous operation settings with tool restrictions and profile requirements
- **`[mcp]`** - Model Context Protocol configuration for external tools and context providers

**Provider-Specific Configuration:**
Each provider supports tailored settings for caching, authentication, and feature flags, allowing fine-grained control over behavior and performance.

See the [Configuration Guide](docs/config/) and [docs.rs reference](https://docs.rs/vtcode-core/latest/vtcode_core/config/index.html) for comprehensive documentation.

---

## Development Workflow

**Build & Validation:**

- `cargo check` - Fast compilation validation (preferred over `cargo build`)
- `cargo clippy --workspace --all-targets` - Comprehensive linting with project-specific rules
- `cargo fmt` - Code formatting with Rust standards
- `cargo test` - Unit and integration test coverage

**Development Scripts:**

- `./run-debug.sh` - Development build with live reload support
- `./run.sh` - Production build and run
- `./scripts/check.sh` - Comprehensive validation including tests and linting

**Project Structure:**

- **Core Library**: `vtcode-core/src/` - Reusable components and traits
- **CLI Binary**: `src/main.rs` - Application entry point and argument parsing
- **Benchmarks**: `benches/` - Performance benchmarks with Criterion
- **Tests**: `tests/` - Integration tests and examples (avoid ad-hoc scripts)
- **Documentation**: `docs/` - All project documentation (no docs in root)

**Development Guidelines:**

- Update `docs/models.json` when adding new model support
- Maintain configuration examples in `vtcode.toml` and `vtcode.toml.example`
- Add tests for new features in appropriate test modules
- Update documentation in `docs/` when changing public APIs
- Use `anyhow::Context` for descriptive error messages

---

## Documentation

**User Guides:**

- [**Getting Started**](docs/user-guide/getting-started.md) - Installation and basic usage
- [**Configuration Guide**](docs/config/) - Comprehensive configuration documentation
- [**Advanced Features**](docs/ADVANCED_FEATURES_IMPLEMENTATION.md) - Safety controls and automation features

**Technical Documentation:**

- [**Architecture Guide**](docs/ARCHITECTURE.md) - System architecture and design principles
- [**Provider Guides**](docs/providers/) - Provider-specific configuration and features
- [**Tool Documentation**](docs/tools/) - Available tools and their capabilities
- [**Development Guide**](docs/development/) - Contributing and development workflow

**API References:**

- [**vtcode API Reference**](https://docs.rs/vtcode) - Complete API documentation for the main application
- [**vtcode-core API Reference**](https://docs.rs/vtcode-core) - Complete API documentation for the core library

**Project Information:**

- [**Project Documentation**](docs/project/) - Project roadmap, changelog, and planning
- [**Research & Analysis**](docs/research/) - Technical research and performance analysis

---

## License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.
