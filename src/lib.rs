//! # VT Code - Terminal Coding Agent
//!
//! VT Code is a Rust-based terminal coding agent that pairs a streamlined
//! crossterm-powered interface with semantic code understanding backed by tree-sitter and
//! ast-grep. It is designed for developers who need precise context handling,
//! secure tool execution, and configurable multi-provider AI workflows.
//!
//! ## Highlights
//!
//! - **Multi-provider agent**: integrations for OpenAI, Anthropic, xAI,
//!   DeepSeek, Gemini, OpenRouter, and Ollama (local) with automatic failover and spend guards.
//! - **Semantic code intelligence**: tree-sitter parsers for Rust, Python,
//!   JavaScript, TypeScript, Go, and Java combined with ast-grep structural
//!   search and refactoring.
//! - **Modern terminal experience**: inline renderer with streaming PTY output,
//!   slash commands, and customizable Ciapre-inspired theming.
//! - **Workspace-aware automation**: git-aware fuzzy navigation, workspace
//!   boundary enforcement, command allowlists, and human-in-the-loop
//!   confirmation.
//! - **Config-driven behavior**: every agent control lives in `vtcode.toml`,
//!   anchored by constants in `vtcode_core::config::constants` and curated model
//!   metadata in `docs/models.json`.
//!
//! ## Quickstart
//!
//! ```bash
//! # Install the CLI (cargo, npm, or Homebrew are also supported)
//! cargo install vtcode
//!
//! # Export the API key for your provider
//! export OPENAI_API_KEY="your-key"
//!
//! # Launch the agent with explicit provider/model overrides
//! vtcode --provider openai --model gpt-5-codex
//!
//! # Run a one-off prompt with streaming output
//! vtcode ask "Summarize diagnostics in src/lib.rs"
//!
//! # Perform a dry run without tool execution
//! vtcode --no-tools ask "Review recent changes in src/main.rs"
//! ```
//!
//! Persist long-lived defaults in `vtcode.toml` instead of hardcoding them:
//!
//! ```toml
//! [agent]
//! provider = "openai"
//! default_model = "gpt-5-codex"
//! ```
//!
//! The configuration loader resolves aliases through
//! `vtcode_core::config::constants`, while `docs/models.json` tracks the latest
//! vetted provider model identifiers.
//!
//! ## Architecture Overview
//!
//! VT Code separates reusable library components from the CLI entrypoint:
//!
//! - `vtcode-core/` exposes the agent runtime, provider abstractions (`llm/`),
//!   tool registry (`tools/`), configuration loaders, and tree-sitter
//!   integrations orchestrated with Tokio.
//! - `src/main.rs` embeds the inline UI, Clap-based CLI, and runtime wiring.
//! - MCP (Model Context Protocol) tools provide contextual resources (e.g.
//!   Serena MCP for journaling and memory), with policies expressed entirely in
//!   configuration.
//! - Safety features include workspace boundary enforcement, rate limiting,
//!   telemetry controls, and confirm-to-run guardrails.
//!
//! Additional implementation details live in `docs/ARCHITECTURE.md` and the
//! guides under `docs/project/`.
//!
//! ## Distribution Channels
//!
//! VT Code is distributed via multiple ecosystems:
//!
//! - **crates.io**: `cargo install vtcode`
//! - **npm**: `npm install -g vtcode`
//! - **Homebrew**: `brew install vinhnx/tap/vtcode`
//! - **GitHub Releases**: pre-built binaries for macOS, Linux, and Windows
//!
//! ## crates.io Categories
//!
//! This crate is listed under the following crates.io categories to improve
//! discoverability for terminal-focused developer tooling:
//!
//! - [`#development-tools`](https://crates.io/categories/development-tools)
//! - [`#command-line-utilities`](https://crates.io/categories/command-line-utilities)
//!
//! ## Library Usage Examples
//!
//! ### Starting an Agent Programmatically
//!
//! ```rust,ignore
//! use vtcode_core::{Agent, VTCodeConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), anyhow::Error> {
//!     let config = VTCodeConfig::load()?;
//!     let agent = Agent::new(config).await?;
//!     agent.run().await?;
//!     Ok(())
//! }
//! ```
//!
//! ### Registering a Custom Tool
//!
//! ```rust,ignore
//! use vtcode_core::tools::{ToolRegistry, ToolRegistration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), anyhow::Error> {
//!     let workspace = std::env::current_dir()?;
//!     let mut registry = ToolRegistry::new(workspace);
//!
//!     let custom_tool = ToolRegistration {
//!         name: "my_custom_tool".into(),
//!         description: "A custom tool for specific tasks".into(),
//!         parameters: serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "input": {"type": "string"}
//!             }
//!         }),
//!         handler: |args| async move {
//!             // Tool implementation goes here
//!             Ok(serde_json::json!({"result": "success"}))
//!         },
//!     };
//!
//!     registry.register_tool(custom_tool).await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Agent Client Protocol (ACP)
//!
//! VT Code ships an ACP bridge tailored for Zed. Enable it via the `[acp]` section in
//! `vtcode.toml`, launch `vtcode acp`, and register the binary inside Zed's
//! `agent_servers` list. The full walkthrough lives in the
//! [Zed ACP integration guide](https://github.com/vinhnx/vtcode/blob/main/docs/guides/zed-acp.md),
//! and the rendered API guarantees appear on
//! [docs.rs](https://docs.rs/vtcode/latest/vtcode/#agent-client-protocol-acp).
//!
//! ### Bridge guarantees
//!
//! - Filesystem tooling stays disabled unless Zed advertises `fs.read_text_file`, so the agent
//!   never emits unsupported requests.
//! - Each `read_file` call is wrapped in `session/request_permission`, letting you approve or
//!   deny access to sensitive paths before any data leaves the editor.
//! - Cancellation signals from Zed halt streaming, mark pending tools as cancelled, and end the
//!   turn with `StopReason::Cancelled` for clean transcripts.
//! - ACP `plan` updates broadcast analysis, optional context gathering, and response drafting
//!   progress so Zed mirrors the agent's workflow.
//! - Strict absolute-path validation blocks relative or out-of-workspace arguments before they
//!   reach the client.
//! - When the model lacks tool calling, VT Code emits reasoning notices and downgrades to plain
//!   completions while keeping the plan timeline consistent.

//!
//! VT Code binary package
//!
//! This package contains the binary executable for VT Code.
//! For the core library functionality, see [`vtcode-core`](https://docs.rs/vtcode-core).

pub mod acp;
mod workspace_trust;

pub mod startup;

pub use startup::StartupContext;
