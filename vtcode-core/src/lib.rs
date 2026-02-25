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
//! - **Semantic Workspace Model**: LLM-native code analysis and navigation
//!   across all modern programming languages.
//! - **Bash Shell Safety**: tree-sitter-bash integration for critical command validation
//!   and security enforcement.
//! - **Tool System**: trait-driven registry for shell execution, file IO,
//!   search, and custom commands, with Tokio-powered concurrency and PTY
//!   streaming.
//! - **Configuration-First**: everything is driven by `vtcode.toml`, with
//!   model, safety, and automation constants centralized in
//!   `config::constants` and curated metadata in `docs/models.json`.
//! - **Safety & Observability**: workspace boundary enforcement, command
//!   allow/deny lists, human-in-the-loop confirmation, and structured event
//!   logging for comprehensive audit trails.
//!
//! ## Architecture Overview
//!
//! The crate is organized into several key modules:
//!
//! - `config/`: configuration loader, defaults, and schema validation.
//! - `llm/`: provider clients, request shaping, and response handling.
//! - `tools/`: built-in tool implementations plus registration utilities.
//! - `context/`: conversation management and memory.
//! - `executor/`: async orchestration for tool invocations and streaming output.
//! - `core/prompt_caching`: cross-provider prompt caching system that leverages
//!   provider-specific caching mechanisms for cost optimization and reduced latency.
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
//! VT Code Core Library
//!
//! This crate provides the core functionality for the VT Code agent,
//! including tool implementations, LLM integration, and utility functions.

// Public modules
pub mod a2a; // Agent2Agent Protocol support
pub mod acp;
pub mod agent_teams;
#[cfg(feature = "anthropic-api")]
pub mod anthropic_api;
pub mod audit;
pub mod auth; // OAuth PKCE authentication for providers
pub mod cache; // Unified caching system
pub mod cli;
pub mod code;
pub mod command_safety; // Command safety detection (Codex patterns)
pub mod commands;
pub mod compaction;
pub mod config;
pub mod constants;
pub mod context; // Vibe coding support: entity resolution, workspace state, conversation memory
pub mod core;
pub mod diagnostics;
pub mod dotfile_protection; // Comprehensive dotfile protection system
pub mod exec;
pub mod exec_policy; // Codex-style execution policy management
/// Backward-compatible alias: command-level validation now lives in `exec_policy::command_validation`.
pub use exec_policy::command_validation as execpolicy;
pub mod gemini;
pub mod git_info; // Git repository information collection
pub mod http_client;
pub mod instructions;
pub mod llm;
pub mod marketplace;
pub mod mcp;
pub mod memory; // Memory monitoring and pressure detection
pub mod metrics;
pub mod models;
pub mod models_manager; // Models discovery, caching, and selection (Codex patterns)
pub mod notifications;
pub mod open_responses; // Open Responses specification conformance layer
pub mod orchestrator;
pub mod plugins;
pub mod project_doc;
pub mod prompts;
pub mod safety;
pub mod sandboxing; // Codex-style sandbox policy and execution environment
pub mod security;
pub mod session;
pub mod skills;
pub mod subagents;
pub mod telemetry;
pub mod terminal_setup;
pub mod tool_policy;
pub mod tools;
pub mod trace; // Agent Trace specification for AI code attribution
pub mod turn_metadata; // Turn metadata for LLM requests (git context)
pub mod types;
pub mod ui;
pub mod utils;

// Re-export common error macros and constants
pub use vtcode_commons::errors::*;
pub use vtcode_commons::{ctx_err, file_err};

// New MCP enhancement modules
// Re-exports for convenience
pub use cli::args::{Cli, Commands};
pub use code::code_completion::{CompletionEngine, CompletionSuggestion};
pub use commands::stats::handle_stats_command;
pub use config::types::{
    AnalysisDepth, CapabilityLevel, CommandResult, ContextConfig, LoggingConfig, OutputFormat,
    PerformanceMetrics, ReasoningEffortLevel, SessionInfo, ToolConfig,
};
pub use config::{
    AgentClientProtocolConfig, AgentClientProtocolTransport, AgentClientProtocolZedConfig,
    AgentClientProtocolZedToolsConfig, AgentConfig, PluginRuntimeConfig, PluginTrustLevel,
    VTCodeConfig, WorkspaceTrustLevel,
};
pub use core::agent::core::Agent;
pub use core::agent::runner::AgentRunner;
pub use core::agent::task::{
    ContextItem as RunnerContextItem, Task as RunnerTask, TaskOutcome as RunnerTaskOutcome,
    TaskResults as RunnerTaskResults,
};
pub use core::memory_pool::{MemoryPool, global_pool};
pub use core::optimized_agent::{AgentContext, AgentState, OptimizedAgentEngine};
pub use core::performance_profiler::{BenchmarkResults, BenchmarkUtils, PerformanceProfiler};
pub use vtcode_bash_runner::BashRunner;

