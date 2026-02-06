use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Notify;
use vtcode_core::config::constants::tools;
use vtcode_core::ui::tui::{InlineHandle, InlineSession};

use crate::agent::runloop::unified::ask_user_question::execute_ask_user_question_tool;
use crate::agent::runloop::unified::request_user_input;
use crate::agent::runloop::unified::state::CtrlCState;

pub(crate) async fn execute_hitl_tool(
    tool_name: &str,
    handle: &InlineHandle,
    session: &mut InlineSession,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Option<Result<Value>> {
    match tool_name {
        tools::ASK_USER_QUESTION => Some(
            execute_ask_user_question_tool(handle, session, args, ctrl_c_state, ctrl_c_notify)
                .await,
        ),
        tools::REQUEST_USER_INPUT | tools::ASK_QUESTIONS => Some(
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
