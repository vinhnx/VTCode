# AGENTS.md

This file provides guidance to VT Code when working with code in this repository.

## Build & Test Commands

```bash
# Preferred development workflow
cargo check                          # Fast compile check
cargo test                           # Run tests
cargo test --package vtcode-core    # Test specific package
cargo clippy                         # Lint with strict rules
cargo fmt                           # Format code

# Additional commands
cargo test -- --nocapture           # Tests with output
cargo bench                          # Run performance benchmarks
cargo build                          # Build the project
cargo run -- ask "Hello world"      # Run VT Code CLI

# Run a single test
cargo test test_name -- --nocapture
```

## Workspace Structure

VT Code uses a **10-member workspace** architecture:

```
vtcode/                          # Binary entrypoint (src/main.rs)
├── vtcode-core/                 # Core library (LLM, tools, config, MCP)
├── vtcode-config/               # Configuration loader & schema
├── vtcode-commons/              # Shared utilities
├── vtcode-llm/                  # LLM provider abstractions
├── vtcode-tools/                # Tool implementations
├── vtcode-bash-runner/          # Shell execution engine
├── vtcode-markdown-store/       # Document storage
├── vtcode-indexer/              # Code indexing
├── vtcode-exec-events/          # Event definitions
└── vtcode-acp-client/           # Agent Client Protocol bridge
```

**Key separation**:

