use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::tools::ToolInvocationId;
use vtcode_tui::app::InlineHandle;

use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_call_safety::{SafetyError, ToolCallSafetyValidator};
use crate::agent::runloop::unified::tool_routing::prompt_session_limit_increase;

pub(crate) enum SafetyValidationFailure {
    SessionLimitNotIncreased,
    SessionLimitPromptFailed(anyhow::Error),
    Validation(SafetyError),
}

pub(crate) async fn validate_tool_call_with_limit_prompt<S: UiSession + ?Sized>(
    safety_validator: &ToolCallSafetyValidator,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    tool_name: &str,
    args: &Value,
    invocation_id: ToolInvocationId,
) -> Result<(), SafetyValidationFailure> {
    loop {
        match safety_validator
            .validate_call_with_invocation_id(tool_name, args, invocation_id)
            .await
        {
            Ok(()) => return Ok(()),
            Err(SafetyError::SessionLimitReached { max }) => {
                match prompt_session_limit_increase(
                    handle,
                    session,
                    ctrl_c_state,
                    ctrl_c_notify,
                    max,
                )
                .await
                {
                    Ok(Some(increment)) => safety_validator.increase_session_limit(increment),
                    Ok(None) => {
                        return Err(SafetyValidationFailure::SessionLimitNotIncreased);
                    }
                    Err(error) => {
                        return Err(SafetyValidationFailure::SessionLimitPromptFailed(error));
                    }
                }
            }
            Err(error) => return Err(SafetyValidationFailure::Validation(error)),
        }
    }
}
