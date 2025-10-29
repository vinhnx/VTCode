# VT Code Architecture Guide

## Overview

VT Code follows a modular, trait-based architecture designed for maintainability, extensibility, and performance.

## Core Architecture

### Modular Tools System

```
tools/
├── mod.rs           # Module coordination & exports
├── traits.rs        # Core composability traits
├── types.rs         # Common types & structures
├── cache.rs         # Enhanced caching system
├── grep_file.rs     # Ripgrep-backed search manager
├── file_ops.rs      # File operations tool (async + cached)
├── command.rs       # Command execution tool (3 modes)
└── registry.rs      # Tool coordination & function declarations
```

### Core Traits

```rust
// Base tool interface
pub trait Tool: Send + Sync {
    async fn execute(&self, args: Value) -> Result<Value>;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
}

// Multi-mode execution
pub trait ModeTool: Tool {
    fn supported_modes(&self) -> Vec<&'static str>;
    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value>;
}

// Intelligent caching
pub trait CacheableTool: Tool {
    fn cache_key(&self, args: &Value) -> String;
    fn should_cache(&self, args: &Value) -> bool;
}
```

## Tool Implementations

### GrepSearchManager (`tools::grep_file`)

-   Debounce and cancellation pipeline for responsive searches
-   Ripgrep primary backend with perg fallback when unavailable
-   Supports glob filters, hidden file handling, and context lines
-   Enforces workspace boundaries with robust path validation

### Search Stack Consolidation

-   `grep_file.rs` is the single source of truth for content search
-   Higher-level helpers were removed; use `ToolRegistry::grep_file_executor`
-   AST-aware discovery continues to rely on `tools::ast_grep`
-   `list_files` remains a discovery/metadata tool; defer all content scanning to `grep_file`

### FileOpsTool

-   Workspace-scoped file listing, metadata inspection, and traversal
-   Async directory walking with cache integration for large trees
-   Path policy enforcement shared with the command subsystem

### CommandTool

-   Standard command execution with exit-code and output capture
-   PTY session management for interactive commands
-   Streaming support for long-lived shell tasks with cancellation

## Design Principles

1. **Trait-based Composability** - Tools implement multiple traits for different capabilities
2. **Mode-based Execution** - Single tools support multiple execution modes
3. **Backward Compatibility** - All existing APIs remain functional
4. **Performance Optimization** - Strategic caching and async operations
5. **Clear Separation** - Each module has single responsibility

## Adding New Tools

```rust
use super::traits::{Tool, ModeTool};
use async_trait::async_trait;

pub struct MyTool;

#[async_trait]
impl Tool for MyTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        // Implementation
    }

    fn name(&self) -> &'static str { "my_tool" }
    fn description(&self) -> &'static str { "My custom tool" }
}

#[async_trait]
impl ModeTool for MyTool {
    fn supported_modes(&self) -> Vec<&'static str> {
        vec!["mode1", "mode2"]
    }

    async fn execute_mode(&self, mode: &str, args: Value) -> Result<Value> {
        match mode {
            "mode1" => self.execute_mode1(args).await,
            "mode2" => self.execute_mode2(args).await,
            _ => Err(anyhow::anyhow!("Unsupported mode: {}", mode))
        }
    }
}
```

## Benefits

-   **77% complexity reduction** from monolithic structure
-   **Enhanced functionality** through mode-based execution
-   **100% backward compatibility** maintained
-   **Plugin-ready architecture** for external development
-   **Performance optimized** with intelligent caching

## Training & Evaluation Alignment

To operationalize the staged training paradigm introduced in `docs/research/kimi_dev_agentless_training.md`, VT Code couples its
modular runtime with a data and evaluation strategy designed for agentless skill priors:

-   **Dual Roles** – Prompt templates for `BugFixer` and `TestWriter` share the same tool registry, enabling deterministic skill
    acquisition before agentic orchestration.
-   **Execution Telemetry** – Command and sandbox outputs are captured through the existing `bash_runner` and PTY subsystems,
    allowing outcome-based rewards for RL without extra instrumentation.
-   **Self-Play Hooks** – The tool layer exposes high-signal search, diff, and patching capabilities that feed directly into the
    multi-rollout evaluation loop defined in the training roadmap.
-   **Context Capacity** – Long-context support in the LLM provider abstraction ensures that multi-turn reasoning traces and
    aggregated rollouts remain accessible during both SFT and inference.

See the research note for the full pipeline (mid-training data curation, SFT cold start, RL curriculum, and test-time self-play).
