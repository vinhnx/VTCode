//! Tool handlers module (Codex-compatible architecture)
//!
//! This module implements the handler pattern from OpenAI's Codex project,
//! providing a modular and composable approach to tool execution.
//!
//! ## Key Components
//!
//! - [`tool_handler`]: Core traits and types (ToolHandler, ToolKind, ToolPayload, etc.)
//! - [`sandboxing`]: Approval, sandbox, and runtime traits (from Codex)
//! - [`tool_orchestrator`]: Approval → sandbox → attempt → retry orchestration
//! - [`orchestrator`]: Legacy sandbox management (for backwards compatibility)
//! - [`events`]: Event emission for tool lifecycle (begin, success, failure)
//! - [`router`]: Tool routing and dispatch (ToolRouter, ToolRegistry, ToolRegistryBuilder)
//! - [`adapter`]: Bidirectional adapters between ToolHandler and Tool trait
//! - [`turn_diff_tracker`]: Aggregates file diffs across patches in a turn
//! - [`intercept_apply_patch`]: Shell command interception for apply_patch
//!
//! ## Handlers
//!
//! - [`apply_patch_handler`]: Apply patch tool implementation
//! - [`shell_handler`]: Shell command execution
//! - [`read_file`]: File reading with line ranges
//! - [`grep_files_handler`]: Pattern search across files
//! - [`list_dir_handler`]: Directory listing
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
pub mod adapter;
pub mod apply_patch_handler;
pub mod events;
pub mod intercept_apply_patch;
pub mod orchestrator;
pub mod router;
pub mod sandboxing;
pub mod tool_handler;
pub mod tool_orchestrator;
pub mod turn_diff_tracker;

// Handler implementations
pub mod grep_files_handler;
pub mod list_dir_handler;
pub mod plan_mode;
pub mod plan_task_tracker;
pub mod read_file;
pub mod shell_handler;
pub mod spawn_subagent;
pub mod task_tracker;
pub mod task_tracking;

// Re-export main types for convenience

// Adapter layer
pub use adapter::{
    DefaultToolSession, HandlerToToolAdapter, ToolToHandlerAdapter, create_cwd_session,
};

// Apply patch handler
pub use apply_patch_handler::{
    ApplyPatchHandler, ApplyPatchRequest as ApplyPatchHandlerRequest, ApplyPatchRuntime,
    ApplyPatchToolArgs, create_apply_patch_freeform_tool, create_apply_patch_json_tool,
};

// Events
pub use events::{
    ExecCommandInput, ExecCommandSource, ParsedCommand, ToolEmitter, ToolEventCtx,
    ToolEventFailureKind, ToolEventStage,
};

// Grep files handler
pub use grep_files_handler::{GrepFilesArgs, GrepFilesHandler, GrepMatch, create_grep_files_tool};

// Intercept apply patch
pub use intercept_apply_patch::{
    ApplyPatchError, ApplyPatchRequest, CODEX_APPLY_PATCH_ARG, intercept_apply_patch,
    maybe_parse_apply_patch_from_command,
};

// List directory handler
pub use list_dir_handler::{DirEntry, ListDirArgs, ListDirHandler, create_list_dir_tool};

// Legacy Orchestrator (for backwards compatibility)
pub use orchestrator::{
    Approvable as LegacyApprovable, CommandSpec as LegacyCommandSpec, ExecEnv as LegacyExecEnv,
    ExecExpiration, ExecToolCallOutput as LegacyExecToolCallOutput, OutputText,
    SandboxAttempt as LegacySandboxAttempt, SandboxConfig, SandboxManager as LegacySandboxManager,
    SandboxMode as LegacySandboxMode, SandboxPolicy as LegacySandboxPolicy,
    SandboxTransformError as LegacySandboxTransformError, Sandboxable as LegacySandboxable,
    SandboxablePreference as LegacySandboxablePreference, StdoutStream, ToolCtx as LegacyToolCtx,
    ToolError as LegacyToolError, ToolOrchestrator as LegacyToolOrchestrator,
    ToolRuntime as LegacyToolRuntime,
};

// New Sandboxing module (Codex-compatible)
pub use sandboxing::{
    Approvable, ApprovalCtx, ApprovalStore, AskForApproval, BoxFuture, CommandSpec,
    ExecApprovalRequirement, ExecEnv, ExecPolicyAmendment, ExecToolCallOutput, NetworkAccess,
    ReviewDecision, SandboxAttempt, SandboxManager, SandboxMode, SandboxOverride, SandboxPolicy,
    SandboxTransformError, SandboxType, Sandboxable, SandboxablePreference, ToolCtx, ToolError,
    ToolRuntime, default_exec_approval_requirement, execute_env, with_cached_approval,
};

// Tool Orchestrator (Codex-compatible)
pub use tool_orchestrator::ToolOrchestrator;

// Turn Diff Tracker with Agent Trace support
pub use turn_diff_tracker::{
    ChangeAttribution, FileChange, FileChangeKind, SharedTurnDiffTracker, TurnDiffTracker,
    new_shared_tracker,
};

// Router
pub use router::{
    ConfiguredToolSpec as RouterConfiguredToolSpec, ToolCall, ToolRegistry, ToolRegistryBuilder,
    ToolRouter, ToolRouterProvider,
};

// Shell handler
pub use shell_handler::{ShellHandler, create_shell_tool};

// Spawn subagent
pub use spawn_subagent::SpawnSubagentTool;

// Plan mode tools
pub use plan_mode::{EnterPlanModeTool, ExitPlanModeTool, PlanModeState};

// Task tracker (NL2Repo-Bench)
pub use plan_task_tracker::PlanTaskTrackerTool;
pub use task_tracker::TaskTrackerTool;

// Core tool handler types
pub use tool_handler::{
    AdditionalProperties, ApprovalPolicy, ConfiguredToolSpec, ContentItem, DiffTracker,
    FreeformTool, FreeformToolFormat, JsonSchema, McpToolResult, PatchApplyBeginEvent,
    PatchApplyEndEvent, ResponsesApiTool, SandboxPermissions, SharedDiffTracker,
    ShellEnvironmentPolicy, ShellToolCallParams, ToolCallError, ToolEvent, ToolEventBegin,
    ToolEventFailure, ToolEventSuccess, ToolHandler, ToolInvocation, ToolKind, ToolOutput,
    ToolPayload, ToolSession, ToolSpec, TurnContext,
};

// Legacy FileChange re-export for backward compatibility
pub use tool_handler::FileChange as LegacyFileChange;
