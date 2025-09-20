//! # VTCode - Terminal Coding Agent
//!
//! VTCode is a Rust-based terminal coding agent with modular architecture
//! supporting multiple LLM providers (Gemini, OpenAI, Anthropic, DeepSeek) and tree-sitter
//! parsers for 6+ languages.
//!
//! ## Features
//!
//! - **Single-Agent Reliability**: Streamlined, linear agent with robust context engineering
//! - **Decision Ledger**: Structured record of key decisions injected each turn for consistency
//! - **Multi-Provider LLM Support**: Gemini, OpenAI, Anthropic, DeepSeek integration
//! - **Advanced Code Analysis**: Tree-sitter parsers for Rust, Python, JavaScript, TypeScript, Go, Java
//! - **Intelligent Tool Suite**: File operations, search, terminal commands, and PTY integration
//! - **Configuration Management**: TOML-based configuration with comprehensive policies
//! - **Safety & Security**: Path validation, command policies, and human-in-the-loop controls
//! - **Workspace-First Automation**: Reads, writes, indexing, and shell execution anchored to `WORKSPACE_DIR`
//!
//! ## Quick Start
//!
//! ```bash
//! # Install VTCode
//! cargo install vtcode
//!
//! # Set your API key
//! export GEMINI_API_KEY=your_api_key_here
//!
//! # Initialize in your project
//! vtcode init
//!
//! # Start interactive chat
//! vtcode chat
//! ```
//!
//! ## Architecture
//!
//! VTCode follows proven patterns for reliable, long-running coding assistance:
//!
//! - **Model-Driven Control**: Maximum autonomy given to language models
//! - **Decision Transparency**: Complete audit trail of all agent actions
//! - **Error Recovery**: Intelligent error handling with context preservation
//! - **Conversation Summarization**: Automatic compression for long sessions
//! - **Tool Integration**: Modular tool system with policy-based execution
//! - **Performance Monitoring**: Real-time metrics and benchmarking capabilities
//!
//! ## Distribution
//!
//! VTCode is available through multiple channels:
//!
//! - **Cargo**: `cargo install vtcode`
//! - **npm**: `npm install -g vtcode`
//! - **Homebrew**: `brew install vinhnx/tap/vtcode`
//! - **GitHub Releases**: Pre-built binaries for all platforms
//!
//! ## Documentation
//!
//! - [User Guide](https://github.com/vinhnx/vtcode/tree/main/docs)
//! - [API Documentation](https://docs.rs/vtcode)
//! - [Configuration Guide](https://github.com/vinhnx/vtcode/blob/main/docs/project/CONFIGURATION.md)
//!
//! ## Examples
//!
//! ### Basic Usage
//!
//! ```rust,ignore
//! use vtcode_core::{Agent, VTCodeConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load configuration
//!     let config = VTCodeConfig::load()?;
//!
//!     // Create agent
//!     let agent = Agent::new(config).await?;
//!
//!     // Start interactive session
//!     agent.run().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ### Custom Tool Integration
//!
//! ```rust,ignore
//! use vtcode_core::tools::{ToolRegistry, ToolRegistration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut registry = ToolRegistry::new(std::env::current_dir()?);
//!
//!     // Register a custom tool
//!     let custom_tool = ToolRegistration {
//!         name: "my_custom_tool".to_string(),
//!         description: "A custom tool for specific tasks".to_string(),
//!         parameters: serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "input": {"type": "string"}
//!             }
//!         }),
//!         handler: |args| async move {
//!             // Tool implementation
//!             Ok(serde_json::json!({"result": "success"}))
//!         },
//!     };
//!
//!     registry.register_tool(custom_tool).await?;
//!
//!     Ok(())
//! }
//! ```

//! VTCode binary package
//!
//! This package contains the binary executable for VTCode.
//! For the core library functionality, see [`vtcode-core`](https://docs.rs/vtcode-core).
