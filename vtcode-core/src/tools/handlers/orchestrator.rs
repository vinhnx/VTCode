//! Legacy orchestrator compatibility shim.
//!
//! Keep this module importable for older handler code, but delegate all
//! behavior to the active sandboxing and tool orchestrator modules.

pub use super::sandboxing::{
    Approvable, ApprovalCtx, ApprovalStore, AskForApproval, BoxFuture, CommandSpec,
    ExecApprovalRequirement, ExecEnv, ExecPolicyAmendment, ExecToolCallOutput, NetworkAccess,
    RejectConfig, ReviewDecision, SandboxAttempt, SandboxManager, SandboxMode, SandboxOverride,
    SandboxPolicy, SandboxTransformError, SandboxType, Sandboxable, SandboxablePreference, ToolCtx,
    ToolError, ToolRuntime, canonical_sandbox_policy, default_exec_approval_requirement,
    execute_env, with_cached_approval,
};
pub use super::tool_orchestrator::ToolOrchestrator;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orchestrator_path_reexports_active_sandboxing_types() {
        let _: SandboxPolicy = SandboxPolicy::default();
        let _: ToolOrchestrator = ToolOrchestrator::default();
    }
}
