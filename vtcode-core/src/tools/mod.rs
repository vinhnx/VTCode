//! # Tool System Architecture
//!
//! This module provides a modular, composable architecture for VT Code agent tools,
//! implementing a registry-based system for tool discovery, execution, and management.
//!
//! ## Architecture Overview
//!
//! The tool system is designed around several key principles:
//!
//! - **Modularity**: Each tool is a focused, reusable component
//! - **Registry Pattern**: Centralized tool registration and discovery
//! - **Policy-Based Execution**: Configurable execution policies and safety checks
//! - **Type Safety**: Strong typing for tool parameters and results
//! - **Async Support**: Full async/await support for all tool operations
//!
//! ## Core Components
//!
//! ### Tool Registry
//! ```rust,no_run
//! use vtcode_core::tools::{ToolRegistry, ToolRegistration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let workspace = std::env::current_dir()?;
//!     let mut registry = ToolRegistry::new(workspace);
//!
//!     // Register a custom tool
//!     let tool = ToolRegistration {
//!         name: "my_tool".to_string(),
//!         description: "A custom tool".to_string(),
//!         parameters: serde_json::json!({"type": "object"}),
//!         handler: |args| async move {
//!             Ok(serde_json::json!({"result": "success"}))
//!         },
//!     };
//!
//!     registry.register_tool(tool).await?;
//!     Ok(())
//! }
//! ```
//!
//! ### Tool Categories
//!
//! #### File Operations
//! - **File Operations**: Read, write, create, delete files
//! - **Search Tools**: grep_file with ripgrep for fast regex-based pattern matching, glob patterns, type filtering
//! - **Cache Management**: File caching and performance optimization
//!
//! #### Terminal Integration
//! - **Bash Tools**: Shell command execution
//! - **PTY Support**: Full terminal emulation
//! - **Command Policies**: Safety and execution controls
//!
//! #### Code Analysis
//! ## Tool Execution
//!
//! ```rust,no_run
//! use vtcode_core::tools::ToolRegistry;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut registry = ToolRegistry::new(std::env::current_dir()?);
//!
//!     // Execute a tool
//!     let args = serde_json::json!({"path": "."});
//!     let result = registry.execute_tool("list_files", args).await?;
//!
//!     println!("Result: {}", result);
//!     Ok(())
//! }
//! ```
//!
//! ## Safety & Policies
//!
//! The tool system includes comprehensive safety features:
//!
//! - **Path Validation**: All file operations check workspace boundaries
//! - **Command Policies**: Configurable allow/deny lists for terminal commands
//! - **Execution Limits**: Timeout and resource usage controls
//! - **Audit Logging**: Complete trail of tool executions
//!
//! ## Custom Tool Development
//!
//! ```rust,no_run
//! use vtcode_core::tools::traits::Tool;
//! use serde_json::Value;
//!
//! struct MyCustomTool;
//!
//! #[async_trait::async_trait]
//! impl Tool for MyCustomTool {
//!     async fn execute(&self, args: Value) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
//!         // Tool implementation
//!         Ok(serde_json::json!({"status": "completed"}))
//!     }
//!
//!     fn name(&self) -> &str {
//!         "my_custom_tool"
//!     }
//!
//!     fn description(&self) -> &str {
//!         "A custom tool for specific tasks"
//!     }
//!
//!     fn parameters(&self) -> Value {
//!         serde_json::json!({
//!             "type": "object",
//!             "properties": {
//!                 "input": {"type": "string"}
//!             }
//!         })
//!     }
//! }
//! ```
//!
//! Modular tool system for VT Code
//!
//! This module provides a composable architecture for agent tools, breaking down
//! the monolithic implementation into focused, reusable components.

pub mod apply_patch;
pub mod builder;
pub mod constants;
pub mod error_messages;
pub mod request_user_input;

pub mod autonomous_executor;
pub mod cache;
pub mod command;
pub mod command_cache;
pub mod command_policy;
pub mod command_resolver;
pub mod editing;
pub mod error_context;
pub mod error_helpers;
pub mod execution_context;
pub mod execution_tracker;
pub mod file_ops;
pub mod file_search_bridge;
pub mod file_search_rpc;
pub mod file_tracker;
pub mod generation_helpers;
pub mod grep_cache;
pub mod grep_file;
pub mod handlers;
pub mod invocation;
pub mod mcp;
pub mod names;
pub mod path_env;
pub mod plugins;
pub mod pty;
pub mod rate_limiter;
pub mod registry;
pub mod result;
pub mod result_cache;
pub mod result_metadata;
pub mod safety_gateway;
pub mod search_metrics;
pub mod shell;
pub mod shell_snapshot;
pub mod skills;
pub mod summarizers;
pub mod terminal_app;
pub mod tool_effectiveness;
pub mod tool_intent;
pub mod traits;
pub mod types;
pub mod validation;
pub mod validation_cache;
pub mod web_fetch;

