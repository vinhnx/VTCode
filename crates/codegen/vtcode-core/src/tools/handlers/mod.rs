//! Tool handlers module
//!
//! The authoritative public tool surface, name resolution, and catalog assembly
//! live in `crate::tools::registry`. This module holds the handler-side types
//! and the tool implementations that the registry and runloop build on.
//!
//! The Codex-compatible execution scaffolding (`router`, `adapter`, `events`,
//! `apply_patch_handler`, `shell_handler`, `list_dir_handler`,
//! `tool_orchestrator`, `orchestrator`, `intercept_apply_patch`) was never
//! registered as a live path and has been removed. `tools::SHELL` and the other
//! command-session aliases resolve to `unified_exec` through
//! `tool_intent::canonical_command_session_tool_name`, and the executor
//! pipeline in `crate::tools::registry` owns dispatch. Do not reintroduce a
//! parallel handler path that bypasses it.
//!
//! ## Key Components
//!
//! - [`tool_handler`]: Handler-side types still used by live code (`ToolCallError`,
//!   `TurnContext`, `ToolSession`, approval/sandbox policy wrappers)
//! - [`sandboxing`]: Approval, sandbox, and runtime traits (from Codex)
//! - [`turn_diff_tracker`]: Aggregates file diffs across patches in a turn
//!
//! ## Handlers
//!
//! - [`compact`]: Tool description compaction
//! - [`planning_workflow`]: Plan approval artifacts and workflow state
//! - [`planning_task_tracker`], [`task_tracker`], [`task_tracking`]: Task trackers
//! - [`read_file`]: File reading with line ranges
//! - [`session_tool_catalog`]: Per-session tool catalog projection
//!
//! ## Usage
//!
//! ```rust,ignore
//! use vtcode_core::tools::handlers::{ToolHandler, ToolInvocation, ToolOutput};
//!
//! struct MyHandler;
//!
//! #[async_trait::async_trait]
//! impl ToolHandler for MyHandler {
//!     fn kind(&self) -> ToolKind {
//!         ToolKind::Function
//!     }
//!
//!     async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, ToolCallError> {
//!         Ok(ToolOutput::simple("Done!"))
//!     }
//! }
//! ```

// Core architecture modules
pub mod sandboxing;
pub mod tool_handler;
pub mod turn_diff_tracker;

// Handler implementations
pub mod compact;
pub mod planning_task_tracker;
pub mod planning_workflow;
pub mod read_file;
pub mod session_tool_catalog;
mod session_tool_projection;
pub mod task_tracker;
pub mod task_tracking;

// Re-export main types for convenience

// New Sandboxing module (Codex-compatible)
pub use sandboxing::{
    Approvable, ApprovalCtx, ApprovalStore, AskForApproval, BoxFuture, CommandSpec, ExecApprovalRequirement, ExecEnv,
    ExecPolicyAmendment, ExecToolCallOutput, NetworkAccess, RejectConfig, ReviewDecision, SandboxAttempt,
    SandboxConfig, SandboxManager, SandboxOverride, SandboxPolicy, SandboxTransformError, SandboxType, Sandboxable,
    SandboxablePreference, ToolCtx, ToolError, ToolRuntime, default_exec_approval_requirement, execute_env,
    with_cached_approval,
};

// Turn Diff Tracker with Agent Trace support
pub use turn_diff_tracker::{
    ChangeAttribution, FileChange, FileChangeKind, SharedTurnDiffTracker, TurnDiffTracker, new_shared_tracker,
};

pub use session_tool_catalog::{
    CatalogToolKind, DeferredToolPolicy, DeferredToolSearchKind, SessionSurface, SessionToolCatalog,
    SessionToolsConfig, ToolCatalogEntry, ToolCatalogSource, ToolModelCapabilities, ToolProfile, ToolSchemaEntry,
    anthropic_native_memory_enabled_for_runtime, deferred_tool_policy_for_runtime,
};
// Planning workflow tools
pub use planning_workflow::{PlanningWorkflowState, StartPlanningTool};

// Task tracker (NL2Repo-Bench)
pub use planning_task_tracker::PlanningTaskTrackerTool;
pub use task_tracker::TaskTrackerTool;

// Core tool handler types
pub use tool_handler::{
    AdditionalProperties, ApprovalPolicy, ConfiguredToolSpec, Constrained, ContentItem, DiffTracker, FreeformTool,
    FreeformToolFormat, JsonSchema, McpToolResult, ResponsesApiTool, SandboxPermissions, SharedDiffTracker,
    ShellEnvironmentPolicy, ShellToolCallParams, ToolCallError, ToolHandler, ToolInvocation, ToolKind, ToolOutput,
    ToolPayload, ToolSession, ToolSpec, TurnContext,
};

// Legacy FileChange re-export for backward compatibility
pub use tool_handler::FileChange as LegacyFileChange;
