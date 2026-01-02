//! Tool Orchestrator (from Codex)
//!
//! Central place for approvals + sandbox selection + retry semantics. Drives a
//! simple sequence for any ToolRuntime: approval → select sandbox → attempt →
//! retry without sandbox on denial (no re-approval thanks to caching).

use crate::tools::handlers::sandboxing::{
    ApprovalCtx, AskForApproval, ExecApprovalRequirement, ReviewDecision, SandboxAttempt,
    SandboxManager, SandboxOverride, SandboxType, ToolCtx, ToolError, ToolRuntime,
    default_exec_approval_requirement,
};
use crate::tools::handlers::tool_handler::TurnContext;

/// Tool orchestrator for coordinating execution (from Codex)
///
/// The orchestrator handles:
/// 1. Approval flow with caching
/// 2. Sandbox creation and selection
/// 3. Retry logic for sandbox escalation
pub struct ToolOrchestrator {
    sandbox: SandboxManager,
}

impl Default for ToolOrchestrator {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolOrchestrator {
    pub fn new() -> Self {
        Self {
            sandbox: SandboxManager::new(),
        }
    }

    /// Run a tool with the orchestrator managing sandbox and retries (from Codex)
    ///
    /// Flow:
    /// 1. Check exec approval requirement
    /// 2. Request approval if needed
    /// 3. Select initial sandbox
    /// 4. First attempt
    /// 5. On sandbox denial, retry without sandbox (with approval if needed)
    pub async fn run<Req, Out, T>(
        &mut self,
        tool: &mut T,
        req: &Req,
        tool_ctx: &ToolCtx<'_>,
        turn_ctx: &TurnContext,
        approval_policy: AskForApproval,
    ) -> Result<Out, ToolError>
    where
        Req: Send + Sync,
        Out: Send + Sync,
        T: ToolRuntime<Req, Out>,
    {
        // 1) Determine approval requirement
        let mut already_approved = false;

        let requirement = tool.exec_approval_requirement(req).unwrap_or_else(|| {
            default_exec_approval_requirement(approval_policy, &turn_ctx.sandbox_policy)
        });

        match &requirement {
            ExecApprovalRequirement::Skip { .. } => {
                // No approval needed, continue
                tracing::debug!("Skipping approval for tool: {}", tool_ctx.tool_name);
            }
            ExecApprovalRequirement::Forbidden { reason } => {
                return Err(ToolError::Rejected(reason.clone()));
            }
            ExecApprovalRequirement::NeedsApproval { reason, .. } => {
                // Request approval
                let approval_ctx = ApprovalCtx {
                    session: tool_ctx.session,
                    turn: tool_ctx.turn,
                    call_id: &tool_ctx.call_id,
                    retry_reason: reason.clone(),
                };

                let decision = tool.start_approval_async(req, approval_ctx).await;

                match decision {
                    ReviewDecision::Denied | ReviewDecision::Abort => {
                        return Err(ToolError::Rejected("rejected by user".to_string()));
                    }
                    ReviewDecision::Approved
                    | ReviewDecision::ApprovedExecpolicyAmendment { .. }
                    | ReviewDecision::ApprovedForSession => {
                        already_approved = true;
                    }
                }
            }
        }

        // 2) Select initial sandbox
        let initial_sandbox = match tool.sandbox_mode_for_first_attempt(req) {
            SandboxOverride::BypassSandboxFirstAttempt => SandboxType::None,
            SandboxOverride::NoOverride => self
                .sandbox
                .select_initial(&turn_ctx.sandbox_policy, tool.sandbox_preference()),
        };

        // 3) First attempt
        let initial_attempt = SandboxAttempt {
            sandbox: initial_sandbox,
            policy: &turn_ctx.sandbox_policy,
            sandbox_cwd: &turn_ctx.cwd,
            codex_linux_sandbox_exe: turn_ctx.codex_linux_sandbox_exe.as_ref(),
        };

        match tool.run(req, &initial_attempt, tool_ctx).await {
            Ok(out) => Ok(out),
            Err(ToolError::SandboxDenied(output)) => {
                // 4) Handle sandbox denial
                if !tool.escalate_on_failure() {
                    return Err(ToolError::SandboxDenied(output));
                }

                // Under `Never` or `OnRequest`, do not retry without sandbox
                if !tool.wants_no_sandbox_approval(approval_policy) {
                    return Err(ToolError::SandboxDenied(build_denial_reason_from_output(
                        Some(&output),
                    )));
                }

                // Ask for approval before retrying without sandbox
                if !tool.should_bypass_approval(approval_policy, already_approved) {
                    let reason_msg = build_denial_reason_from_output(Some(&output));
                    let approval_ctx = ApprovalCtx {
                        session: tool_ctx.session,
                        turn: tool_ctx.turn,
                        call_id: &tool_ctx.call_id,
                        retry_reason: Some(reason_msg),
                    };

                    let decision = tool.start_approval_async(req, approval_ctx).await;

                    match decision {
                        ReviewDecision::Denied | ReviewDecision::Abort => {
                            return Err(ToolError::Rejected("rejected by user".to_string()));
                        }
                        ReviewDecision::Approved
                        | ReviewDecision::ApprovedExecpolicyAmendment { .. }
                        | ReviewDecision::ApprovedForSession => {}
                    }
                }

                // 5) Second attempt without sandbox
                let escalated_attempt = SandboxAttempt {
                    sandbox: SandboxType::None,
                    policy: &turn_ctx.sandbox_policy,
                    sandbox_cwd: &turn_ctx.cwd,
                    codex_linux_sandbox_exe: None,
                };

                tool.run(req, &escalated_attempt, tool_ctx).await
            }
            Err(other) => Err(other),
        }
    }
}

/// Build a denial reason message from output (from Codex)
fn build_denial_reason_from_output(output: Option<&str>) -> String {
    match output {
        Some(o) if !o.is_empty() => format!("Sandbox denied with output: {}", truncate_output(o)),
        _ => "Sandbox denied execution".to_string(),
    }
}

/// Truncate output for display
fn truncate_output(output: &str) -> &str {
    const MAX_LEN: usize = 500;
    if output.len() > MAX_LEN {
        &output[..MAX_LEN]
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_denial_reason_empty() {
        assert_eq!(
            build_denial_reason_from_output(None),
            "Sandbox denied execution"
        );
        assert_eq!(
            build_denial_reason_from_output(Some("")),
            "Sandbox denied execution"
        );
    }

    #[test]
    fn test_build_denial_reason_with_output() {
        let reason = build_denial_reason_from_output(Some("permission denied"));
        assert!(reason.contains("permission denied"));
    }

    #[test]
    fn test_truncate_output() {
        let short = "hello";
        assert_eq!(truncate_output(short), short);

        let long = "a".repeat(1000);
        assert_eq!(truncate_output(&long).len(), 500);
    }
}
