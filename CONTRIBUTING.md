# Contributing to VT Code

Welcome to VT Code! We're excited that you're interested in contributing to this Rust-based terminal coding agent. This document outlines the guidelines and best practices for contributing to the project.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure) 
- [Code Style](#code-style)
- [Testing](#testing)
- [Submitting Changes](#submitting-changes)
- [Development Guidelines](#development-guidelines)
- [Architecture Overview](#architecture-overview)
- [Community](#community)

## Getting Started

VT Code is a Rust-based terminal coding agent with semantic code intelligence via Tree-sitter (parsers for Rust, Python, JavaScript/TypeScript, Go, Java) and ast-grep (structural pattern matching and refactoring). It supports multiple LLM providers with automatic failover, prompt caching, and token-efficient context management.

Before contributing, please familiarize yourself with:

1. The [README.md](README.md) for an overview of the project
2. The [Architecture documentation](docs/ARCHITECTURE.md) for understanding the system design
3. The [Development Guide](docs/development/README.md) for detailed development processes

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

The project is organized into two main components:

- **`vtcode-core/`**: Reusable library code (LLM providers, tools, config, MCP integration)
- **`src/`**: CLI executable (Ratatui TUI, PTY execution, slash commands)
- **`vtcode-acp-client/`**: Agent Client Protocol client for editor integration
- **`vtcode-commons/`, `vtcode-config/`, `vtcode-llm/`, `vtcode-tools/`**: Modular crates for component extraction

### Key Directories

- **`docs/`**: Documentation files
- **`tests/`**: Integration and end-to-end tests
- **`vtcode.toml`**: Configuration file (never hardcode values, always read from config)
- **`vtcode-core/src/config/constants.rs`**: Constants definition
- **`docs/models.json`**: Model IDs for providers

## Code Style

### Rust Conventions

- **Naming**: `snake_case` for functions/variables, `PascalCase` for types
- **Formatting**: Use 4 spaces (no tabs), `cargo fmt` for formatting
- **Error Handling**: Use `anyhow::Result<T>` with `.with_context()` for all fallible functions
- **Early Returns**: Prefer early returns over nested if statements
- **Variable Names**: Use descriptive variable names
- **No hardcoded values**: Always read from `vtcode.toml` or `vtcode-core/src/config/constants.rs`
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
# Run all tests
cargo test

# Run with nextest (preferred)
cargo nextest run

# Run specific test
cargo test test_name

# Run specific test with nextest
cargo nextest run test_name

# Run tests with debug output
cargo test -- --nocapture
```

### Test Structure

- Unit tests: Inline with the code they test, in `#[cfg(test)]` modules
- Integration tests: In the `tests/` directory
- Follow the Arrange-Act-Assert pattern
- Use descriptive test names that explain what is being tested

### Code Quality Checks

```bash
# Linting
cargo clippy

# Formatting
cargo fmt

# Check build
cargo check

# Run all checks (recommended before committing)
cargo clippy && cargo fmt --check && cargo check && cargo test
```

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
- Use constants defined in `vtcode-core/src/config/constants.rs`

### Security Practices

- Validate all user input and file paths
- Use safe methods for file operations
- Follow Rust's safety guarantees
- Implement proper permissions and sandboxing where applicable

## Architecture Overview

### Core Components

- **LLM Abstractions**: Provider traits with uniform async interfaces supporting OpenAI, Anthropic, Gemini, xAI, DeepSeek, Z.AI, Moonshot AI, OpenRouter, and Ollama
- **Modular Tools**: Trait-based extensibility for built-in and custom tools
- **Configuration Engine**: Deserializes `vtcode.toml` into validated structs
- **Context Engineering System**: Implements iterative, per-turn curation with token budgeting
- **Code Intelligence**: Tree-sitter integration for AST traversal and ast-grep for rule-based transforms
- **MCP Integration**: Model Context Protocol support for extensible tooling

### User Interface

- **Ratatui TUI**: Reactive terminal user interface
- **PTY Integration**: Real-time PTY for command streaming
- **Slash Commands**: Fuzzy-matched commands for various actions

## Community

### Need Help?

- **GitHub Issues**: Report bugs and request features at [GitHub Issues](https://github.com/vinhnx/vtcode/issues)
- **Discussions**: Ask questions and discuss development in [GitHub Discussions](https://github.com/vinhnx/vtcode/discussions)

### Questions?

If you have questions about contributing or need clarification on any aspect of the project, feel free to open an issue or reach out through the appropriate channel. We're committed to helping new contributors get up to speed.

---

Thank you for contributing to VT Code and helping make it a better tool for developers!