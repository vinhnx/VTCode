use serde_json::Value;
use vtcode_core::tools::ToolInvocationId;

use crate::agent::runloop::unified::tool_call_safety::{SafetyError, ToolCallSafetyValidator};

pub(crate) enum SafetyValidationFailure {
    /// The safety gateway requires human approval before this tool call can
    /// proceed.  The inner string is the justification (risk description)
    /// that should be forwarded to the HITL permission prompt.
    NeedsApproval(String),
    Validation(SafetyError),
}

pub(crate) async fn validate_tool_call_safety(
    safety_validator: &ToolCallSafetyValidator,
    tool_name: &str,
    args: &Value,
    invocation_id: ToolInvocationId,
) -> Result<(), SafetyValidationFailure> {
    match safety_validator
        .validate_call_with_invocation_id(tool_name, args, invocation_id)
        .await
    {
        Ok(()) => Ok(()),
        Err(SafetyError::NeedsApproval(justification)) => Err(SafetyValidationFailure::NeedsApproval(justification)),
        Err(error) => Err(SafetyValidationFailure::Validation(error)),
    }
}
