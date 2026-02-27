# VT Code Architecture Guide

## Overview

VT Code follows a modular, trait-based architecture designed for maintainability, extensibility, and performance.

### CLI Architecture

The command-line interface is built on specific principles for robustness and interoperability:

1.  **Strict Output Separation**: Data goes to `stdout`, diagnostics/logs go to `stderr`. This enables clean piping of machine-readable output.
2.  **Standard Argument Parsing**: Uses `clap` for POSIX/GNU compliance, supporting standard flags and behavior.
3.  **Command Isolation**: Each sub-command (`ask`, `exec`, `chat`) is handled by a dedicated module in `src/cli/`, sharing core logic via `vtcode-core`.
4.  **Signal Handling**: Graceful handling of `SIGINT`/`SIGTERM` to ensure resource cleanup (e.g., restoring terminal state).

## Core Architecture

### TUI Architecture

The terminal UI now has a dedicated crate boundary:

- `vtcode-tui`: public TUI-facing API surface for downstream consumers
- `vtcode-core::ui::tui`: canonical runtime type surface for VT Code internals

This separation allows external code to import TUI types and session APIs from
`vtcode-tui` while keeping host-specific integrations inside `vtcode-core`.

`vtcode-tui` now exposes standalone launch primitives (`SessionOptions`,
`SessionSurface`, `KeyboardProtocolSettings`) plus host adapters
(`host::HostAdapter`) so downstream projects can start sessions without
importing `vtcode_core::config` types directly.

The full TUI source tree is now located in:

- `vtcode-tui/src/core_tui/`

`vtcode-core/src/ui/tui.rs` is a compatibility shim that compiles this migrated
source tree to preserve existing `vtcode_core::ui::tui` paths.

The TUI runner is organized into focused modules:

```
vtcode-tui/src/core_tui/runner/
 mod.rs           # Orchestration entrypoint (`run_tui`)
 drive.rs         # Main terminal/event loop drive logic
 events.rs        # Async event stream + tick scheduling
 signal.rs        # SIGINT/SIGTERM cleanup guard
 surface.rs       # Inline vs alternate screen detection
 terminal_io.rs   # Cursor/screen prep + drain helpers
 terminal_modes.rs# Raw/mouse/focus/keyboard mode management
```

### Modular Tools System

```
tools/
 mod.rs           # Module coordination & exports
 traits.rs        # Core composability traits
 types.rs         # Common types & structures
 cache.rs         # Enhanced caching system
 grep_file.rs     # Ripgrep-backed search manager
 file_ops.rs      # File operations tool (async + cached)
 command.rs       # Command execution tool (3 modes)
 registry.rs      # Tool coordination & function declarations
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

## RL Optimization Loop (Adaptive Action Selection)

-   **Module:** `vtcode-core/src/llm/rl` (bandit and actor-critic implementations)
-   **Config:** `[optimization]` with `strategy = "bandit" | "actor_critic"` plus reward shaping knobs
-   **Signals:** Success/timeout + latency feed `RewardSignal`, stored in a rolling `RewardLedger`
-   **Usage:** Construct `RlEngine::from_config(&VTCodeConfig::optimization)` and call `select(actions, PolicyContext)`; on completion emit `apply_reward`
-   **Goal:** Prefer low-latency, high-success actions (e.g., choose edge vs cloud executors) while remaining pluggable for future policies

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
