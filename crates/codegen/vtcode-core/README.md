# vtcode-core

Core library for VT Code — a Rust-based terminal coding agent.

`vtcode-core` powers the VT Code agent runtime. It provides the
reusable building blocks for multi-provider LLM orchestration, tool
execution, semantic code analysis, and configurable safety policies.

## Highlights

- **Provider Abstraction** — unified LLM interface with adapters for OpenAI,
  Anthropic, xAI, DeepSeek, Gemini, OpenRouter, and Ollama (local), including automatic
  failover and spend controls.
- **Prompt Caching** — cross-provider caching system that leverages
  provider-specific caching capabilities (OpenAI's automatic caching, Anthropic's
  `cache_control` blocks, Gemini's implicit/explicit caching) to reduce costs and
  latency, with configurable settings per provider.
- **Semantic Workspace Model** — LLM-native code analysis and navigation
  across all modern programming languages.
- **Bash Shell Safety** — `tree-sitter-bash` integration for critical command validation
  and security enforcement.
- **Tool System** — trait-driven registry for shell execution, file IO,
  search, and custom commands, with Tokio-powered concurrency and PTY
  streaming.
- **Ownership-First Tool Bridging** — prefer native CGP wrappers and borrowed tool references; use `Arc<dyn Tool>` bridges only when tools genuinely need shared ownership.
- **Configuration-First** — everything is driven by `vtcode.toml`, with
  model, safety, and automation constants centralized in `config::constants`
  and curated metadata in `docs/models.json`.
- **Safety & Observability** — workspace boundary enforcement, command
  allow/deny lists, human-in-the-loop confirmation, and structured event
  logging for comprehensive audit trails.

<!-- cargo-rdme start -->

### vtcode-core - Runtime for VT Code

`vtcode-core` powers the VT Code terminal coding agent. It provides the
reusable building blocks for multi-provider LLM orchestration, tool
execution, semantic code analysis, and configurable safety policies.

#### Highlights

- **Provider Abstraction**: unified LLM interface with adapters for OpenAI,
  Anthropic, xAI, DeepSeek, Gemini, OpenRouter, and Ollama (local), including automatic
  failover and spend controls.
- **Prompt Caching**: cross-provider prompt caching system that leverages
  provider-specific caching capabilities (OpenAI's automatic caching, Anthropic's
  cache_control blocks, Gemini's implicit/explicit caching) to reduce costs and
  latency, with configurable settings per provider.
- **Semantic Workspace Model**: LLM-native code analysis and navigation
  across all modern programming languages.
- **Bash Shell Safety**: tree-sitter-bash integration for critical command validation
  and security enforcement.
- **Tool System**: trait-driven registry for shell execution, file IO,
  search, and custom commands, with Tokio-powered concurrency and PTY
  streaming.
- **Configuration-First**: everything is driven by `vtcode.toml`, with
  model, safety, and automation constants centralized in
  `config::constants` and curated metadata in `docs/models.json`.
- **Safety & Observability**: workspace boundary enforcement, command
  allow/deny lists, human-in-the-loop confirmation, and structured event
  logging for comprehensive audit trails.

#### Architecture Overview

The crate is organized into several key modules:

- `config/`: configuration loader, defaults, and schema validation.
- `llm/`: provider clients, request shaping, and response handling.
- `tools/`: built-in tool implementations plus registration utilities.
- `context/`: conversation management and memory.
- `executor/`: async orchestration for tool invocations and streaming output.
- `core/prompt_caching`: cross-provider prompt caching system that leverages
  provider-specific caching mechanisms for cost optimization and reduced latency.

#### Quickstart

```rust
use vtcode_core::{Agent, VTCodeConfig};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Load configuration from vtcode.toml or environment overrides
    let config = VTCodeConfig::load()?;

    // Construct the agent runtime
    let agent = Agent::new(config).await?;

    // Execute an interactive session
    agent.run().await?;

    Ok(())
}
```

#### Extending VT Code

Register custom tools or providers by composing the existing traits:

```rust
use vtcode_core::tools::{ToolRegistry, ToolRegistration};

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let workspace = std::env::current_dir()?;
    let mut registry = ToolRegistry::new(workspace);

    let custom_tool = ToolRegistration {
        name: "my_custom_tool".into(),
        description: "A custom tool for specific tasks".into(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": { "input": { "type": "string" } }
        }),
        handler: |_args| async move {
            // Implement your tool behavior here
            Ok(serde_json::json!({ "result": "success" }))
        },
    };

    registry.register_tool(custom_tool).await?;
    Ok(())
}
```

For a complete tour of modules and extension points, read
`docs/ARCHITECTURE.md` and the guides in `docs/project/`.

#### Agent Client Protocol (ACP)

VT Code's binary exposes an ACP bridge for Zed. Enable it via the `[acp]` section in
`vtcode.toml`, launch the `vtcode acp` subcommand, and register the binary under
`agent_servers` in Zed's `settings.json`. Detailed instructions and troubleshooting live in the
[Zed ACP integration guide](https://github.com/vinhnx/vtcode/blob/main/docs/guides/zed-acp.md),
with a rendered summary on
[docs.rs](https://docs.rs/vtcode/latest/vtcode/#agent-client-protocol-acp).
##### Bridge guarantees

- Tool exposure follows capability negotiation: `read_file` stays disabled unless Zed
  advertises `fs.read_text_file`.
- Each filesystem request invokes `session/request_permission`, ensuring explicit approval
  within the editor before data flows.
- Cancellation signals propagate into VT Code, cancelling active tool calls and ending the
  turn with `StopReason::Cancelled`.
- ACP `plan` entries track analysis, context gathering, and response drafting for timeline
  parity with Zed.
- Absolute-path checks guard every `read_file` argument before forwarding it to the client.
- Non-tool-capable models trigger reasoning notices and an automatic downgrade to plain
  completions without losing plan consistency.

VT Code Core Library

This crate provides the core functionality for the VT Code agent,
including tool implementations, LLM integration, and utility functions.

<!-- cargo-rdme end -->

## Architecture Overview

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
