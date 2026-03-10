use serde_json::Value;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::tools::{ToolRiskContext, ToolSource, WorkspaceTrust};

pub(super) fn build_tool_risk_context(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> ToolRiskContext {
    let mut risk_context = ToolRiskContext::new(
        tool_name.to_string(),
        ToolSource::Internal,
        WorkspaceTrust::Untrusted,
    );
    if let Some(args) = tool_args {
        risk_context.command_args = vec![args.to_string()];
    }
    risk_context
}

pub(super) fn approval_policy_rejects_prompt(
    approval_policy: AskForApproval,
    requires_rule_prompt: bool,
    requires_sandbox_prompt: bool,
) -> bool {
    (requires_rule_prompt && approval_policy.rejects_rule_prompt())
        || (requires_sandbox_prompt && approval_policy.rejects_sandbox_prompt())
}
