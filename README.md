# VT Code

[![crates.io](https://img.shields.io/crates/v/vtcode.svg?style=for-the-badge&label=crates.io&logo=rust)](https://crates.io/crates/vtcode)
[![docs.rs](https://img.shields.io/docsrs/vtcode.svg?style=for-the-badge&label=docs.rs&logo=docsdotrs)](https://docs.rs/vtcode)
[![npm](https://img.shields.io/npm/v/vtcode.svg?style=for-the-badge&label=npm&logo=npm)](https://www.npmjs.com/package/vtcode)

`cargo install vtcode`
or `brew install vinhnx/tap/vtcode` (macOS)
or `npm install -g vtcode`

**VT Code** is a Rust-based terminal coding agent with semantic code intelligence via Tree-sitter (parsers for Rust, Python, JavaScript/TypeScript, Go, Java) and ast-grep (structural pattern matching and refactoring).

It supports multiple LLM providers: OpenAI, Anthropic, xAI, DeepSeek, Gemini, OpenRouter, all with automatic failover, prompt caching, and token-efficient context management. Configuration occurs entirely through `vtcode.toml`, sourcing constants from `vtcode-core/src/config/constants.rs` and model IDs from `docs/models.json` to ensure reproducibility and avoid hardcoding.

![Demo](resources/vhs/demo.gif)

## Technical Motivation

VT Code addresses limitations in existing coding agents by prioritizing Rust's type safety, zero-cost abstractions, and async ecosystem for reliable, high-performance execution. Motivated by agentic AI research (e.g., Anthropic's context engineering principles), it integrates Tree-sitter for precise parsing and MCP for extensible tooling. This enables long-running sessions with maintained context integrity, error resilience, and minimal token overhead. Builds on foundational work like [perg](https://crates.io/crates/perg) while incorporating lessons from Anthropic's [Context Engineering](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) and [Building effective agents](https://www.anthropic.com/engineering/building-effective-agents) guide. Inspiration from OpenAI's [codex-cli](https://github.com/openai/codex).

## System Architecture

The architecture divides into `vtcode-core` (reusable library) and `src/` (CLI executable), leveraging Tokio for multi-threaded async runtime (`#[tokio::main(flavor = "multi_thread")]` for CPU-intensive tasks), anyhow for contextual error propagation, and clap for derive-based CLI parsing. Key design tenets include atomic operations, metadata-driven tool calls (to optimize context tokens), and phase-aware context curation.

### Core Components (`vtcode-core/`)

-   **LLM Abstractions (`llm/`)**:
    Provider traits enable uniform async interfaces:

    ```rust
    #[async_trait::async_trait]
    pub trait Provider: Send + Sync {
        async fn complete(&self, prompt: &str) -> anyhow::Result<Completion>;
        fn supports_caching(&self) -> bool;
    }
    ```

    Features: Streaming responses, model-specific optimizations (e.g., Anthropic's `cache_control: { ttl: "5m" }` for 5-minute TTL; OpenAI's `prompt_tokens_details.cached_tokens` reporting ~40% savings). Tokenization via `tiktoken-rs` ensures accurate budgeting across models.

-   **Modular Tools (`tools/`)**:
    Trait-based extensibility:

    ```rust
    #[async_trait]
    pub trait Tool: Send + Sync {
        fn name(&self) -> &'static str;
        fn description(&self) -> &'static str;
        async fn execute(&self, args: serde_json::Value) -> anyhow::Result<serde_json::Value>;
    }
    ```

    Built-ins include `read_file` (chunked at 2000 lines, metadata-first), `ast_grep_search` (operations: search/transform/lint/refactor with preview_only=true), and `run_terminal_cmd` (modes: terminal/pty/streaming; 30s timeout default). Git integration via `list_files` uses `walkdir` with `ignore` crate for .gitignore-aware traversal and `nucleo-matcher` for fuzzy scoring.

-   **Configuration Engine (`config/`)**:
    Deserializes `vtcode.toml` into structs with validation:

    ```toml
    [context.curation]
    enabled = true
    max_tokens_per_turn = 100000  # Enforce per-provider limits
    phase_detection = true  # Auto-classify: exploration/implementation/etc.
    ```

    Sections cover agents, tools (allow/deny), MCP (provider URLs), caching (quality_threshold=0.7), and safety (workspace_paths, max_file_size=1MB).

-   **Context Engineering System**:
    Implements iterative, per-turn curation based on conversation phase detection (e.g., exploration prioritizes search tools). Token budgeting: Real-time tracking with `tiktoken-rs` (~10μs/message), thresholds (0.75 warn/0.85 compact), and automatic summarization (LLM-driven, preserving decision ledger and errors; targets 30% compression ratio, saving ~29% tokens/turn). Decision ledger: Structured audit (`Vec<DecisionEntry>` with status: pending/in_progress/completed, confidence: 0-1). Error recovery: Pattern matching (e.g., parse failures) with fallback strategies and context preservation.

-   **Code Intelligence**:
    Tree-sitter integration for AST traversal (e.g., symbol resolution in `tools/ast_grep_search`); ast-grep for rule-based transforms:

    ```yaml
    # Example pattern in tool call
    pattern: "fn $NAME($PARAMS) { $BODY }"
    replacement: "async fn $NAME($PARAMS) -> Result<()> { $BODY.await }"
    ```

    Supports preview mode to avoid destructive applies.

-   **MCP Integration**:
    Client uses official Rust SDK for protocol-compliant calls:
    ```rust
    let client = McpClient::new("ws://localhost:8080");
    let docs = client.call("get-library-docs", json!({
        "context7CompatibleLibraryID": "/tokio/docs",
        "tokens": 5000,
        "topic": "async runtime"
    })).await?;
    ```
    Discovers tools dynamically (e.g., `mcp_resolve-library-id` for Context7 IDs, `mcp_sequentialthinking` for chain-of-thought reasoning with branch/revision support, `mcp_get_current_time` for timezone-aware ops). Connection pooling and failover for multi-provider setups.

### CLI Execution (`src/`)

-   **User Interface**: Ratatui for reactive TUI (mouse-enabled, ANSI escape sequences for colors: e.g., \x1b[34m for blue tool banners). Real-time PTY via `vte` crate for command streaming; slash commands parsed with fuzzy matching.
-   **Runtime**: Tokio executor handles concurrent tool calls; human-in-the-loop via confirmation prompts for high-risk ops (e.g., `rm -rf` denials).
-   **Observability**: Logs to file/console with structured format; metrics (e.g., cache hit rate, token usage) exposed via debug flags.

Performance notes: Multi-threaded Tokio reduces latency for I/O-bound tasks (~20% faster than single-thread); context compression yields 50-80% token savings in long sessions. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for dependency graph and profiling data.

## Key Capabilities

-   **LLM Orchestration**: Failover logic (e.g., Gemini primary, OpenAI fallback); reasoning control (low/medium/high effort via provider params); caching with quality gating (cache only >70% confidence, TTL=30 days).
-   **Code Analysis & Editing**: Semantic search (AST-grep similarity mode, threshold=0.7); targeted edits (exact string match in `edit_file`, preserving whitespace); multi-file patches via `apply_patch`.
-   **Context & Session Management**: Phase-adaptive tool selection (e.g., validation phase favors `run_terminal_cmd` with `cargo test`); ledger injection for coherence (max 12 entries); summarization triggers at 20 turns or 85% budget.
-   **Extensibility**: Custom tools via trait impls; MCP for domain-specific extensions (e.g., library docs resolution: `resolve-library-id` → `get-library-docs` with max_tokens=5000).
-   **Security Posture**: Path validation (no escapes outside WORKSPACE_DIR); sandboxed network (curl HTTPS only, no localhost); allowlists (e.g., deny `rm`, permit `cargo`); env-var secrets (no file storage).

## Installation and Initialization

Binaries on [GitHub Releases](https://github.com/vinhnx/vtcode/releases/latest) support macOS (aarch64/x86_64-apple-darwin), Linux (x86_64/aarch64-unknown-linux-gnu), Windows (x86_64-pc-windows-msvc).

Initialize environment:

```bash
# API keys (required for providers)
export OPENAI_API_KEY="your_api_key"  # Validate via curl test
# [RECOMMEND] use dot file .env in your project root: OPENAI_API_KEY=sk-...

# Workspace setup
cd my-project
cp /path/to/vtcode.toml.example .vtcode/vtcode.toml  # Customize [mcp.enabled=true] etc.
```

Execution:

```bash
vtcode  # Launch TUI (loads vtcode.toml defaults)
vtcode --provider openai --model gpt-5-codex ask "Refactor async fn in src/lib.rs using ast-grep"  # Tool-enabled query
vtcode --debug --no-tools ask "Compute token budget for current context"  # Dry-run analysis
```

### Zed IDE integration (Agent Client Protocol)

The ACP bridge lets Zed treat VT Code as an external agent. The full walkthrough lives in
[`docs/guides/zed-acp.md`](docs/guides/zed-acp.md); the summary below captures the critical steps.

#### Setup overview
- Ensure a VT Code binary is built and reachable (either on `PATH` or via an absolute path).
- Enable the ACP bridge in `vtcode.toml` (or with environment overrides).
- Register VT Code as a custom ACP server inside Zed's `settings.json`.
- Launch the agent from Zed and confirm the stdio transport is healthy via ACP logs.

#### Prerequisites
- Rust toolchain matching `rust-toolchain.toml`.
- A configured `vtcode.toml` with provider, model, and credentials.
- Zed `v0.201` or newer with the Agent Client Protocol feature flag enabled.

Build VT Code once for editor use (release profile recommended):

```bash
cargo build --release
```

If the binary is not on `PATH`, note the absolute location (`target/release/vtcode`).

#### Configure VT Code for ACP
1. Edit your `vtcode.toml` and enable the bridge:

    ```toml
    [acp]
    enabled = true

        [acp.zed]
        enabled = true
        transport = "stdio"

            [acp.zed.tools]
            read_file = true
    ```

    Environment overrides:

    | Variable | Purpose |
    | --- | --- |
    | `VT_ACP_ENABLED` | Toggles the global ACP bridge. |
    | `VT_ACP_ZED_ENABLED` | Enables the Zed transport. |
    | `VT_ACP_ZED_TOOLS_READ_FILE_ENABLED` | Controls the `read_file` tool forwarding. |

    Zed must advertise the `fs.read_text_file` capability during the ACP handshake. If the
    initialization request omits it, VT Code leaves the `read_file` tool disabled and surfaces a
    reasoning notice inside the turn.

    Disable the tool bridge when your provider does not expose function calling (for example
    `openai/gpt-oss-20b:free` on OpenRouter). VT Code streams a reasoning notice back to Zed when it
    detects unsupported tool calls and automatically downgrades to plain completions. When enabled,
    Zed now prompts for approval before each `read_file` tool invocation so you can gate sensitive
    paths. If the permission dialog cannot be presented, the tool call is cancelled rather than
    proceeding without consent. All `read_file` arguments must reference absolute workspace paths;
    relative values are rejected before reaching the client. Cancelling a turn in Zed immediately sets
    the ACP stop reason to `cancelled`, short-circuits pending tool executions, and reports the tool
    calls as cancelled so no additional output is sent after you abort the run. Each prompt also
    publishes an ACP execution plan that tracks when VT Code is analysing the request, gathering
    workspace context, and composing the final reply so Zed's UI mirrors the bridge's progress.

2. Run a manual smoke test:

    ```bash
    ./target/release/vtcode acp
    ```

    Append `--config /absolute/path/to/vtcode.toml` if your configuration lives outside the default
    lookup chain. Successful startup leaves the process waiting on stdio; stop it with `Ctrl+C`.

#### Register VT Code in Zed
Add a custom agent entry to `settings.json`:

```jsonc
{
    "agent_servers": {
        "vtcode": {
            "command": "/absolute/path/to/vtcode",
            "args": ["acp"],
            "env": {
                "VT_ACP_ENABLED": "1",
                "VT_ACP_ZED_ENABLED": "1",
                "RUST_LOG": "info"
            },
            "cwd": "/workspace/containing/vtcode"
        }
    }
}
```

If the binary is on `PATH`, shrink `command` to `"vtcode"`. Add extra flags (for example `--config`) as
needed to match your environment.

#### Use it inside Zed
1. Open the agent panel (`Cmd-?` on macOS) and create an **External Agent**.
2. Pick the `vtcode` entry you added. Zed spawns the ACP bridge over stdio.
3. Chat as normal; mention files with `@path/to/file` or attach buffers. Tool requests for
   `read_file` forward to Zed when enabled.

#### Debugging and verification
- Command palette → `dev: open acp logs` surfaces raw ACP traffic.
- Empty responses usually mean the environment overrides were not applied; double-check the `env`
  map in `settings.json`.
- Errors referencing the transport imply the config drifted away from `transport = "stdio"`.
- VT Code emits notices when tool calls are skipped because the model lacks function calling.

Configuration validation: On load, checks TOML against schema (e.g., model in `docs/models.json`); logs warnings for deprecated keys.

## Command-Line Interface

Parsed via clap derives:

```bash
vtcode [OPTIONS] <SUBCOMMAND>

SUBCOMMANDS:
    ask <QUERY>     Non-interactive query with optional tools
    OPTIONS:
        --provider <PROVIDER>    LLM provider (default: from config)
        --model <MODEL>          Model ID (e.g., gemini-1.5-pro; from constants.rs)
        --no-tools               Disable tool execution (analysis only)
        --debug                  Enable verbose logging and metrics
        --help                   Print usage
```
Development runs: `./run.sh` (release profile: lto=true, codegen-units=1 for opt); `./run-debug.sh` (debug symbols, hot-reload via cargo-watch).

## Development Practices

-   **Validation Pipeline**: `cargo check` for incremental builds; `cargo clippy` with custom lints (e.g., no unsafe, prefer ? over unwrap); `cargo fmt --check` for style enforcement (4-space indents, max_width=100).
-   **Testing**: Unit tests in `#[cfg(test)]` modules; integration in `tests/` (e.g., mock LLM responses with wiremock); coverage via `cargo tarpaulin`. Property testing for tools (e.g., proptest for path sanitization).
-   **Error Handling**: Uniform `anyhow::Result<T>` with context:
    ```rust
    fn load_config(path: &Path) -> anyhow::Result<Config> {
        toml::from_str(&fs::read_to_string(path)?)
            .with_context(|| format!("Invalid TOML in {}", path.display()))?
    }
    ```
-   **Benchmarking**: Criterion in `benches/` (e.g., token counting: <50μs for 1k tokens); flamegraphs for async bottlenecks.
-   **Documentation**: Rustdoc for public APIs; Markdown in `./docs/` (e.g., [context engineering impl](docs/phase_1_2_implementation_summary.md)). No root-level Markdown except README.

Adhere to [CONTRIBUTING.md](CONTRIBUTING.md): Conventional commits, PR templates with benchmarks.

## References

-   **User Guides**: [Getting Started](docs/user-guide/getting-started.md), [Configuration](docs/config/).
-   **Technical Docs**: [Context Engineering](docs/context_engineering.md), [MCP Setup](docs/mcp_integration.md), [Prompt Caching](docs/tools/PROMPT_CACHING_GUIDE.md).
-   **API**: [vtcode-core](https://docs.rs/vtcode-core) (full crate docs).
-   **Changelog**: [CHANGELOG](CHANGELOG.md).

## License

MIT License - [LICENSE](LICENSE) for full terms.