-   **vtcode-core/**: Reusable library with 77% complexity reduction through mode-based execution
-   **src/**: CLI executable (Ratatui TUI, PTY, slash commands)

## Architecture Highlights

Understanding these patterns requires reading multiple files across the codebase:

### Multi-Provider LLM System

-   **Location**: `vtcode-llm/`, `vtcode-core/src/llm/`
-   **Pattern**: Factory pattern with provider-specific request shaping
-   **Providers**: OpenAI, Anthropic, Gemini, xAI, DeepSeek, OpenRouter, Ollama, Z.AI, Moonshot AI, MiniMax
-   **Features**: Automatic failover, prompt caching, token budget tracking

### Trait-Based Tool System

-   **Location**: `vtcode-tools/`, `vtcode-core/src/tools/`
-   **Pattern**: Trait-driven composition (`Tool`, `ModeTool`, `CacheableTool`)
-   **Key**: Single source of truth for content search (grep_file) and file operations
-   **Tools**: 54+ specialized handlers with workspace-scoped isolation

### Code Intelligence Tool

-   **Location**: `vtcode-core/src/tools/code_intelligence.rs`
-   **Purpose**: LSP-like code navigation using tree-sitter
-   **Operations**:
    -   `goto_definition`: Find where a symbol is defined
    -   `find_references`: Find all references to a symbol
    -   `hover`: Get documentation and type info for a symbol
    -   `document_symbol`: Get all symbols in a file
    -   `workspace_symbol`: Search for symbols across the workspace
-   **Languages**: Rust, Python, JavaScript, TypeScript, Go, Java, Bash, Swift
-   **Usage**: Call `code_intelligence` tool with operation, file_path, line, and character parameters

### Configuration Precedence

**Critical**: Configuration flows in this order:

1. Environment variables (API keys, overrides)
2. `vtcode.toml` (runtime configuration)
3. `vtcode-core/src/config/constants.rs` (code constants)

### PTY Session Management

-   **Location**: `vtcode-core/src/exec/`, `vtcode-bash-runner/`
-   **Pattern**: Interactive shell sessions with streaming output
-   **Use**: Long-running commands, interactive workflows, real-time feedback

### Tree-Sitter Integration

-   **Location**: `vtcode-core/src/tree_sitter/`, `vtcode-indexer/`
-   **Languages**: Rust, Python, JavaScript/TypeScript, Go, Java, Bash (+ optional Swift)
-   **Pattern**: Incremental AST building with caching for semantic code analysis

### Protocol Integrations

-   **ACP** (Agent Client Protocol): `vtcode-acp-client/` - Zed IDE integration
-   **MCP** (Model Context Protocol): `vtcode-core/src/mcp/` - Extensible tooling via `rmcp`

## Communication Style

### Response Guidelines

-   **No emoji**: Never use emojis in responses. Keep output professional and text-based.
-   **Minimize exclamation points**: Use them sparingly; let the content speak for itself.
-   **Be concise**: Answer directly without unnecessary preamble, elaboration, or summaries.
-   **Avoid flattery**: Don't call ideas "good," "great," "interesting," or other positive adjectives. Respond directly to the request.
-   **Focus on the task**: Only address the user's specific query. Skip tangential information unless critical.
-   **One-to-three sentences**: Aim for brevity whenever possible. One-word answers are preferred for simple questions.
-   **No long introductions**: Get to the point immediately.
-   **Clean markdown**: Format responses with GitHub-flavored Markdown where appropriate.

## Code Style & Conventions

### Critical Standards from .github/copilot-instructions.md

**Error Handling**:

```rust
// ALWAYS use anyhow::Result<T> with context
use anyhow::{Context, Result};

pub async fn read_config(path: &str) -> Result<Config> {
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read config at {}", path))?;
    // Process...
}

// NEVER use unwrap()
// ❌ let data = result.unwrap();
// ✅ let data = result.with_context(|| "Description")?;
```

**Constants & Configuration**:

```rust
// NEVER hardcode values (especially model IDs)
// ❌ let model = "gpt-4";
// ❌ let timeout = 30;

// ✅ Use constants from vtcode-core/src/config/constants.rs
use vtcode_core::config::constants::DEFAULT_TIMEOUT;

// ✅ Read from vtcode.toml at runtime
let config = Config::load("vtcode.toml")?;

// ✅ Model metadata in docs/models.json
```

**Naming Conventions**:

-   `snake_case` for functions and variables
-   `PascalCase` for types and structs
-   Descriptive names, early returns
-   4-space indentation

**Documentation**:

-   All `.md` files go in `./docs/` directory (NOT in root)
-   `README.md` is the only exception (stays in root)
-   Use `docs/models.json` for latest LLM model metadata

## Testing Infrastructure

### Test Organization

-   **Unit tests**: Inline with source in `#[cfg(test)]` modules
-   **Integration tests**: `tests/` directory (20+ test files)
-   **Benchmarks**: `benches/` directory
-   **Mock data**: `tests/mock_data.rs` for realistic scenarios

### Running Specific Tests

```bash
# All tests
cargo test

# Integration tests
cargo test --test integration_tests

# Benchmarks
cargo bench
cargo bench -- search_benchmark

# With output
cargo test -- --nocapture
```

## Important Development Notes

### Git Operations

**Agent Must Not Perform Git Operations Automatically**

-   Do NOT run `git commit`, `git push`, `git merge`, or any destructive git operations on behalf of the user
-   Inform the user when changes need to be committed and let them handle git operations
-   Use `git status`, `git diff`, and `git log` only for diagnostic/informational purposes
-   Always inform the user before making changes that would affect git history

### Security & Safety

-   Validate all file paths (workspace boundary enforcement)
-   Command allowlist with per-argument validation
-   Tool policies (allow/deny/prompt) in `vtcode.toml`
-   Human-in-the-loop approval system

### Autonomy & Verification

-   **Verification Autonomy**: The agent MUST run verification commands (`cargo check`, `cargo test`, etc.) itself using `run_pty_cmd` after making changes. Do NOT ask the user to run these commands.
-   **Planning**: Use `update_plan` for any task requiring 4+ steps to maintain state and provide visibility to the user.

### Memory & Performance

-   LTO enabled even in dev profile (optimized for M4 Apple Silicon)
-   Single codegen unit for better optimization
-   Strict Clippy linting rules (see `Cargo.toml` workspace.lints)
-   No `expect_used`, `unwrap_used`, or manual implementations when stdlib has them

### Key Files Never to Hardcode

-   **Model IDs**: Use `docs/models.json`
-   **Constants**: Use `vtcode-core/src/config/constants.rs`
-   **Config values**: Read from `vtcode.toml`

### Common Pitfalls

1. **Don't hardcode model names** - they change frequently, use constants
2. **Don't use `unwrap()`** - use `.with_context()` for error context
3. **Don't create .md files in root** - they belong in `./docs/`
4. **Don't modify constants directly** - consider if it should be in `vtcode.toml` instead

## Development Workflow

### Before Committing

```bash
# Run all quality checks
cargo clippy && cargo fmt --check && cargo check && cargo test
```

### Adding New Features

1. Read existing code patterns first
2. Use configuration from `vtcode.toml` when possible
3. Add constants to `vtcode-core/src/config/constants.rs` if needed
4. Write tests (unit + integration)
5. Update documentation in `./docs/` if needed

### Environment Setup

```bash
# Set API key for your preferred provider
export OPENAI_API_KEY="sk-..."      # OpenAI
export ANTHROPIC_API_KEY="sk-..."   # Anthropic
export GEMINI_API_KEY="..."         # Google Gemini
# See docs/installation/ for all provider options

# Run VT Code
cargo run
```

## Additional Resources

-   **Architecture**: See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
-   **Security**: See [docs/security/SECURITY_MODEL.md](docs/security/SECURITY_MODEL.md)
-   **Contributing**: See [docs/CONTRIBUTING.md](docs/CONTRIBUTING.md)
-   **Configuration**: See [docs/config/CONFIGURATION_PRECEDENCE.md](docs/config/CONFIGURATION_PRECEDENCE.md)
-   **Provider Setup**: See [docs/providers/PROVIDER_GUIDES.md](docs/providers/PROVIDER_GUIDES.md)
-   **Testing Guide**: See [docs/development/testing.md](docs/development/testing.md)

## Agent Workflows

This CLAUDE.md focuses on the **VT Code codebase itself**.

## IMPORTANT:

-   When working on VT Code features, ALWAYS follow the guidelines in this document to ensure code quality, maintainability, and security.

-   Make sure the name is "VT Code" not "VTCODE" or "vtcode" in user-facing text.
