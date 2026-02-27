use anyhow::{Result, anyhow};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_tui::{EditingMode, InlineHandle, InlineSession};

use crate::agent::runloop::unified::request_user_input;
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) async fn execute_hitl_tool(
    tool_name: &str,
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    editing_mode: EditingMode,
) -> Option<Result<Value>> {
    if tool_name == tools::REQUEST_USER_INPUT && editing_mode != EditingMode::Plan {
        let message = format!(
            "request_user_input is unavailable in {} mode",
            editing_mode.display_name()
        );
        return Some(Err(anyhow!(message)));
    }

    match tool_name {
        tools::REQUEST_USER_INPUT => Some(
            request_user_input::execute_request_user_input_tool(
                handle,
                session,
                args,
                ctrl_c_state,
                ctrl_c_notify,
            )
            .await,
        ),
        _ => None,
    }
}
