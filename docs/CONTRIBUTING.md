# Contributing to VT Code

Welcome to VT Code! We're excited that you're interested in contributing to this Rust-based terminal coding agent. This
document outlines the guidelines and best practices for contributing to the project.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Code Style](#code-style)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Out of scope contributions](#out-of-scope-contributions)
- [Development Guidelines](#development-guidelines)
- [Architecture Overview](#architecture-overview)
- [Community](#community)

## Getting Started

VT Code is a Rust-based terminal coding agent with LLM-native code understanding and robust shell safety. It supports
multiple LLM providers with automatic failover, prompt caching, and token-efficient context management.

Before contributing, please familiarize yourself with:

1. The [README.md](README.md) for an overview of the project
2. The [AGENTS.md](../AGENTS.md) for agent governance, workspace conventions, and per-crate guidance
3. The [Architecture documentation](./architecture.md) for understanding the system design
4. The [Extension Boundaries guide](./development/extension_boundaries.md) before adding new trait- or provider-shaped
   extension points
5. The [Development Guide](./development/README.md) for detailed development processes

## Development Setup

### Prerequisites

- Rust (latest stable version) - Install from [rust-lang.org](https://www.rust-lang.org/tools/install)
- Git
- An API key from one of the supported providers (OpenAI, Anthropic, xAI, etc.)

### Setup Process

```bash
# 1. Fork and clone the repository
git clone https://github.com/your-username/vtcode.git
cd vtcode

# 2. Build the project
cargo build

# 3. Run tests to ensure everything works
cargo test

# 4. Check code quality
cargo clippy
cargo fmt --check

# 5. Try running VT Code
cargo run -- ask "Hello world"
```

### Environment Setup

Set up your API key environment variable:

```bash
# For OpenAI (adjust for your preferred provider)
export OPENAI_API_KEY="sk-..."  # Replace with your actual API key
```

## Project Structure

Cargo workspace with ~30 crates. Rust stable, MSRV 1.100.0, edition 2024. See
[Rust Toolchain and MSRV](./development/toolchain-updates.md) before changing the pin.

- **`src/`** (root): Binary crate — CLI, session bootstrap
- **`vtcode-core/`**: Agent loop, tools, prompts, LLM orchestration, UI
- **`vtcode-ui/`**: Unified UI — design system, theme registry, TUI framework
- **`vtcode-config/`**: Config loading and schema
- **`vtcode-bash-runner/`**: Shell execution sandbox
- **`vtcode-acp/`**: Agent Client Protocol (Zed)
- **`vtcode-auth/`**: OAuth and credential storage
- **`vtcode-indexer/`**: Code indexing and search
- **`vtcode-exec-events/`**: `ThreadEvent` contract and ATIF export
- **`vtcode-commons/`**: Shared utilities
- **`vtcode-macros/`**: Procedural macros
- **`vtcode-llm/`**: LLM provider abstraction and streaming
- **`vtcode-skills/`**: Skill discovery, loading, and validation
- **`vtcode-safety/`**: Command safety detection and sandboxing
- **`vtcode-a2a/`**: Agent2Agent protocol client and server
- **`vtcode-mcp/`**: Model Context Protocol client and tool discovery
- **`crates/codegen/xtask/`**: Release packaging automation

### Key Directories

- **`docs/`**: Documentation files
- **`tests/`**: Integration and end-to-end tests
- **`vtcode.toml`**: Configuration file (never hardcode values, always read from config)
- **`crates/codegen/vtcode-core/src/config/constants.rs`**: Constants definition
- **`docs/models.json`**: Model IDs for providers

## Code Style

### Rust Conventions

- **Naming**: `snake_case` for functions/variables, `PascalCase` for types
- **Formatting**: Use 4 spaces (no tabs), `cargo fmt` for formatting
- **Error Handling**: Use `anyhow::Result<T>` with `.with_context()` for all fallible functions
- **Early Returns**: Prefer early returns over nested if statements
- **Variable Names**: Use descriptive variable names
- **No hardcoded values**: Always read from `vtcode.toml` or `crates/codegen/vtcode-core/src/config/constants.rs`
- **No emojis in code**: Maintain professional code style

### Documentation

- All public APIs should have Rustdoc documentation
- Complex logic should include explanatory comments
- Follow the existing documentation patterns in the codebase
- All .md files should be placed in `./docs/` directory (not in the root)

### Examples

**Good:**

```rust
/// Reads a file with proper error handling
pub async fn read_file_with_context(path: &str) -> anyhow::Result<String> {
    tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file: {}", path))
}
```

**Avoid:**

```rust
// Hardcoded values
let limit = 1000;

// Unclear variable names
let x = calculate_something(a, b, c);
```

## Testing

### Running Tests

```bash
# Run all tests (preferred — parallel, fast)
cargo nextest run

# Run specific test
cargo nextest run test_name

# Run single crate
cargo nextest run -p vtcode-core

# Fallback if nextest is not installed
cargo test --workspace
```

### Test Structure

- Unit tests: Inline with the code they test, in `#[cfg(test)]` modules
- Integration tests: In the `tests/` directory at workspace root
- Harness regressions: `cargo test -p vtcode-core --test pty_tests`
- Follow the Arrange-Act-Assert pattern
- Use descriptive test names that explain what is being tested

### Code Quality Checks

```bash
# Fast quality gate (10-30s, recommended for iteration)
./scripts/check-dev.sh

# Full quality gate (2-5m, for PRs)
./scripts/check.sh

# Narrow checks
cargo clippy && cargo fmt --check && cargo check
```

Both gates enforce the tracked-file length ceiling: no tracked text file may exceed 1000 lines
(`python3 scripts/check_file_length.py --mode enforce`). Generated or vendored outliers are parked in
`scripts/file_length_allowlist.txt` as `glob=MAX_LINES`; first-party source is never allowlisted without a split plan.
If your change pushes a file over the ceiling, split it instead of extending the allowlist.

## Submitting Changes

### Pull Request Process

1. **Fork the repository** and create your feature branch from `main`
2. **Make your changes** following the code style guidelines
3. **Add tests** for any new functionality
4. **Update documentation** if needed
5. **Run all tests** to ensure nothing is broken
6. **Commit your changes** with clear, descriptive commit messages
7. **Open a pull request** with a detailed description of your changes

### Commit Guidelines

- Use present tense ("Add feature" not "Added feature")
- Use imperative mood ("Move cursor to..." not "Moves cursor to...")
- Limit first line to 72 characters or less
- Reference issues and pull requests after the first line
- Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification if possible

### Pull Request Guidelines

- Provide a clear title and description
- Link to any relevant issues
- Explain the problem you're solving and how you solved it
- Include any relevant screenshots or examples (if applicable)
- Ensure all CI checks pass before requesting review

## Out of scope contributions

Some changes are closed without review. Knowing them up front saves you a rejected pull request.

### New MCP provider documentation

The project no longer accepts documentation or example configuration for new third-party MCP providers. This covers:

- Setup guides under `docs/guides/<vendor>-mcp.md`
- Runnable examples under `docs/examples/<vendor>-mcp.toml`
- New sections in [MCP Integration Guide](./guides/mcp-integration.md) for a specific provider
- Docs-map entries in `docs/modules/vtcode_docs_map.md` for the above

Vendor-authored documentation is indistinguishable from promotion, and per-vendor guides rot as endpoints, auth flows,
and tool catalogs change. We cannot keep verifying them, so we stopped adding them.

This does not affect the MCP protocol support itself. VT Code continues to implement MCP client behavior, and any
provider that speaks the protocol works through the generic `[[mcp.providers]]` configuration described in the
[MCP Integration Guide](./guides/mcp-integration.md). You do not need a VT Code docs entry to be usable.

If you maintain an MCP server, publish the setup guide on your own site and point at the generic configuration docs
here. That keeps the instructions in the hands of whoever can keep them current.

Changes are still welcome when they fix a factual error in the protocol surface, add a generic configuration capability
that any provider benefits from, or remove a provider guide that has gone stale.

This rule is enforced automatically. `scripts/check_mcp_provider_docs.py` runs in `./scripts/check-dev.sh` and fails when
a new `docs/guides/<vendor>-mcp.md` or `docs/examples/<vendor>-mcp.toml` appears. Guides that already exist stay
editable; only new provider identities are rejected. The check is deterministic rather than model-based so its verdict
does not depend on which tools or review plan a contributor has.

### Other out of scope changes

- New public Rust traits for third-party integrations. Prefer config, manifests, plugins, and MCP; see
  [Extension Boundaries](./development/extension_boundaries.md).
- Vendored copies of third-party code without an upstream link and licensing review.
- Drive-by formatting, dependency bumps unrelated to a fix, and renames with no behavior change.

## Development Guidelines

### Error Handling

Always use proper error handling with context:

```rust
use anyhow::{Context, Result};

pub async fn example_function(path: &str) -> Result<()> {
    let content = tokio::fs::read_to_string(path)
        .await
        .with_context(|| format!("Failed to read file at {}", path))?;

    // Process content...
    Ok(())
}
```

### Async Programming

- Use `#[tokio::main]` or `#[tokio::main(flavor = "multi_thread")]` for async main functions when needed
- Prefer async/await for I/O operations
- Use the multi-threaded flavor for CPU-intensive tasks

### Configuration

- Never hardcode values - always read from `vtcode.toml`
- Validate configuration values at runtime
- Use constants defined in `crates/codegen/vtcode-core/src/config/constants.rs`

### Security Practices

- Validate all user input and file paths
- Use safe methods for file operations
- Follow Rust's safety guarantees
- Implement proper permissions and sandboxing where applicable

## Architecture Overview

### Core Components

- **LLM Abstractions**: Provider traits with uniform async interfaces supporting OpenAI, Anthropic, Gemini, Meta AI,
  xAI, DeepSeek, Z.AI, Moonshot AI, OpenRouter, and Ollama
- **Modular Tools**: Internal trait-based composition for built-in tools, with MCP/manifests preferred for external
  extensions
- **Extension Boundaries**: Prefer config, manifests, plugins, and MCP over new public Rust traits for third-party
  integrations
- **Configuration Engine**: Deserializes `vtcode.toml` into validated structs
- **Context Engineering System**: Implements iterative, per-turn curation with token budgeting
- **Security & Safety**: Tree-sitter-bash for critical shell command validation and OS-native sandboxing
- **MCP Integration**: Model Context Protocol support for extensible tooling

### User Interface

- **Ratatui TUI**: Reactive terminal user interface
- **PTY Integration**: Real-time PTY for command streaming
- **Slash Commands**: Fuzzy-matched commands for various actions

## Community

VT Code is created and maintained by [@vinhnx](https://github.com/vinhnx), and it gets better every time someone
contributes - code, docs, a bug report, or a careful review. You do not need to be a Rust expert, know the harness, or
ship a big change to help. Curiosity is enough to start.

### Where to start

- [Good first issues](https://github.com/vinhnx/vtcode/labels/good%20first%20issue) - small, self-contained tasks with
  the relevant files and a way to verify your work.
- [Help wanted](https://github.com/vinhnx/vtcode/labels/help%20wanted) - bigger items where a second pair of hands
  genuinely helps.
- [GitHub Discussions](https://github.com/vinhnx/vtcode/discussions) - ask questions and talk through design. Asking
  before you build is always cheaper than reworking later.
- [GitHub Issues](https://github.com/vinhnx/vtcode/issues) - report bugs or suggest features. For bugs, include
  `vtcode trajectory` output, your OS, and the provider/model if you can.

### How a contribution lands

1. Comment on the issue and say what you plan to change. That way nobody duplicates work, and you get early input.
2. Keep the first pull request small. A failing test plus a fix is the ideal shape, and a tiny docs fix is just as
   welcome.
3. Run `./scripts/check-dev.sh` and the relevant `cargo nextest run` target, then paste the result in the PR.
4. Review will focus on tests, docs, and boundaries - not on your experience level. Disagreeing with reasoning is
   welcome too.
5. Once merged, your avatar joins the [README contributor grid](../README.md#thanks-to-our-contributors).

### Community standards

The [Code of Conduct](./CODE_OF_CONDUCT.md) covers every space here. Assume good intent, keep feedback about the code,
and help the next person the way you would have liked to be helped.

---

Thank you for contributing to VT Code and helping make it a better tool for developers!
