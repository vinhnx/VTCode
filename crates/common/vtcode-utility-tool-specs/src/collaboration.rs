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
                "description": "spawn: delegate a scoped task to a child agent (requires message). spawn_subprocess: launch a managed background subprocess for long-running daemons (requires message). send_input: send follow-up input to a running child (requires id + message or items). resume: reopen a completed or closed child from saved context (requires id). wait: block the current foreground turn until one or more children reach a terminal state (requires ids). close: cancel and free a child's tool budget (requires id)."
            },
            "agent_type": {"type": "string", "description": "spawn or spawn_subprocess: subagent type or name to run."},
            "message": {"type": "string", "description": "spawn or spawn_subprocess: task prompt. send_input: follow-up prompt for the child."},
            "items": {
                "type": "array",
                "description": "Structured context items for the child.",
                "items": collaboration_input_item_schema()
            },
            "fork_context": {"type": "boolean", "description": "spawn: seed the child with the current thread history.", "default": false},
            "model": {"type": "string", "description": "spawn or spawn_subprocess: model override. Omit to use parent model."},
            "reasoning_effort": {"type": "string", "description": "spawn or spawn_subprocess: reasoning effort override."},
            "background": {"type": "boolean", "description": "spawn: run the child agent in background and return immediately.", "default": false},
            "max_turns": {"type": "integer", "description": "spawn or spawn_subprocess: optional turn limit for the child."},
            "id": {"type": "string", "description": "send_input, resume, or close: child agent id."},
            "interrupt": {"type": "boolean", "description": "send_input: abort current child work and restart with this input; false queues it.", "default": false},
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
pub fn spawn_agent_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agent_type": {"type": "string", "description": "Subagent type or name to run."},
            "message": {"type": "string", "description": "Task prompt for the child agent."},
            "items": {
                "type": "array",
                "description": "Structured context items for the child agent.",
                "items": collaboration_input_item_schema()
            },
            "fork_context": {"type": "boolean", "description": "Seed the child with the current thread history.", "default": false},
            "model": {
                "type": "string",
                "description": "Model override. Omit to use parent model."
            },
            "reasoning_effort": {"type": "string", "description": "Reasoning effort override."},
            "background": {
                "type": "boolean",
                "description": "Run agent in background. Returns immediately.",
                "default": false
            },
            "max_turns": {
                "type": "integer",
                "description": "Optional turn limit for this child. Values below 2 are promoted to 2 so the child can recover from an initial blocked or denied tool call."
            }
        }
    })
}

#[must_use]
pub fn spawn_background_subprocess_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "agent_type": {"type": "string", "description": "Background-enabled subagent type or name to run."},
            "message": {"type": "string", "description": "Task prompt for the background subprocess."},
            "items": {
                "type": "array",
                "description": "Structured context items for the background subprocess.",
                "items": collaboration_input_item_schema()
            },
            "model": {
                "type": "string",
                "description": "Model override. Omit to use parent model."
            },
            "reasoning_effort": {"type": "string", "description": "Reasoning effort override."},
            "max_turns": {
                "type": "integer",
                "description": "Optional turn limit for the launched background subprocess task before it reports readiness. Values below 4 are promoted to 4 for background launches."
            }
        }
    })
}

#[must_use]
pub fn send_input_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": {"type": "string", "description": "Child agent id to message."},
            "message": {"type": "string", "description": "Follow-up prompt for the child."},
            "items": {
                "type": "array",
                "description": "Structured follow-up items.",
                "items": collaboration_input_item_schema()
            },
            "interrupt": {"type": "boolean", "description": "When true, abort current child work and restart with this input. When false (default), queue the input; if the child is already running, it starts the child's next turn after the current turn completes.", "default": false}
        }
    })
}

#[must_use]
pub fn wait_agent_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["ids"],
        "properties": {
            "ids": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Child agent ids to wait for. This blocks the current foreground turn until one target reaches a terminal state or the wait times out."
            },
            "timeout_ms": {
                "type": "integer",
                "description": "Optional wait timeout in milliseconds. Uses the session default timeout when omitted."
            }
        }
    })
}

#[must_use]
pub fn resume_agent_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": {"type": "string", "description": "Child agent id to resume."}
        }
    })
}

#[must_use]
pub fn close_agent_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": {"type": "string", "description": "Child agent id to close."}
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
        let spawn_items = &spawn_agent_parameters()["properties"]["items"]["items"];
        let send_items = &send_input_parameters()["properties"]["items"]["items"];

        assert_eq!(spawn_items, send_items);
        assert_eq!(spawn_items["additionalProperties"], json!(false));
        assert_eq!(spawn_items["properties"]["image_url"]["type"], json!("string"));
    }
    #[test]
    fn collaboration_schemas_expose_updated_agent_description_text() {
        let spawn = spawn_agent_parameters();
        let spawn_background = spawn_background_subprocess_parameters();
        let send = send_input_parameters();
        let wait = wait_agent_parameters();

        assert_eq!(spawn["properties"]["message"]["description"], json!("Task prompt for the child agent."));
        assert_eq!(send["properties"]["id"]["description"], json!("Child agent id to message."));
        assert_eq!(
            spawn["properties"]["background"]["description"],
            json!("Run agent in background. Returns immediately.")
        );
        assert_eq!(
            spawn_background["properties"]["message"]["description"],
            json!("Task prompt for the background subprocess.")
        );
        assert_eq!(
            wait["properties"]["ids"]["description"],
            json!(
                "Child agent ids to wait for. This blocks the current foreground turn until one target reaches a terminal state or the wait times out."
            )
        );
        assert_eq!(
            wait["properties"]["timeout_ms"]["description"],
            json!("Optional wait timeout in milliseconds. Uses the session default timeout when omitted.")
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
            schema["properties"]["questions"]["items"]["properties"]["options"]["items"]["properties"]["description"]["type"],
            json!("string")
        );
    }
}
