# vtcode-core

Core library for VT Code — a Rust-based terminal coding agent.

`vtcode-core` powers the VT Code agent runtime. It provides the reusable
building blocks for multi-provider LLM orchestration, tool execution,
semantic code analysis, and configurable safety policies.

## Highlights

- **Provider Abstraction** — unified LLM interface with adapters for OpenAI, Anthropic, xAI, DeepSeek, Gemini, OpenRouter, and Ollama, including automatic failover and spend controls.
- **Prompt Caching** — cross-provider caching system that leverages provider-specific mechanisms (OpenAI automatic, Anthropic `cache_control`, Gemini implicit/explicit) to reduce cost and latency.
- **Semantic Workspace Model** — LLM-native code analysis and navigation across modern programming languages.
- **Bash Shell Safety** — `tree-sitter-bash` integration for critical command validation and security enforcement.
- **Tool System** — trait-driven registry for shell execution, file IO, search, and custom commands, with Tokio-powered concurrency and PTY streaming.
- **Configuration-First** — everything is driven by `vtcode.toml`, with model, safety, and automation constants centralized in `config::constants`.
- **Safety & Observability** — workspace boundary enforcement, command allow/deny lists, human-in-the-loop confirmation, and structured event logging.

## Modules

| Module | Purpose |
|---|---|
| `config/` | Configuration loader, defaults, schema validation |
| `llm/` | Provider clients, request shaping, response handling |
| `tools/` | Built-in tool implementations and registration utilities |
| `context/` | Conversation management and memory |
| `exec/` | Async orchestration for tool invocations and streaming output |
| `core/prompt_caching` | Cross-provider prompt caching system |
| `mcp/` | Model Context Protocol client support |
| `safety/` | Workspace boundary enforcement and command safety |

## Public entrypoints

- `Agent` / `AgentRunner` — main agent runtime
- `VTCodeConfig` — configuration loader (`vtcode.toml` + environment overrides)
- `ToolRegistry` / `OptimizedToolRegistry` — tool registration and execution
- `AnyClient` / `make_client` — provider-agnostic LLM client factory
- `PromptCache` / `PromptOptimizer` — prompt caching primitives
- `ThreadManager` — thread lifecycle and event recording

## Usage

```rust,ignore
use vtcode_core::{Agent, VTCodeConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = VTCodeConfig::load()?;
    let agent = Agent::new(config).await?;
    agent.run().await?;
    Ok(())
}
```

## Feature flags

| Flag | Description |
|---|---|
| `tui` (default) | Terminal UI via crossterm |
| `schema` | JSON Schema generation via `schemars` |
| `a2a-server` | Agent2Agent Protocol HTTP server |
| `anthropic-api` | Anthropic-compatible API server |
| `desktop-notifications` | Desktop notification support |

## API reference

See [docs.rs/vtcode-core](https://docs.rs/vtcode-core).

## Related docs

- [Architecture overview](../docs/ARCHITECTURE.md)
