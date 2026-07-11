//! Collaboration and human-in-the-loop tool schemas.

use serde_json::{Value, json};

#[must_use]
pub fn agent_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["spawn", "spawn_subprocess", "send_input", "resume", "wait", "close"],
                "description": "spawn: delegate a scoped task to a child agent (requires message). spawn_subprocess: launch a managed background subprocess for long-running daemons (requires message). send_input: send follow-up input to a running child (requires id + message or items). resume: reopen a completed/closed child from saved context (requires id). wait: block the current foreground turn until one or more children reach a terminal state (requires ids). close: cancel and free a child's tool budget (requires id)."
            },
            "agent_type": {"type": "string", "description": "spawn/spawn_subprocess: subagent type or name to run."},
            "message": {"type": "string", "description": "spawn/spawn_subprocess: task prompt. send_input: follow-up prompt for the child."},
            "items": {
                "type": "array",
                "description": "Structured context items for the child.",
                "items": collaboration_input_item_schema()
            },
            "fork_context": {"type": "boolean", "description": "spawn: seed the child with the current thread history.", "default": false},
            "model": {"type": "string", "description": "spawn/spawn_subprocess: model override. Omit to use parent model."},
            "reasoning_effort": {"type": "string", "description": "spawn/spawn_subprocess: reasoning effort override."},
            "background": {"type": "boolean", "description": "spawn: run the child agent in background and return immediately.", "default": false},
            "max_turns": {"type": "integer", "description": "spawn/spawn_subprocess: optional turn limit for the child."},
            "id": {"type": "string", "description": "send_input/resume/close: child agent id."},
            "interrupt": {"type": "boolean", "description": "send_input: abort current child work and restart with this input; false (default) queues it.", "default": false},
            "ids": {
                "type": "array",
                "items": {"type": "string"},
                "description": "wait: child agent ids to wait for. Blocks the current foreground turn until one target reaches a terminal state or the wait times out."
            },
            "timeout_ms": {
                "type": "integer",
                "description": "wait: optional wait timeout in milliseconds. Uses the session default timeout when omitted."
            }
        }
    })
}

#[must_use]
pub fn request_user_input_description() -> &'static str {
    "Request user input for one to three short questions. Blocks the agent loop until the user responds. Returns the user's answers mapped by question id. Canonical HITL tool for the Planning workflow."
}

#[must_use]
pub fn request_user_input_parameters() -> Value {
    json!({
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
                        "focus_area": {
                            "type": "string",
                            "description": "Optional short topic hint used to bias auto-suggested choices when options are omitted."
                        },
                        "analysis_hints": {
                            "type": "array",
                            "description": "Optional weakness/risk hints used by the UI to generate suggested options.",
                            "items": {
                                "type": "string"
                            },
                            "maxItems": 8
                        },
                        "options": {
                            "type": "array",
                            "description": "Optional 2-3 mutually exclusive choices. Put the recommended option first and suffix its label with \"(Recommended)\". Do not include an \"Other\" option; the UI provides that automatically. If omitted, the UI auto-suggests options using question text and hints.",
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
    })
}

fn collaboration_input_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {"type": "string"},
            "text": {"type": "string"},
            "path": {"type": "string"},
            "name": {"type": "string"},
            "image_url": {"type": "string"}
        },
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collaboration_schemas_keep_structured_items_consistent() {
        let schema = agent_parameters();
        let items = &schema["properties"]["items"]["items"];

        assert_eq!(items["additionalProperties"], json!(false));
        assert_eq!(items["properties"]["image_url"]["type"], json!("string"));
    }

    #[test]
    fn collaboration_schemas_expose_updated_agent_description_text() {
        let schema = agent_parameters();

        assert_eq!(
            schema["properties"]["action"]["enum"],
            json!([
                "spawn",
                "spawn_subprocess",
                "send_input",
                "resume",
                "wait",
                "close"
            ])
        );
        assert_eq!(
            schema["properties"]["message"]["description"],
            json!(
                "spawn/spawn_subprocess: task prompt. send_input: follow-up prompt for the child."
            )
        );
        assert_eq!(
            schema["properties"]["id"]["description"],
            json!("send_input/resume/close: child agent id.")
        );
        assert_eq!(
            schema["properties"]["background"]["description"],
            json!("spawn: run the child agent in background and return immediately.")
        );
        assert_eq!(
            schema["properties"]["ids"]["description"],
            json!(
                "wait: child agent ids to wait for. Blocks the current foreground turn until one target reaches a terminal state or the wait times out."
            )
        );
        assert_eq!(
            schema["properties"]["timeout_ms"]["description"],
            json!(
                "wait: optional wait timeout in milliseconds. Uses the session default timeout when omitted."
            )
        );
    }

    #[test]
    fn request_user_input_schema_preserves_description_field_name() {
        let schema = request_user_input_parameters();

        assert_eq!(schema["required"], json!(["questions"]));
        assert_eq!(
            schema["properties"]["questions"]["items"]["properties"]["options"]["items"]["required"],
            json!(["label", "description"])
        );
        assert_eq!(
            schema["properties"]["questions"]["items"]["properties"]["options"]["items"]["properties"]
                ["description"]["type"],
            json!("string")
        );
    }
}
