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
    match approval_policy {
        AskForApproval::Never => requires_rule_prompt || requires_sandbox_prompt,
        AskForApproval::Reject(reject_config) => {
            (requires_rule_prompt && reject_config.rejects_rules_approval())
                || (requires_sandbox_prompt && reject_config.rejects_sandbox_approval())
        }
        AskForApproval::OnFailure | AskForApproval::OnRequest | AskForApproval::UnlessTrusted => {
            false
        }
    }
}
