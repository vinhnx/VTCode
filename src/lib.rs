//! # VT Code - Terminal Coding Agent
//!
//! VT Code is a Rust-based terminal coding agent that pairs a streamlined
//! termion-powered interface with semantic code understanding backed by tree-sitter and
//! ast-grep. It is designed for developers who need precise context handling,
//! secure tool execution, and configurable multi-provider AI workflows.
//!
//! ## Highlights
//!
//! - **Multi-provider agent**: integrations for OpenAI, Anthropic, xAI,
//!   DeepSeek, Gemini, and OpenRouter with automatic failover and spend guards.
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
//! VT Code binary package
//!
//! This package contains the binary executable for VT Code.
//! For the core library functionality, see [`vtcode-core`](https://docs.rs/vtcode-core).
