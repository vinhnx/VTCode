//! Prototype crate that exposes VT Code's tool registry and built-in tools.
//!
//! The goal is to surface the current API surface to external consumers
//! while we iterate on decoupling policies, configuration, and optional
//! dependencies. By shipping this crate as a thin wrapper we can collect
//! integration feedback and identify breaking changes early.
//!
//! Feature flags mirror the extraction plan so adopters can opt into only the
//! tool categories they need.
//!
//! See `docs/modules/vtcode_tools_policy.md` for guidance on supplying a custom
//! `ToolPolicyManager` when the `policies` feature is enabled, allowing
//! consumers to store policy configuration outside of VT Code's defaults.

pub use vtcode_commons::{
    ErrorFormatter, ErrorReporter, NoopErrorReporter, NoopTelemetry, PathResolver, TelemetrySink,
    WorkspacePaths,
};

#[cfg(feature = "policies")]
pub mod adapters;

#[cfg(feature = "policies")]
pub use adapters::{RegistryBuilder, RegistryEvent};

pub mod acp_tool;
pub use acp_tool::{AcpDiscoveryTool, AcpHealthTool, AcpTool};

pub use vtcode_collaboration_tool_specs::{
    close_agent_parameters, request_user_input_description, request_user_input_parameters,
    resume_agent_parameters, send_input_parameters, spawn_agent_parameters, wait_agent_parameters,
};
pub use vtcode_utility_tool_specs::{
    APPLY_PATCH_ALIAS_DESCRIPTION, DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION, SEMANTIC_ANCHOR_GUIDANCE,
    apply_patch_parameter_schema, apply_patch_parameters, cron_create_parameters,
    cron_delete_parameters, cron_list_parameters, list_files_parameters, read_file_parameters,
    unified_exec_parameters, unified_file_parameters, unified_search_parameters,
    with_semantic_anchor_guidance,
};

pub mod cache;
pub use cache::{CacheObserver, CacheStats, EvictionReason, LruCache, NoopObserver};

pub mod middleware;
pub use middleware::{
    LoggingMiddleware, MetricsMiddleware, MetricsSnapshot, Middleware, MiddlewareChain,
    MiddlewareResult, NoopMiddleware, ToolRequest, ToolResponse,
};

pub mod patterns;
pub use patterns::{DetectedPattern, PatternDetector, ToolEvent};

pub mod executor;
pub use executor::{CachedToolExecutor, ExecutorStats};

pub mod optimizer;
pub use optimizer::{Optimization, OptimizationType, WorkflowOptimizer};

pub use vtcode_core::tools::command;
pub use vtcode_core::tools::names;
pub use vtcode_core::tools::{UnifiedErrorKind, UnifiedToolError};

pub mod registry {
    //! Registry exports shared across tool categories.
    pub use vtcode_core::tools::registry::{
        self, ToolPermissionDecision, ToolRegistration, ToolRegistry,
    };
}

pub use registry::{ToolPermissionDecision, ToolRegistration, ToolRegistry};

pub mod traits {
    pub use vtcode_core::tools::traits::{Tool, ToolExecutor};
}

pub use traits::{Tool, ToolExecutor};

pub mod types {
    pub use vtcode_core::tools::types::*;
}

pub use types::*;

#[cfg(feature = "bash")]
pub mod bash {
    pub use vtcode_core::tools::pty::{PtyCommandRequest, PtyCommandResult, PtyManager};
}

#[cfg(feature = "bash")]
pub use bash::{PtyCommandRequest, PtyCommandResult, PtyManager};

#[cfg(feature = "search")]
pub mod search {
    pub use vtcode_core::tools::grep_file::GrepSearchManager;
}

#[cfg(feature = "search")]
pub use search::GrepSearchManager;

// #[cfg(feature = "planner")]
// pub mod planner {
//     pub use vtcode_core::tools::plan::{
//         PlanCompletionState, PlanManager, PlanPhase, PlanStep, PlanSummary, PlanUpdateResult,
//         StepStatus, TaskPlan, TaskTrackerArgs,
//     };
// }

// #[cfg(feature = "planner")]
// pub use planner::{
//     PlanCompletionState, PlanManager, PlanPhase, PlanStep, PlanSummary, PlanUpdateResult,
//     StepStatus, TaskPlan, TaskTrackerArgs,
// };

#[cfg(feature = "policies")]
pub mod policies {
    pub use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
}

#[cfg(feature = "policies")]
pub use policies::{ToolPolicy, ToolPolicyManager};

#[cfg(feature = "examples")]
pub mod examples {
    //! File helper methods used by the headless integration examples
    //! under `vtcode-tools/examples`.
    #[allow(unused_imports)]
    pub use vtcode_core::tools::registry::file_helpers::*;
}
