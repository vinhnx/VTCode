//! Prototype crate that exposes VTCode's tool registry and built-in tools.
//!
//! The goal is to surface the current API surface to external consumers
//! while we iterate on decoupling policies, configuration, and optional
//! dependencies. By shipping this crate as a thin wrapper we can collect
//! integration feedback and identify breaking changes early.
//!
//! Feature flags mirror the extraction plan so adopters can opt into only the
//! tool categories they need.
//!
//! See `docs/vtcode_tools_policy.md` for guidance on supplying a custom
//! `ToolPolicyManager` when the `policies` feature is enabled, allowing
//! consumers to store policy configuration outside of VTCode's defaults.

pub use vtcode_commons::{
    ErrorFormatter, ErrorReporter, NoopErrorReporter, NoopTelemetry, PathResolver, TelemetrySink,
    WorkspacePaths,
};

#[cfg(feature = "policies")]
pub mod adapters;

#[cfg(feature = "policies")]
pub use adapters::{RegistryBuilder, RegistryEvent};

pub use vtcode_core::tools::command;
pub use vtcode_core::tools::names;

pub mod registry {
    //! Registry exports shared across tool categories.
    pub use vtcode_core::tools::registry::{
        self, ToolPermissionDecision, ToolRegistration, ToolRegistry, build_function_declarations,
        build_function_declarations_for_level, build_function_declarations_with_mode,
    };
}

pub use registry::{
    ToolPermissionDecision, ToolRegistration, ToolRegistry, build_function_declarations,
    build_function_declarations_for_level, build_function_declarations_with_mode,
};

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
    pub use vtcode_core::tools::ast_grep::*;
    pub use vtcode_core::tools::ast_grep_tool::AstGrepTool;
    pub use vtcode_core::tools::grep_file::GrepSearchManager;
    pub use vtcode_core::tools::tree_sitter::*;
}

#[cfg(feature = "search")]
pub use search::{AstGrepTool, GrepSearchManager};

#[cfg(feature = "net")]
pub mod net {
    pub use vtcode_core::tools::curl_tool::CurlTool;
}

#[cfg(feature = "net")]
pub use net::CurlTool;

#[cfg(feature = "planner")]
pub mod planner {
    pub use vtcode_core::tools::plan::{
        PlanCompletionState, PlanManager, PlanStep, PlanSummary, PlanUpdateResult, StepStatus,
        TaskPlan, UpdatePlanArgs,
    };
}

#[cfg(feature = "planner")]
pub use planner::{
    PlanCompletionState, PlanManager, PlanStep, PlanSummary, PlanUpdateResult, StepStatus,
    TaskPlan, UpdatePlanArgs,
};

#[cfg(feature = "policies")]
pub mod policies {
    pub use vtcode_core::tool_policy::{ToolPolicy, ToolPolicyManager};
}

#[cfg(feature = "policies")]
pub use policies::{ToolPolicy, ToolPolicyManager};

#[cfg(feature = "examples")]
pub mod examples {
    //! Legacy registry helpers used by the headless integration examples
    //! under `vtcode-tools/examples`.
    pub use vtcode_core::tools::registry::legacy::*;
}