// Production-grade improvements modules
pub mod adaptive_rate_limiter;
pub mod async_middleware;
pub mod async_pipeline;
pub mod circuit_breaker;
pub mod golden_path_orchestrator;
pub mod health;
pub mod improvement_algorithms;
pub mod improvements_cache; // Deprecated - use crate::cache::UnifiedCache instead
pub mod improvements_config;
pub mod improvements_errors;
pub mod improvements_registry_ext;
#[allow(deprecated)]
pub mod middleware; // Deprecated - prefer async_middleware
pub mod optimized_registry;
pub mod output_spooler;
pub mod parallel_executor;
pub mod parallel_tool_batch;
pub mod pattern_engine;
pub mod request_response;
pub mod unified_error;
pub mod unified_executor;

#[cfg(test)]
#[allow(deprecated)]
mod improvements_integration_tests;

#[cfg(test)]
#[allow(deprecated)]
mod improvements_real_workflow_tests;

// Re-export main types and traits for backward compatibility
pub use async_pipeline::{
    AsyncToolPipeline, ExecutionContext, ExecutionPriority, ToolRequest as AsyncToolRequest,
};
pub use autonomous_executor::{AutonomousExecutor, AutonomousPolicy};
pub use cache::FileCache;
pub use command_cache::PermissionCache;
pub use command_resolver::CommandResolver;
pub use editing::{Patch, PatchError, PatchHunk, PatchLine, PatchOperation};
pub use error_context::ToolErrorContext;
pub use execution_context::{ToolExecutionContext, ToolExecutionRecord, ToolPattern};
pub use execution_tracker::{ExecutionRecord, ExecutionStats, ExecutionStatus, ExecutionTracker};
pub use file_search_rpc::{
    FileMatchRpc, FileSearchRpcHandler, ListFilesRequest, ListFilesResponse, RpcError, RpcRequest,
    RpcResponse, SearchFilesRequest, SearchFilesResponse,
};
pub use grep_file::GrepSearchManager;
pub use invocation::{
    InvocationBuilder, ToolInvocation as UnifiedToolInvocation, ToolInvocationId,
};

pub use optimized_registry::{OptimizedToolRegistry, ToolMetadata as OptimizedToolMetadata};
pub use plugins::{PluginHandle, PluginId, PluginInstaller, PluginManifest, PluginRuntime};
pub use pty::{PtyCommandRequest, PtyCommandResult, PtyManager};
pub use registry::{
    ApprovalPattern, ApprovalRecorder, JustificationExtractor, JustificationManager, RiskLevel,
    ToolJustification, ToolRegistration, ToolRegistry, ToolRiskContext, ToolRiskScorer, ToolSource,
    WorkspaceTrust,
};
pub use request_response::{ToolCallRequest, ToolCallResponse};
pub use result::{TokenCounts, ToolMetadata, ToolMetadataBuilder, ToolResult as SplitToolResult};
pub use result_cache::{ToolCacheKey, ToolResultCache};
pub use result_metadata::{
    EnhancedToolResult, ResultCompleteness, ResultMetadata, ResultScorer, ScorerRegistry,
};
pub use safety_gateway::{
    SafetyCheckResult, SafetyDecision, SafetyError, SafetyGateway, SafetyGatewayConfig, SafetyStats,
};
pub use search_metrics::{SearchMetric, SearchMetrics, SearchMetricsStats};
pub use shell_snapshot::{
    FileFingerprint, ShellKind, ShellSnapshot, ShellSnapshotManager, SnapshotStats,
    apply_snapshot_env, global_snapshot_manager,
};
pub use tool_effectiveness::{
    AdaptiveToolSelector, ToolEffectiveness, ToolEffectivenessTracker, ToolFailureMode,
    ToolSelectionContext, ToolSelector,
};
pub use traits::{Tool, ToolExecutor};
pub use types::*;
pub use web_fetch::WebFetchTool;

// Dynamic context discovery
pub use output_spooler::{SpoolResult, SpoolerConfig, ToolOutputSpooler};

