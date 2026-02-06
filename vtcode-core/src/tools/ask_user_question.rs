use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;

/// Tool declaration for asking the human a question via the interactive UI.
///
/// The actual interactive UI implementation is provided by the VT Code front-end
/// (TUI runloop) which can intercept this tool call and present a modal.
pub struct AskUserQuestionTool;

#[async_trait]
impl Tool for AskUserQuestionTool {
    async fn execute(&self, _args: Value) -> Result<Value> {
        Err(anyhow!(
            "ask_user_question requires an interactive UI session and is handled by the VT Code front-end"
        ))
    }

    fn name(&self) -> &'static str {
        tools::ASK_USER_QUESTION
    }

    fn description(&self) -> &'static str {
        "Ask the human a question via VT Code's interactive UI with tabbed choices. Prefer \
        request_user_input for simple clarifications."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "title": {"type": "string", "description": "Modal title"},
                "question": {"type": "string", "description": "Prompt shown to the user"},
                "tabs": {
                    "type": "array",
                    "minItems": 1,
                    "description": "Tabbed choices. Each tab contains a selectable list.",
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "id": {"type": "string"},
                            "title": {"type": "string"},
                            "items": {
                                "type": "array",
                                "minItems": 1,
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "properties": {
                                        "id": {"type": "string"},
                                        "title": {"type": "string"},
                                        "subtitle": {"type": "string"},
                                        "badge": {"type": "string"}
                                    },
                                    "required": ["id", "title"]
                                }
                            }
                        },
                        "required": ["id", "title", "items"]
                    }
                },
                "allow_freeform": {
                    "type": "boolean",
                    "description": "If true, the UI may offer an option for freeform text input."
                },
                "freeform_label": {"type": "string"},
                "freeform_placeholder": {"type": "string"},
                "default_tab_id": {"type": "string"},
                "default_choice_id": {"type": "string"}
            },
            "required": ["question", "tabs"]
        }))
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
