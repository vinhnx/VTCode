//! # vtcode-core - Runtime for VT Code
//!
//! `vtcode-core` powers the VT Code terminal coding agent. It provides the
//! reusable building blocks for multi-provider LLM orchestration, tool
//! execution, semantic code analysis, and configurable safety policies.
//!
//! ## Highlights
//!
//! - **Provider Abstraction**: unified LLM interface with adapters for OpenAI,
//!   Anthropic, xAI, DeepSeek, Gemini, OpenRouter, and Ollama (local), including automatic
//!   failover and spend controls.
//! - **Prompt Caching**: cross-provider prompt caching system that leverages
//!   provider-specific caching capabilities (OpenAI's automatic caching, Anthropic's
//!   cache_control blocks, Gemini's implicit/explicit caching) to reduce costs and
//!   latency, with configurable settings per provider.
//! - **Semantic Workspace Model**: incremental tree-sitter parsing for Rust,
//!   Python, JavaScript, TypeScript, Go, and Java augmented by ast-grep based
//!   structural search and refactoring.
//! - **Tool System**: trait-driven registry for shell execution, file IO,
//!   search, and custom commands, with Tokio-powered concurrency and PTY
//!   streaming.
//! - **Configuration-First**: everything is driven by `vtcode.toml`, with
//!   model, safety, and automation constants centralized in
//!   `config::constants` and curated metadata in `docs/models.json`.
//! - **Safety & Observability**: workspace boundary enforcement, command
//!   allow/deny lists, human-in-the-loop confirmation, structured event
//!   logging, and Anthropic sandbox runtime integration for secure command execution.
//!
//! ## Architecture Overview
//!
//! The crate is organized into several key modules:
//!
//! - `config/`: configuration loader, defaults, and schema validation.
//! - `llm/`: provider clients, request shaping, and response handling.
//! - `tools/`: built-in tool implementations plus registration utilities.
//! - `context/`: conversation management, summarization, and memory.
//! - `executor/`: async orchestration for tool invocations and streaming output.
//! - `tree_sitter/`: language-specific parsers, syntax tree caching, and
//!   semantic extraction helpers.
//! - `core/prompt_caching`: cross-provider prompt caching system that leverages
//!   provider-specific caching mechanisms for cost optimization and reduced latency.
//! - `sandbox/`: integration with Anthropic sandbox runtime (`srt`) for secure command execution with configurable permissions.
//!
//! ## Quickstart
//!
//! ```rust,ignore
//! use vtcode_core::{Agent, VTCodeConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), anyhow::Error> {
//!     // Load configuration from vtcode.toml or environment overrides
//!     let config = VTCodeConfig::load()?;
//!
//!     // Construct the agent runtime
//!     let agent = Agent::new(config).await?;
//!
//!     // Execute an interactive session
//!     agent.run().await?;
//!
//!     Ok(())
//! }
//! ```
//!
//! ## Extending VT Code
//!
//! Register custom tools or providers by composing the existing traits:
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
//!             "properties": { "input": { "type": "string" } }
//!         }),
//!         handler: |_args| async move {
//!             // Implement your tool behavior here
//!             Ok(serde_json::json!({ "result": "success" }))
//!         },
//!     };
//!
//!     registry.register_tool(custom_tool).await?;
//!     Ok(())
//! }
//! ```
//!
//! For a complete tour of modules and extension points, read
//! `docs/ARCHITECTURE.md` and the guides in `docs/project/`.
//!
//! ## Agent Client Protocol (ACP)
//!
//! VT Code's binary exposes an ACP bridge for Zed. Enable it via the `[acp]` section in
//! `vtcode.toml`, launch the `vtcode acp` subcommand, and register the binary under
//! `agent_servers` in Zed's `settings.json`. Detailed instructions and troubleshooting live in the
//! [Zed ACP integration guide](https://github.com/vinhnx/vtcode/blob/main/docs/guides/zed-acp.md),
//! with a rendered summary on
//! [docs.rs](https://docs.rs/vtcode/latest/vtcode/#agent-client-protocol-acp).

//! ### Bridge guarantees
//!
//! - Tool exposure follows capability negotiation: `read_file` stays disabled unless Zed
//!   advertises `fs.read_text_file`.
//! - Each filesystem request invokes `session/request_permission`, ensuring explicit approval
//!   within the editor before data flows.
//! - Cancellation signals propagate into VT Code, cancelling active tool calls and ending the
//!   turn with `StopReason::Cancelled`.
//! - ACP `plan` entries track analysis, context gathering, and response drafting for timeline
//!   parity with Zed.
//! - Absolute-path checks guard every `read_file` argument before forwarding it to the client.
//! - Non-tool-capable models trigger reasoning notices and an automatic downgrade to plain
//!   completions without losing plan consistency.

//!
//! VTCode Core Library
//!
//! This crate provides the core functionality for the VTCode agent,
//! including tool implementations, LLM integration, and utility functions.

// Public modules
pub mod bash_runner;
pub mod cli;
pub mod code;
pub mod commands;
pub mod config;
pub mod constants;
pub mod core;
pub mod exec;
pub mod execpolicy;
pub mod gemini;
pub mod instructions;
pub mod llm;
pub mod markdown_storage;
pub mod mcp_client;
pub mod models;
pub mod project;
pub mod project_doc;
pub mod prompts;
pub mod safety;
pub mod sandbox;
pub mod simple_indexer;
pub mod tool_policy;
pub mod tools;
pub mod types;
pub mod ui;
pub mod utils;

