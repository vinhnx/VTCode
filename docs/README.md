# VT Code Documentation Hub

Welcome to the comprehensive documentation for **VT Code**, a Rust-based terminal coding agent with modular architecture supporting multiple LLM providers and advanced code analysis capabilities.

## 🚀 Quick Installation

[![Crates.io](https://img.shields.io/crates/v/vtcode.svg)](https://crates.io/crates/vtcode)
[![Homebrew](https://img.shields.io/badge/dynamic/json?url=https://formulae.brew.sh/api/formula/vtcode.json&query=$.versions.stable&label=homebrew)](https://formulae.brew.sh/formula/vtcode)
[![GitHub release](https://img.shields.io/github/release/vinhnx/vtcode.svg)](https://github.com/vinhnx/vtcode/releases)

### Choose Your Installation Method

#### Cargo (Recommended)

```bash
cargo install vtcode
```

📚 [API Documentation](https://docs.rs/vtcode) | 📦 [Crates.io](https://crates.io/crates/vtcode)

#### Homebrew (macOS)

```bash
brew install vtcode
```

🍺 [Homebrew Formula](https://formulae.brew.sh/formula/vtcode)

#### Pre-built Binaries

Download from [GitHub Releases](https://github.com/vinhnx/vtcode/releases) for Linux, macOS, or Windows.

## What Makes VT Code Special

VT Code represents a modern approach to AI-powered software development, featuring:

-   **Single-Agent Reliability (default)** - Streamlined, linear agent with robust context engineering
-   **Decision Ledger** - Structured, compact record of key decisions injected each turn for consistency
-   **Error Recovery & Resilience** - Intelligent error handling with pattern detection and context preservation
-   **Conversation Summarization** - Automatic compression when exceeding thresholds with quality assessment
-   **Multi-Provider LLM Support** - Gemini, OpenAI, Anthropic, OpenRouter integration
-   **Advanced Code Intelligence** - Tree-sitter parsers for 6+ programming languages
-   **Enterprise-Grade Safety** - Comprehensive security controls and path validation
-   **Flexible Configuration** - TOML-based configuration with granular policies
-   **Workspace-First Execution** - Full read/write/command capabilities anchored to `WORKSPACE_DIR` with built-in indexing workflows

## Documentation Overview

This documentation is organized to support different user personas and use cases:

### Recent Major Enhancements

VT Code has undergone significant improvements inspired by Anthropic's agent architecture:

-   **Decision Transparency System** - Complete audit trail with reasoning and confidence scores
-   **Error Recovery & Resilience** - Intelligent error handling with pattern detection and multiple recovery strategies
-   **Conversation Summarization** - Automatic compression for extended sessions with quality metrics
-   **Enhanced Tool Design** - Comprehensive specifications with error-proofing and clear usage guidelines
-   **Configuration Synchronization** - Two-way sync between generated configs and existing settings

See [CHANGELOG](../CHANGELOG.md) for complete details on these improvements.

### For Users

New to VT Code? Start with installation and basic usage:

-   **[Getting Started](./user-guide/getting-started.md)** - Installation, configuration, and first steps
-   **[Interactive Mode Reference](./user-guide/interactive-mode.md)** - Keyboard shortcuts and terminal workflows
-   [Decision Ledger](./context/DECISION_LEDGER.md) - How decisions are tracked and injected
-   **[Configuration Guide](./CONFIGURATION.md)** - Comprehensive configuration options
-   **[Status Line Configuration](./guides/status-line.md)** - Customize the inline prompt footer

### For Developers

Contributing to VT Code? Understand the architecture and development processes:

-   **[Architecture Overview](./ARCHITECTURE.md)** - System design and core components
-   **[Development Guide](./development/README.md)** - Development environment setup
-   **[API Documentation](./api/README.md)** - Technical API references
-   **[Code Standards](./development/code-style.md)** - Coding guidelines and best practices
-   **[Codex Cloud Setup](./guides/codex-cloud-setup.md)** - Configure Codex Cloud environments for VT Code
-   **[Memory Management](./guides/memory-management.md)** - Tune VT Code's AGENTS.md hierarchy for reliable prompts

### For Organizations

Deploying VT Code in production? Focus on enterprise features:

-   **[Security Model](./SECURITY_MODEL.md)** - Multi-layered security architecture
-   **[Security Audit](./SECURITY_AUDIT.md)** - Vulnerability analysis and recommendations
-   **[Security Implementation](./SAFETY_IMPLEMENTATION.md)** - Security controls and compliance
-   **[Performance Analysis](./PERFORMANCE_ANALYSIS.md)** - Optimization and benchmarking
-   **[Provider Guides](./PROVIDER_GUIDES.md)** - LLM provider integration guides
    -   [OpenRouter Integration](./providers/openrouter.md)

## Core Capabilities

### Context Engineering

-   **Decision Ledger** - Persistent, compact history of key decisions and constraints
-   **Error Recovery** - Intelligent error handling with pattern detection and context preservation
-   **Smart Summarization (EXPERIMENTAL)** - Automatic conversation compression with importance scoring, semantic similarity detection, and advanced error analysis (disabled by default)
-   **Conversation Summarization** - Automatic compression for long sessions with quality scoring
-   **Context Compression** - Summarizes older turns while preserving ledger, errors, and recent activity
-   **Tool Traces** - Tool inputs/outputs summarized and fed back for continuity

### Advanced Code Intelligence

-   **Tree-Sitter Integration** - Syntax-aware parsing for Rust, Python, JavaScript, TypeScript, Go, Java
-   **Intelligent Search** - Ripgrep and AST-grep powered code analysis
-   **Fuzzy File Discovery** - Git-aware traversal using `ignore` with `nucleo-matcher` scoring
-   **Symbol Analysis** - Function, class, and variable extraction
-   **Dependency Mapping** - Import relationship analysis
-   **Code Quality Assessment** - Complexity and maintainability scoring

### Comprehensive Tool Suite

-   **File Operations** - Safe, validated file system operations
-   **Terminal Integration** - Enhanced PTY support with color-coded tool/MCP status banners and interactive commands
-   **Search & Analysis** - Fast text and syntax-aware code search
-   **Batch Processing** - Efficient multi-file operations
-   **Configuration Management** - Dynamic TOML-based settings

### Safety & Security

VT Code implements a **multi-layered security model** to protect against prompt injection and argument injection attacks:

-   **Execution Policy** - Command allowlist with per-command argument validation (only 9 safe commands allowed)
-   **Argument Injection Protection** - Explicit blocking of dangerous flags (e.g., `--pre`, `-exec`, `-e`)
-   **Workspace Isolation** - All operations confined to workspace boundaries with symlink resolution
-   **Sandbox Integration** - Optional Anthropic sandbox runtime for network commands
-   **Human-in-the-Loop** - Three-tier approval system (once/session/permanent)
-   **Path Validation** - Prevents access outside workspace with comprehensive traversal checks
-   **Command Policies** - Allow/deny lists with pattern matching
-   **Audit Trail** - Comprehensive logging of all command executions
-   **File Size Limits** - Configurable resource constraints
-   **API Key Security** - Secure credential management

**[Security Quick Reference](./SECURITY_QUICK_REFERENCE.md)** - Security at a glance
**[Security Model](./SECURITY_MODEL.md)** - Complete security architecture
**[Security Audit](./SECURITY_AUDIT.md)** - Vulnerability analysis and testing
**[Security Guide](./guides/security.md)** - Best practices and configuration
**[Tool Policies](./vtcode_tools_policy.md)** - Command execution policies

## Quick Start Guide

### For New Users

1. **[Installation](../README.md#installation)** - Get VT Code running in minutes
2. **[Basic Configuration](./CONFIGURATION.md)** - Set up your environment
3. **[First Chat Session](../README.md#basic-usage)** - Try interactive coding assistance

### For Developers

1. **[Architecture Overview](./ARCHITECTURE.md)** - Understand the system design
2. **[Development Setup](./development/README.md)** - Configure development environment
3. **[Decision Ledger](./context/DECISION_LEDGER.md)** - Learn decision tracking and context engineering

### For Organizations

1. **[Security Implementation](./SAFETY_IMPLEMENTATION.md)** - Enterprise security features
2. **[Provider Integration](./PROVIDER_GUIDES.md)** - LLM provider setup (Gemini, OpenAI, Anthropic, OpenRouter)
3. **[Performance Tuning](./PERFORMANCE_ANALYSIS.md)** - Optimization strategies

## Usage Patterns

### Usage Notes

**LLM Routing:**
To enable LLM routing: set `[router] llm_router_model = "<model-id>"`.

**Budget Tuning:**
To tune budgets: add `[router.budgets.<class>]` with max_tokens and max_parallel_tools.

**Trajectory Logs:**
Logs for trajectory: check `.vtcode/logs/trajectory.jsonl`.

### Workspace-First Operations

-   `WORKSPACE_DIR` always points to the active project root; treat it as the default scope for every command and edit.
-   Use targeted indexing (directory walks, dependency introspection, metadata extraction) before large changes to stay aligned with the current codebase.
-   Keep shell commands and scripts within the workspace unless the workflow explicitly requires external paths.
-   Ask for confirmation before operating outside `WORKSPACE_DIR` or when interacting with untrusted downloads.
-   Launch sessions against another repository with `vtcode /abs/path`; you can also pass `--workspace-dir` (alias: `--workspace`) to other commands when needed.

### Single-Agent Workflows

```bash
# Complex task execution with Decision Ledger
./run.sh chat "Implement user authentication system"

# Codebase analysis
./run.sh analyze
```

### Configuration Management

```bash
# Initialize project configuration
./run.sh init

# Generate complete configuration (preserves existing settings)
./run.sh config

# Generate configuration to custom file
./run.sh config --output my-config.toml

# Edit configuration interactively
./run.sh config --edit

# Validate configuration
./run.sh config --validate
```

**Smart Configuration Generation**: The `config` command implements two-way synchronization that reads your existing `vtcode.toml` and generates a complete template while preserving all your customizations.

## Benchmarks & Performance

VT Code is evaluated on industry-standard benchmarks to measure code generation quality and performance:

### HumanEval Results

**Latest:** `gemini-2.5-flash-lite` achieves **61.6% pass@1** on the complete HumanEval dataset (164 tasks)

| Metric         | Value             |
| -------------- | ----------------- |
| Pass@1         | 61.6%             |
| Median Latency | 0.973s            |
| P90 Latency    | 1.363s            |
| Cost           | $0.00 (free tier) |

**[View Full Benchmark Results](./benchmarks/HUMANEVAL_2025-10-22.md)**
📈 **[Model Comparison](./benchmarks/COMPARISON.md)**
🔧 **[Run Your Own Benchmarks](./benchmarks/README.md)**

### Running Benchmarks

```bash
# Run full HumanEval benchmark
make bench-humaneval PROVIDER=gemini MODEL='gemini-2.5-flash-lite'

# Compare multiple models
python3 scripts/compare_benchmarks.py reports/HE_*.json

# Generate visualizations
python3 scripts/generate_benchmark_chart.py reports/HE_*.json --png
```

## Testing & Quality Assurance

VT Code includes comprehensive testing infrastructure:

### Test Categories

-   **Unit Tests** - Component-level testing with `cargo nextest run`
-   **Integration Tests** - End-to-end workflow validation
-   **Performance Tests** - Benchmarking with `cargo bench`
-   **Configuration Tests** - TOML validation and policy testing
-   **Code Generation Benchmarks** - HumanEval and other standard benchmarks

### Quality Assurance

```bash
# Run full test suite
cargo nextest run --workspace

# Run with coverage
cargo tarpaulin

# Performance benchmarking
cargo bench

# Code generation benchmarks
make bench-humaneval

# Linting and formatting
cargo clippy && cargo fmt
```

## Project Information

### Current Status & Roadmap

-   **[Roadmap](../ROADMAP.md)** - Future development plans and milestones
-   **[Changelog](../CHANGELOG.md)** - Version history and release notes
-   **[TODO](./project/TODO.md)** - Current development tasks

### Development Resources

-   **[Contributing Guide](../CONTRIBUTING.md)** - How to contribute
-   **[Code Standards](./development/code-style.md)** - Coding guidelines
-   **[Architecture Decisions](./ARCHITECTURE.md)** - Design rationale

## Support & Community

### Getting Help

-   **GitHub Issues** - Report bugs and request features
-   **GitHub Discussions** - Community discussions and support
-   **Documentation** - Comprehensive guides and tutorials

### Community Resources

-   **[Main README](../README.md)** - Project overview and quick reference
-   **[GitHub Repository](https://github.com/vinhnx/vtcode)** - Source code and collaboration
-   **[Discussions](https://github.com/vinhnx/vtcode/discussions)** - Community support

### Enterprise Support

-   **Security Features** - Enterprise-grade security controls
-   **Single-Agent Coordination** - Reliable workflow orchestration with Decision Ledger
-   **Provider Integration** - Multiple LLM provider support
-   **Performance Optimization** - Enterprise-scale performance tuning

## License & Attribution

This documentation is part of the VT Code project. See the main [README](../README.md) for license information.

### Attribution

VT Code builds upon key developments in AI agent technology:

-   **Anthropic's Agent Patterns** - Tool design and safety principles
-   **Cognition's Context Engineering** - Long-running agent reliability and Decision Ledger
-   **Single-Agent Architecture** - Reliable coordination patterns
-   **Tree-Sitter Ecosystem** - Advanced code parsing capabilities
-   **Rust Community** - High-performance systems programming

---

**Documentation Version:** 2.1.0
**Last Updated:** September 2025
**VT Code Version:** 0.15.9

**Ready to get started?** **[Installation Guide](../README.md#quick-start)**

## Documentation Version

This documentation reflects version 0.15.9 of VT Code, which includes significant enhancements including:

-   Decision Ledger system for transparent decision tracking
-   Error recovery and resilience with intelligent pattern detection
-   Conversation summarization for long-running sessions
-   Enhanced Terminal User Interface (TUI) with improved mouse support and text selection
-   Two-way configuration synchronization