pub use core::prompt_caching::{CacheStats, PromptCache, PromptCacheConfig, PromptOptimizer};
pub use core::timeout_detector::TimeoutDetector;
pub use diagnostics::{
    DiagnosticReport, HealthSample, LabeledAction, PredictiveMonitor, RecoveryAction,
    RecoveryPlaybook,
};
pub use dotfile_protection::{
    AccessType as DotfileAccessType, AuditEntry as DotfileAuditEntry, AuditLog as DotfileAuditLog,
    AuditOutcome as DotfileAuditOutcome, BackupManager as DotfileBackupManager, DotfileBackup,
    DotfileGuardian, ProtectionDecision, ProtectionViolation, get_global_guardian,
    init_global_guardian, is_protected_dotfile,
};
pub use exec::events::{
    AgentMessageItem, CommandExecutionItem, CommandExecutionStatus, EVENT_SCHEMA_VERSION,
    ErrorItem, FileChangeItem, FileUpdateChange, ItemCompletedEvent, ItemStartedEvent,
    ItemUpdatedEvent, McpToolCallItem, McpToolCallStatus, PatchApplyStatus, PatchChangeKind,
    PlanDeltaEvent, PlanItem, ReasoningItem, ThreadEvent, ThreadItem, ThreadItemDetails,
    ThreadStartedEvent, TurnCompletedEvent, TurnFailedEvent, TurnStartedEvent, Usage,
    VersionedThreadEvent, WebSearchItem,
};
pub use exec::{CodeExecutor, ExecutionConfig, ExecutionResult, Language};
pub use gemini::{Content, FunctionDeclaration, Part};
pub use llm::{AnyClient, make_client};
pub use mcp::{
    tool_discovery::{DetailLevel, ToolDiscovery, ToolDiscoveryResult},
    validate_mcp_config,
};
pub use memory::{MemoryCheckpoint, MemoryMonitor, MemoryPressure, MemoryReport};
pub use models_manager::{
    ModelFamily, ModelPreset, ModelsCache, ModelsManager, builtin_model_presets,
    model_family::find_family_for_model,
};
pub use notifications::{
    NotificationConfig, NotificationEvent, NotificationManager, get_global_notification_manager,
    init_global_notification_manager, notify_command_failure, notify_error,
    notify_human_in_the_loop, notify_tool_failure, send_global_notification,
};
pub use orchestrator::{
    DistributedOrchestrator, ExecutionTarget, ExecutorRegistry, LocalExecutor, ScheduledWork,
    WorkExecutor,
};
pub use prompts::{
    generate_lightweight_instruction, generate_specialized_instruction, generate_system_instruction,
};
pub use security::{IntegrityTag, PayloadEnvelope, ZeroTrustContext};
pub use telemetry::{TelemetryEvent, TelemetryPipeline};

// Open Responses specification types
pub use open_responses::{
    ContentPart, CustomItem, DualEventEmitter, FunctionCallItem, FunctionCallOutputItem,
    IncompleteDetails, IncompleteReason, InputTokensDetails, ItemStatus, MessageItem, MessageRole,
    OpenResponseError, OpenResponseErrorCode, OpenResponseErrorType, OpenResponsesCallback,
    OpenResponsesIntegration, OpenResponsesProvider, OpenUsage, OutputItem, OutputItemId,
    OutputTokensDetails, ReasoningItem as OpenReasoningItem, Response as OpenResponse,
    ResponseBuilder, ResponseId, ResponseStatus, ResponseStreamEvent, StreamEventEmitter,
    ToOpenResponse, VecStreamEmitter, generate_item_id, generate_response_id,
};

pub use tool_policy::{ToolPolicy, ToolPolicyManager};

// Codex-style execution policy and sandboxing
pub use exec_policy::{
    AskForApproval, Decision, ExecApprovalRequirement, ExecPolicyAmendment, ExecPolicyConfig,
    ExecPolicyManager, Policy, PolicyEvaluation, PolicyParser, PrefixRule, RuleMatch,
    SharedExecPolicyManager,
};
pub use sandboxing::{
    CommandSpec as SandboxCommandSpec, ExecEnv as SandboxExecEnv, ExecExpiration,
    SandboxManager as CodexSandboxManager, SandboxPermissions as CodexSandboxPermissions,
    SandboxPolicy as CodexSandboxPolicy, SandboxType, WritableRoot,
};

