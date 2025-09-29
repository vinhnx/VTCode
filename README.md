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

<p align="center"><strong>VT Code</strong> is a Rust-based terminal coding agent with semantic code understanding powered by <a href="https://tree-sitter.github.io/tree-sitter/">tree-sitter</a> and <a href="https://ast-grep.github.io/">ast-grep</a>, and fully <a href="https://docs.rs/vtcode-core/latest/vtcode_core/config/index.html"><b>configurable</b></a> for steering the Agent.</p>

<p align="center">
  <img src="resources/vhs/demo.gif" alt="Demo" />
</p>

</div>

---

## Background

VT Code represents my own research into how coding agents work in practice and explores how far I can push the boundaries of building one. Yes, the development is AI-assisted, primarily using [OpenAI Codex](https://chatgpt.com/codex). While my main background is in [mobile development and Swift](https://vinhnx.github.io/), I have a deep appreciation for [Rust](https://www.rust-lang.org/)'s ecosystem and its robust ownership and memory management model.

I previously built perg ([GitHub](https://github.com/vinhnx/perg), [Crate](https://crates.io/crates/perg)) as a learn-by-doing project around 2019. Since then, [perg](https://crates.io/crates/perg) has been downloaded over 10,000 times on [crates.io](https://crates.io/crates/perg). For this coding agent project, I chose Rust as the primary programming language and toolchain, inspired by OpenAI's implementation of [codex-cli](https://github.com/openai/codex) in Rust.

I believe the best way to understand how to build a full coding agent is by actually building one. Additionally, by open-sourcing VT Code, I hope to give back to the [Rust community](https://github.com/rust-unofficial/awesome-rust), which I'm proud to be part of. 🦀

## VT Code

VT Code excels at codebase context understanding, semantic code intelligence, and comprehensive safety controls. While the features are fully built and complete, you are in control of how the agent operates on your workspace through various configs, tool-use policies, and advanced shell-command safeguards. 

**Core Capabilities:**

- **Multi-Provider AI Agent** - First-class integrations for OpenAI, Anthropic, xAI, DeepSeek, Gemini, and OpenRouter with auto-failover and intelligent cost guards
- **Context Engineering Foundation** - Advanced context compression, multi-provider prompt caching, conversation intelligence, and MCP integration for optimal long-session performance
- **Semantic Code Intelligence** - Tree-sitter parsers for 6+ languages (Rust, Python, JavaScript, TypeScript, Go, Java) combined with ast-grep powered structural search and refactoring
- **Modern Terminal Experience** - Built with Ratatui featuring mouse support, streaming PTY output, slash commands, and customizable themes (Ciapre and Catppuccin)
- **MCP Integration** - Model Context Protocol support for enhanced context awareness and external tool integration via official Rust SDK
- **Advanced Prompt Caching** - Multi-provider caching system with quality-based decisions, configurable cleanup, and significant latency/cost reduction
- **Modular Tools Architecture** - Trait-based design with `Tool`, `ModeTool`, and `CacheableTool` traits supporting multiple execution modes
- **Workspace Awareness** - Git-aware fuzzy navigation, boundary enforcement, command allowlists, and human-in-the-loop confirmations
- **Fully Configurable** - Every agent behavior controlled via `vtcode.toml`, with constants in `vtcode-core/src/config/constants.rs` and model IDs in `docs/models.json`

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

VT Code supports advanced configuration via `vtcode.toml` with comprehensive sections for agent behavior, security controls, prompt caching, MCP integration, and UI preferences. See [Configuration Guide](docs/config/) for details.

### Getting Started

Launch the agent with explicit provider/model flags or rely on the defaults from `vtcode.toml`:

```shell
export OPENAI_API_KEY="your_key_here" # or check the OPENAI_API_KEY value on .env file
vtcode --provider openai --model gpt-5-codex
```

The default configuration uses OpenRouter with `x-ai/grok-4-fast:free`. You can always customize your setup in `vtcode.toml` to your preferred models and config, and optional with router models for various tasks:

Create a vtcode.toml or copy the content of [vtcode.toml.example](https://github.com/vinhnx/vtcode/blob/main/vtcode.toml.example) to `.vtcode/` directory inside your working reposity.

```bash
cd {work_dir}
mkdir .vtcode
```

Then config of your choice

`./vtcode/vtcode.toml`

```toml
[agent]
provider = "openai"
default_model = "gpt-5"

[router.models]
simple = "gpt-5"
standard = "gpt-5"
complex = "gpt-5-codex"
codegen_heavy = "gpt-5-codex"
retrieval_heavy = "gpt-5-codex"
```

### Notes

* For full configurable options, check [vtcode.toml.example](https://github.com/vinhnx/vtcode/blob/main/vtcode.toml.example), or [vtcode.toml](https://github.com/vinhnx/vtcode/blob/main/vtcode.toml)
* Model identifiers should always reference `vtcode-core/src/config/constants.rs` and `docs/models.json` to stay aligned with vetted releases.

Simply spawn `vtcode` agent in your working directory:

```shell
vtcode
```

---

## CLI Usage

- Launch interactive mode with your preferred provider/model:

    ```shell
    vtcode --provider openrouter --model x-ai/grok-4-fast:free
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

VT Code is composed of a reusable core library plus a thin CLI binary, built around a sophisticated context engineering foundation:

- `vtcode-core/` contains the agent runtime with advanced context management:
  - **Context Engineering Core** (`core/`): Context compression, conversation summarization, decision tracking, and performance monitoring
  - **Provider Abstractions** (`llm/`): Multi-provider support with intelligent caching and failover
  - **Modular Tools System** (`tools/`): Trait-based architecture with context-aware tool execution
  - **Configuration Management** (`config/`): Centralized configuration with context-aware defaults
  - **Tree-sitter Integration**: Semantic parsing with context preservation and workspace awareness
  - **MCP Client** (`mcp_client.rs`): Official Rust SDK integration for enhanced contextual resources
- `src/main.rs` wires the CLI, TUI, and runtime together using `clap` for argument parsing and Ratatui for rendering
- **Context-Aware MCP Integration**: Model Context Protocol tools extend the agent with enhanced context awareness via official [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
- **Tree-sitter & AST Analysis**: Semantic code intelligence with context-aware parsing and structural search via `ast-grep`

Design goals prioritize **contextual intelligence**, composability, guarded execution, and predictable performance. The architecture document in `docs/ARCHITECTURE.md` dives deeper into module responsibilities and extension hooks, with particular focus on the context engineering patterns that enable long-running, high-quality coding sessions.

---

## Context Engineering Foundation

VT Code's context engineering foundation represents a sophisticated approach to managing conversational AI context, ensuring optimal performance, cost efficiency, and response quality across long-running coding sessions.

### Advanced Context Compression

**Intelligent Context Management:**

- **Dynamic Compression**: Automatically compresses conversation context when approaching token limits (80% threshold by default)
- **Smart Preservation**: Preserves recent turns (5 by default), system messages, error messages, and tool calls
- **Decision-Aware**: Maintains decision ledger summaries and critical workflow information during compression
- **Quality Metrics**: Tracks compression ratios and maintains context quality through LLM-powered summarization

**Compression Architecture:**

```rust
// Core compression engine with configurable thresholds
ContextCompressor {
    max_context_length: 128000,      // ~128K tokens
    compression_threshold: 0.8,      // Trigger at 80% capacity
    preserve_recent_turns: 5,        // Always keep recent messages
    preserve_system_messages: true,  // Critical system context
    preserve_error_messages: true,   // Error patterns and solutions
}
```

### Multi-Provider Prompt Caching

**Sophisticated Caching Strategy:**

- **Quality-Based Decisions**: Only caches high-quality responses (70% confidence threshold)
- **Provider-Specific Optimization**: Tailored caching for OpenAI, Anthropic, Gemini, OpenRouter, and xAI
- **Automatic Cleanup**: Configurable cache lifecycle management with age-based expiration
- **Cost Optimization**: Significant latency and token cost reduction through intelligent caching

**Provider-Specific Caching:**

- **OpenAI**: Automatic caching for `gpt-5`, `gpt-5-codex`, `4o`, `4o mini`... with detailed token reporting
- **Anthropic**: Explicit cache control with 5-minute and 1-hour TTL options via `cache_control` blocks
- **Google Gemini**: Implicit caching for 2.5 models with explicit cache creation APIs
- **OpenRouter**: Pass-through provider caching with savings reporting via `cache_discount`
- **xAI**: Automatic platform-level caching with usage metrics

### Conversation Intelligence & Summarization

**Session-Aware Context:**

- **Turn Tracking**: Maintains conversation flow with automatic turn counting and session duration tracking
- **Decision Logging**: Records key decisions, tool executions, and workflow changes with importance scoring
- **Error Pattern Analysis**: Identifies recurring error patterns and provides proactive solutions
- **Task Completion Tracking**: Monitors completed tasks, success rates, and tool usage patterns

**Intelligent Summarization:**

```rust
// Advanced conversation summarization
ConversationSummarizer {
    summarization_threshold: 20,     // Summarize after 20 turns
    max_summary_length: 2000,        // Concise summary generation
    compression_target_ratio: 0.3,   // Target 70% size reduction
}
```

### Model Context Protocol (MCP) Integration

**Enhanced Context Awareness:**

- **External Tool Integration**: Connects to external systems via official [Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)
- **Contextual Resources**: Provides additional context through MCP servers
- **Multi-Provider Tools**: Aggregates tools across multiple MCP providers with connection pooling
- **Intelligent Routing**: Routes tool calls to appropriate MCP providers based on capabilities

**MCP Architecture:**

```rust
// High-level MCP client with provider management
McpClient {
    providers: HashMap<String, McpProvider>,
    active_connections: Arc<Mutex<HashMap<String, RunningMcpService>>>,
    tool_discovery: Automatic tool enumeration and caching,
}
```

### Workspace & Context Awareness

**Intelligent Context Boundaries:**

- **Git-Aware Navigation**: Context-aware file discovery using `.gitignore` patterns and `nucleo-matcher`
- **Workspace Boundary Enforcement**: Prevents operations outside configured workspace boundaries
- **Project Structure Understanding**: Leverages tree-sitter parsers for semantic code navigation
- **Multi-Language Support**: Context-aware parsing for Rust, Python, JavaScript, TypeScript, Go, and Java

**Context-Aware Features:**

- **Semantic Search**: AST-powered structural search and refactoring with `ast-grep`
- **Code Intelligence**: Symbol lookup, definition finding, and reference tracking
- **Fuzzy Navigation**: Intelligent file and symbol matching with workspace awareness

### Advanced Prompt Engineering

**Context-Optimized Prompts:**

- **Dynamic Prompt Refinement**: Multi-pass prompt optimization for complex tasks
- **Provider-Specific Templates**: Tailored prompt structures for different LLM capabilities
- **Self-Review Mechanisms**: Optional self-review passes for enhanced response quality
- **Reasoning Effort Control**: Configurable reasoning depth for supported models

**Prompt Management:**

```toml
[prompt_cache]
enabled = true
min_quality_threshold = 0.7      # Only cache high-quality responses
max_age_days = 30                # Automatic cleanup after 30 days

[agent]
reasoning_effort = "medium"      # Control model reasoning depth
refine_prompts_enabled = false   # Enable prompt optimization
```

### Context Quality & Performance Metrics

**Comprehensive Monitoring:**

- **Cache Hit Rates**: Tracks cache effectiveness across providers
- **Context Compression Ratios**: Monitors compression efficiency and quality preservation
- **Response Quality Scoring**: Evaluates cached response quality for retention decisions
- **Session Performance**: Tracks conversation health, error rates, and completion rates

**Quality Assurance:**

- **Automatic Quality Scoring**: LLM-powered evaluation of response quality
- **Context Preservation Validation**: Ensures critical information survives compression
- **Error Pattern Recognition**: Identifies and addresses recurring context-related issues

This context engineering foundation enables VT Code to maintain high-quality, cost-effective AI assistance across extended coding sessions while preserving critical workflow context and decision history.

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
- Support for the latest models including OpenAI's `gpt-5`, `gpt-5-codex`; Anthropic's `Claude 4.1 Opus`, `Claude 4 Sonnet`; xAI's `grok 4`, `Grok Code Fast`; Gemini 2.5 latest, and all OpenRouters [models](https://openrouter.ai/models), with reasoning effort configurable.

**Enhanced Terminal User Interface**

- Modern TUI with mouse support and text selection
- Real-time terminal command output with ANSI color support and PTY streaming
- Color-coded tool and MCP status banners highlight execution summaries versus detailed output for faster scanning
- Customizable themes with my own [Ciapre](https://github.com/vinhnx/Ciapre) theme palette (or Catppuccin via config)
- Interactive slash commands with auto-suggestions
- Smooth scrolling and navigation controls
- Dedicated status bar with contextual information

**Advanced Code Intelligence**

- **Context-Aware Tree-sitter Parsing**: Semantic analysis for 6+ languages (Rust, Python, JavaScript, TypeScript, Go, Java) with workspace context preservation
- **AST-Powered Structural Search**: Advanced pattern recognition and refactoring using `ast-grep` with semantic understanding
- **Intelligent Code Navigation**: Context-aware symbol lookup, definition finding, and reference tracking
- **Git-Aware Fuzzy Search**: Intelligent file discovery using `.gitignore` patterns and `nucleo-matcher` with workspace boundary enforcement
- **Semantic Refactoring**: Context-preserving code transformations with structural pattern matching

**Performance & Cost Optimization**

- **Prompt Caching**: Automatic and configurable caching of conversation prefixes across providers to reduce latency and token consumption
  - OpenAI: Automatic caching for gpt-5, gpt-5-codex, 4o, 4o mini, o1-preview/mini with `prompt_tokens_details.cached_tokens` reporting
  - Anthropic: Explicit cache control via `cache_control` blocks with 5-minute and 1-hour TTL options
  - Google Gemini: Implicit caching for 2.5 models with explicit cache creation APIs available
  - OpenRouter: Pass-through provider caching with savings reporting via `cache_discount`
  - xAI: Automatic platform-level caching with usage metrics
- Configurable cache settings per provider in `vtcode.toml`
- Quality scoring to determine which responses to cache

**You're in control**

- Steerable agent's behavior via [vtcode.toml](https://github.com/vinhnx/vtcode/blob/main/vtcode.toml).
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