// Re-exports for convenience
pub use bash_runner::BashRunner;
pub use cli::args::{Cli, Commands};
pub use code::code_completion::{CompletionEngine, CompletionSuggestion};
pub use commands::stats::handle_stats_command;
pub use config::types::{
    AnalysisDepth, CapabilityLevel, CommandResult, CompressionLevel, ContextConfig, LoggingConfig,
    OutputFormat, PerformanceMetrics, ReasoningEffortLevel, SessionInfo, ToolConfig,
};
pub use config::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentConfig, VTCodeConfig, WorkspaceTrustLevel,
};
pub use core::agent::core::Agent;
pub use core::agent::runner::{
    AgentRunner, ContextItem as RunnerContextItem, Task as RunnerTask,
    TaskOutcome as RunnerTaskOutcome, TaskResults as RunnerTaskResults,
};
pub use core::context_compression::{
    CompressedContext, ContextCompressionConfig, ContextCompressor,
};
pub use core::conversation_summarizer::ConversationSummarizer;
pub use core::performance_profiler::PerformanceProfiler;
pub use core::prompt_caching::{CacheStats, PromptCache, PromptCacheConfig, PromptOptimizer};
pub use core::timeout_detector::TimeoutDetector;
pub use exec::events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, ErrorItem, FileChangeItem,
    FileUpdateChange, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, McpToolCallItem,
    McpToolCallStatus, PatchApplyStatus, PatchChangeKind, ReasoningItem, ThreadEvent, ThreadItem,
    ThreadItemDetails, TurnCompletedEvent, TurnFailedEvent, TurnStartedEvent, Usage, WebSearchItem,
};
pub use gemini::{Content, FunctionDeclaration, Part};
pub use llm::{AnyClient, make_client};
pub use markdown_storage::{MarkdownStorage, ProjectData, ProjectStorage, SimpleKVStorage};
pub use project::{SimpleCache, SimpleProjectManager};
pub use prompts::{
    generate_lightweight_instruction, generate_specialized_instruction, generate_system_instruction,
};
pub use sandbox::SandboxProfile;  // Configuration for Anthropic sandbox runtime integration
pub use simple_indexer::SimpleIndexer;
pub use tool_policy::{ToolPolicy, ToolPolicyManager};
pub use tools::advanced_search::{AdvancedSearchTool, SearchOptions};
pub use tools::grep_file::GrepSearchManager;
pub use tools::tree_sitter::TreeSitterAnalyzer;
pub use tools::{
    ToolRegistration, ToolRegistry, build_function_declarations,
    build_function_declarations_for_level, build_function_declarations_with_mode,
};
pub use ui::diff_renderer::DiffRenderer;
pub use utils::dot_config::{
    CacheConfig, DotConfig, DotManager, ProviderConfigs, UiConfig, UserPreferences,
    WorkspaceTrustRecord, WorkspaceTrustStore, initialize_dot_folder, load_user_config,
    save_user_config, update_model_preference, update_theme_preference,
};
pub use utils::vtcodegitignore::initialize_vtcode_gitignore;

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn test_library_exports() {
        // Test that all public exports are accessible
        let _cache = PromptCache::new();
    }

    #[test]
    fn test_module_structure() {
        // Test that all modules can be imported
        // This is a compile-time test that ensures module structure is correct
    }

    #[test]
    fn test_version_consistency() {
        // Test that version information is consistent across modules
        // This would be more meaningful with actual version checking
    }

    #[tokio::test]
    async fn test_tool_registry_integration() {
        use crate::config::constants::tools;

        let temp_dir = TempDir::new().unwrap();
        std::env::set_current_dir(&temp_dir).unwrap();

        let mut registry = ToolRegistry::new(temp_dir.path().to_path_buf());

        // Test that we can execute basic tools
        let list_args = serde_json::json!({
            "path": "."
        });

        let result = registry.execute_tool(tools::LIST_FILES, list_args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.unwrap();
        assert!(response["files"].is_array());
    }

    #[tokio::test]
    async fn test_pty_basic_command() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();
        let mut registry = ToolRegistry::new(workspace.clone());

        // Test a simple PTY command
        let args = serde_json::json!({
            "command": "echo",
            "args": ["Hello, PTY!"]
        });

        let result = registry.execute_tool("run_pty_cmd", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["code"], 0);
        assert!(response["output"].as_str().unwrap().contains("Hello, PTY!"));
    }

    #[tokio::test]
    async fn test_pty_session_management() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path().to_path_buf();
        let mut registry = ToolRegistry::new(workspace.clone());

        // Test creating a PTY session
        let args = serde_json::json!({
            "session_id": "test_session",
            "command": "bash"
        });

        let result = registry.execute_tool("create_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");

        // Test listing PTY sessions
        let args = serde_json::json!({});
        let result = registry.execute_tool("list_pty_sessions", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert!(
            response["sessions"]
                .as_array()
                .unwrap()
                .contains(&"test_session".into())
        );

        // Test closing a PTY session
        let args = serde_json::json!({
            "session_id": "test_session"
        });

        let result = registry.execute_tool("close_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");
    }
}