pub use tools::grep_file::GrepSearchManager;
pub use tools::{AsyncToolPipeline, AsyncToolRequest, ExecutionPriority, OptimizedToolRegistry};
pub use tools::{
    ToolRegistration, ToolRegistry, build_function_declarations,
    build_function_declarations_for_level, build_function_declarations_with_mode,
};

/// Macro for consistent error context formatting to reduce code duplication
/// Replaces repetitive `.with_context(|| format!("Failed to {} {}", operation, path.display()))?` patterns
#[macro_export]
macro_rules! error_context {
    ($operation:expr, $target:expr) => {
        anyhow::Context::with_context(|| format!("Failed to {} {}", $operation, $target))
    };
    ($operation:expr, $target:expr, $details:expr) => {
        anyhow::Context::with_context(|| {
            format!("Failed to {} {}: {}", $operation, $target, $details)
        })
    };
}
pub use ui::diff_renderer::DiffRenderer;
pub use utils::dot_config::{
    CacheConfig, DotConfig, DotManager, ProviderConfigs, UiConfig, UserPreferences,
    WorkspaceTrustRecord, WorkspaceTrustStore, initialize_dot_folder, load_user_config,
    save_user_config, update_model_preference, update_theme_preference,
};
pub use utils::vtcodegitignore::initialize_vtcode_gitignore;
pub use vtcode_indexer::SimpleIndexer;
pub use vtcode_markdown_store::{
    MarkdownStorage, ProjectData, ProjectStorage, SimpleCache, SimpleKVStorage,
    SimpleProjectManager,
};

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod memory_integration_tests;

#[cfg(test)]
mod config_verification_tests;

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[tokio::test]
    async fn test_library_exports() {
        // Test that all public exports are accessible
        let _cache = PromptCache::new().await;
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

        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        std::env::set_current_dir(&temp_dir).expect("Failed to change dir");

        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
        registry
            .initialize_async()
            .await
            .expect("Failed to init registry");

        // Test that we can execute basic tools
        let list_args = serde_json::json!({
            "path": "."
        });

        let result = registry.execute_tool(tools::LIST_FILES, list_args).await;
        assert!(result.is_ok());

        let response: serde_json::Value = result.expect("Failed to execute list_files");
        assert_eq!(response["success"], serde_json::Value::Bool(true));
        assert!(response["items"].is_array());
    }

    #[tokio::test]
    async fn test_pty_basic_command() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let workspace = temp_dir.path().to_path_buf();
        let registry = ToolRegistry::new(workspace.clone()).await;
        registry
            .initialize_async()
            .await
            .expect("Failed to init registry");

        // Test a simple PTY command
        let args = serde_json::json!({
            "command": "echo",
            "args": ["Hello, PTY!"]
        });

        let result = registry.execute_tool("run_pty_cmd", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.expect("Failed to run PTY");
        assert_eq!(response["success"], true);
        assert_eq!(response["code"], 0);
        assert!(
            response["output"]
                .as_str()
                .expect("Failed to read PTY output")
                .contains("Hello, PTY!")
        );
    }

    #[tokio::test]
    async fn test_pty_session_management() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let workspace = temp_dir.path().to_path_buf();
        let registry = ToolRegistry::new(workspace.clone()).await;
        registry
            .initialize_async()
            .await
            .expect("Failed to init registry");

        // Test creating a PTY session
        let args = serde_json::json!({
            "session_id": "test_session",
            "command": "bash"
        });

        let result = registry.execute_tool("create_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.expect("Failed to create PTY session");
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");

        // Test listing PTY sessions
        let args = serde_json::json!({});
        let result = registry.execute_tool("list_pty_sessions", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.expect("Failed to list PTY sessions");
        assert!(
            response["sessions"]
                .as_array()
                .expect("Failed to read sessions array")
                .contains(&"test_session".into())
        );

        // Test closing a PTY session
        let args = serde_json::json!({
            "session_id": "test_session"
        });

        let result = registry.execute_tool("close_pty_session", args).await;
        assert!(result.is_ok());
        let response: serde_json::Value = result.expect("Failed to close PTY session");
        assert_eq!(response["success"], true);
        assert_eq!(response["session_id"], "test_session");
    }
}
