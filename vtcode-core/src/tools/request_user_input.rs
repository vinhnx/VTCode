use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use vtcode_collaboration_tool_specs::{
    request_user_input_description, request_user_input_parameters,
};

use crate::config::constants::tools;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;

/// Tool declaration for requesting structured user input mid-turn.
///
/// This tool allows the LLM to ask 1-3 short questions with optional multiple-choice
/// options.
///
/// The actual interactive UI implementation is provided by the VT Code front-end
/// (TUI runloop) which can intercept this tool call and present a modal.
pub struct RequestUserInputTool;

#[async_trait]
impl Tool for RequestUserInputTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        Err(anyhow!(
            "request_user_input requires an interactive UI session and is handled by the VT Code front-end"
        ))
    }

    fn name(&self) -> &'static str {
        tools::REQUEST_USER_INPUT
    }

    fn description(&self) -> &'static str {
        request_user_input_description()
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(request_user_input_parameters())
    }

    fn default_permission(&self) -> ToolPolicy {
        // Asking the user is always safe; it is still gated by interactive availability.
        ToolPolicy::Allow
    }

    fn is_mutating(&self) -> bool {
        false
    }

    fn is_parallel_safe(&self) -> bool {
        false
    }

    fn kind(&self) -> &'static str {
        "hitl"
    }
}
