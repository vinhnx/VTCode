use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::{Value, json};

use crate::config::constants::tools;
use crate::tool_policy::ToolPolicy;
use crate::tools::traits::Tool;

/// Tool declaration for requesting structured user input mid-turn.
///
/// This tool allows the LLM to ask 1-3 short questions with optional multiple-choice
/// options. Simpler than `ask_user_question` for basic clarifying questions.
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
        "Request user input for one to three short questions and wait for the response. Only available in Plan mode."
    }

    fn parameter_schema(&self) -> Option<Value> {
        Some(json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["questions"],
            "properties": {
                "questions": {
                    "type": "array",
                    "description": "Questions to show the user (1-3). Prefer 1 unless multiple independent decisions block progress.",
                    "minItems": 1,
                    "maxItems": 3,
                    "items": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["id", "header", "question"],
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Stable identifier for mapping answers (snake_case)."
                            },
                            "header": {
                                "type": "string",
                                "description": "Short header label shown in the UI (12 or fewer chars)."
                            },
                            "question": {
                                "type": "string",
                                "description": "Single-sentence prompt shown to the user."
                            },
                            "options": {
                                "type": "array",
                                "description": "Optional 2-3 mutually exclusive choices. Put the recommended option first and suffix its label with \"(Recommended)\". Only include \"Other\" option if a free form option is desired. If the question is free form in nature, do not include any options.",
                                "minItems": 2,
                                "maxItems": 3,
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["label", "description"],
                                    "properties": {
                                        "label": {
                                            "type": "string",
                                            "description": "User-facing label (1-5 words)."
                                        },
                                        "description": {
                                            "type": "string",
                                            "description": "One short sentence explaining impact/tradeoff if selected."
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
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
