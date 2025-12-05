//! # Tool System Architecture
//!
//! This module provides a modular, composable architecture for VTCode agent tools,
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
//! - **Tree-Sitter**: Syntax-aware code analysis
//!
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
//! Modular tool system for VTCode
//!
//! This module provides a composable architecture for agent tools, breaking down
//! the monolithic implementation into focused, reusable components.

pub mod apply_patch;
pub mod constants;
pub mod error_messages;

pub mod cache;
pub mod command;
pub mod command_cache;
pub mod command_policy;
pub mod command_resolver;
pub mod editing;
pub mod error_context;
pub mod execution_context;
pub mod fallback_chains;
pub mod file_ops;
pub mod grep_cache;
pub mod grep_file;
pub mod names;
pub(crate) mod path_env;
pub mod plan;
pub mod pty;
pub mod registry;
pub mod result_cache;
pub mod result_metadata;
pub mod search_metrics;
pub mod shell;
pub mod smart_cache;
pub mod terminal_app;
pub mod tool_effectiveness;
pub mod traits;
pub mod tree_sitter;
pub mod types;
pub mod web_fetch;

// Production-grade improvements modules
pub mod async_middleware;
pub mod improvement_algorithms;
#[deprecated(since = "0.47.7", note = "Use crate::cache::UnifiedCache instead")]
pub mod improvements_cache; // Deprecated - kept for backward compatibility only
pub mod improvements_config;
pub mod improvements_errors;
pub mod improvements_registry_ext;
pub mod middleware;
pub mod pattern_engine;

#[cfg(test)]
mod improvements_integration_tests;

#[cfg(test)]
mod improvements_real_workflow_tests;

// Re-export main types and traits for backward compatibility
pub use cache::FileCache;
pub use command_cache::PermissionCache;
pub use command_resolver::CommandResolver;
pub use editing::{Patch, PatchError, PatchHunk, PatchLine, PatchOperation};
pub use error_context::ToolErrorContext;
pub use execution_context::{ToolExecutionContext, ToolExecutionRecord, ToolPattern};
pub use fallback_chains::{
    AbortCondition, ChainStopReason, FallbackChain, FallbackChainExecutor, FallbackChainResult,
    FallbackStep,
};
pub use grep_file::GrepSearchManager;
pub use plan::{
    PlanCompletionState, PlanManager, PlanStep, PlanSummary, PlanUpdateResult, StepStatus,
    TaskPlan, UpdatePlanArgs,
};
pub use pty::{PtyCommandRequest, PtyCommandResult, PtyManager};
pub use registry::{
    ApprovalPattern, ApprovalRecorder, JustificationExtractor, JustificationManager, RiskLevel,
    ToolJustification, ToolRegistration, ToolRegistry, ToolRiskContext, ToolRiskScorer, ToolSource,
    WorkspaceTrust,
};
pub use result_cache::{ToolCacheKey, ToolResultCache};
pub use result_metadata::{
    EnhancedToolResult, ResultCompleteness, ResultMetadata, ResultScorer, ScorerRegistry,
};
pub use search_metrics::{SearchMetric, SearchMetrics, SearchMetricsStats};
pub use smart_cache::{CachedResult as SmartCachedResult, SmartResultCache};
pub use tool_effectiveness::{
    AdaptiveToolSelector, ToolEffectiveness, ToolEffectivenessTracker, ToolFailureMode,
    ToolSelectionContext, ToolSelector,
};
pub use traits::{Tool, ToolExecutor};
pub use types::*;
pub use web_fetch::WebFetchTool;

// Production-grade improvements re-exports
pub use async_middleware::{
    AsyncCachingMiddleware, AsyncLoggingMiddleware, AsyncMiddleware, AsyncMiddlewareChain,
    AsyncRetryMiddleware, ToolRequest as AsyncToolRequest, ToolResult,
};
pub use improvement_algorithms::{
    MLScoreComponents, PatternDetector, PatternState, TimeDecayedScore, jaro_winkler_similarity,
};
// Deprecated exports - use crate::cache instead
#[allow(deprecated)]
#[deprecated(since = "0.47.7", note = "Use crate::cache::CacheStats instead")]
pub use improvements_cache::CacheStats as LruCacheStats;
#[allow(deprecated)]
#[deprecated(since = "0.47.7", note = "Use crate::cache::UnifiedCache instead")]
pub use improvements_cache::LruCache;
pub use improvements_config::{
    CacheConfig, ContextConfig, FallbackConfig, ImprovementsConfig, PatternConfig,
    SimilarityConfig, TimeDecayConfig,
};
pub use improvements_errors::{
    ErrorKind, ErrorSeverity, EventType, ImprovementError, ImprovementEvent, ImprovementResult,
    ObservabilityContext, ObservabilitySink,
};
pub use improvements_registry_ext::{ToolMetrics, ToolRegistryImprovement};
pub use middleware::{
    CachingMiddleware, ExecutionMetadata, LoggingMiddleware, Middleware, MiddlewareChain,
    MiddlewareError, MiddlewareResult, RequestMetadata, RetryMiddleware, ToolRequest,
    ValidationMiddleware,
};
pub use pattern_engine::{DetectedPattern, ExecutionEvent, ExecutionSummary, PatternEngine};

// Re-export function declarations for external use
pub use registry::build_function_declarations;
pub use registry::build_function_declarations_for_level;
pub use registry::build_function_declarations_with_mode;
