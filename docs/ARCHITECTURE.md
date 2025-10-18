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
├── search.rs        # Unified search tool (4 modes)
├── file_ops.rs      # File operations tool (4 modes)
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

### SearchTool (4 modes)

-   `exact`: Exact string matching
-   `fuzzy`: Fuzzy string matching
-   `multi-pattern`: Multiple pattern search
-   `similarity`: Semantic similarity search

### FileOpsTool (4 modes)

-   `list`: Basic directory listing
-   `recursive`: Recursive directory traversal
-   `find_name`: Find files by name pattern
-   `find_content`: Find files by content

### CommandTool (3 modes)

-   `terminal`: Standard command execution
-   `pty`: Pseudo-terminal execution
-   `streaming`: Real-time output streaming

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