// Production-grade improvements re-exports
pub use async_middleware::{
    AsyncCachingMiddleware, AsyncLoggingMiddleware, AsyncMiddleware, AsyncMiddlewareChain,
    AsyncRetryMiddleware, ToolRequest as MiddlewareToolRequest, ToolResult,
};
pub use improvement_algorithms::{
    MLScoreComponents, PatternDetector, PatternState, TimeDecayedScore, jaro_winkler_similarity,
};
pub use improvements_config::{
    CacheConfig, ContextConfig, FallbackConfig, ImprovementsConfig, PatternConfig,
    SimilarityConfig, TimeDecayConfig,
};
pub use improvements_errors::{
    ErrorKind, ErrorSeverity, EventType, ImprovementError, ImprovementEvent, ImprovementResult,
    ObservabilityContext, ObservabilitySink,
};
pub use improvements_registry_ext::{ToolMetrics, ToolRegistryImprovement};
#[allow(deprecated)]
#[deprecated(
    since = "0.1.0",
    note = "Use async_middleware types instead: AsyncMiddleware, AsyncMiddlewareChain, etc."
)]
pub use middleware::{
    CachingMiddleware, ExecutionMetadata, LoggingMiddleware, Middleware, MiddlewareChain,
    MiddlewareError, MiddlewareResult, RequestMetadata, RetryMiddleware, ToolRequest,
    ValidationMiddleware,
};
pub use pattern_engine::{DetectedPattern, ExecutionEvent, ExecutionSummary, PatternEngine};

// Golden Path Enforcement - Unified Executor
pub use unified_executor::{
    ApprovalState, ExecutionContextBuilder, PolicyConfig,
    ToolExecutionContext as UnifiedExecutionContext, ToolRegistryAdapter, TrustLevel,
    UnifiedExecutionResult, UnifiedToolExecutor,
};

// Parallel tool batch execution
pub use parallel_tool_batch::{ParallelToolBatch, QueuedToolCall};

// Golden Path Orchestrator - Consolidated execution entry point
pub use golden_path_orchestrator::{
    ExecutionBuilder, GoldenPathConfig, GoldenPathResult, execute_batch_golden_path,
    execute_golden_path, execute_golden_path_simple,
};

// Re-export function declarations for external use
pub use registry::build_function_declarations;
pub use registry::build_function_declarations_cached;
pub use registry::build_function_declarations_for_level;
pub use registry::build_function_declarations_with_mode;

// Codex-compatible handler architecture exports
pub use handlers::{
    // Apply patch handler
    ApplyPatchHandler,
    ApplyPatchRequest,
    ApplyPatchRuntime,
    ApplyPatchToolArgs,
    // Orchestrator and sandboxing
    Approvable,
    // Core handler traits and types
    ApprovalPolicy,
    // Turn diff tracker with Agent Trace support
    ChangeAttribution,
    CommandSpec,
    ConfiguredToolSpec,
    ContentItem,
    DiffTracker,
    // Event emission
    ExecCommandInput,
    ExecCommandSource,
    ExecEnv,
    ExecExpiration,
    ExecToolCallOutput,
    FileChange,
    FileChangeKind,
    FreeformTool,
    FreeformToolFormat,
    JsonSchema as ToolJsonSchema,
    McpToolResult,
    OutputText,
    ParsedCommand,
    ResponsesApiTool,
    SandboxAttempt,
    SandboxConfig,
    SandboxManager,
    SandboxMode,
    SandboxPermissions,
    SandboxPolicy,
    SandboxTransformError,
    Sandboxable,
    SandboxablePreference,
    SharedDiffTracker,
    SharedTurnDiffTracker,
    ShellEnvironmentPolicy,
    ShellToolCallParams,
    // Subagent
    SpawnSubagentTool,
    StdoutStream,
    ToolCallError,
    ToolCtx,
    ToolEmitter,
    ToolError,
    ToolEvent,
    ToolEventBegin,
    ToolEventCtx,
    ToolEventFailure,
    ToolEventFailureKind,
    ToolEventStage,
    ToolEventSuccess,
    ToolHandler,
    ToolInvocation,
    ToolKind,
    ToolOrchestrator,
    ToolOutput,
    ToolPayload,
    ToolRuntime,
    ToolSession,
    ToolSpec,
    TurnContext,
    TurnDiffTracker,
    create_apply_patch_freeform_tool,
    create_apply_patch_json_tool,
    intercept_apply_patch,
    new_shared_tracker,
};
