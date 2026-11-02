//! Collaboration and human-in-the-loop tool schemas.

use serde_json::{Value, json};

/// Model-visible description of the `agent` tool.
pub const AGENT_DESCRIPTION: &str = "Spawn and steer delegated child agents. Use action=spawn to delegate a scoped task, action=spawn_subprocess for a managed background subagent, action=send_input to continue a child, action=resume to reopen a completed child, action=wait for results, or action=close to cancel a child. spawn_subprocess runs a subagent defined with background: true in a separate VT Code process; shell commands, including long-running ones such as dev servers, go through exec_command, with background=true to keep them running.";

#[must_use]
pub fn agent_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["spawn", "spawn_subprocess", "send_input", "resume", "wait", "close"],
                "description": "spawn: delegate a scoped task to a child agent (requires message). spawn_subprocess: run a subagent defined with background: true as a managed background VT Code process (requires message). send_input: send follow-up input to a running child (requires id + message or items). resume: reopen a completed or closed child from saved context (requires id). wait: block the current foreground turn until one or more children reach a terminal state, including managed background subprocess ids (requires ids). close: cancel and free a child's tool budget (requires id)."
            },
            "agent_type": {"type": "string", "description": agent_type_description("spawn or spawn_subprocess: ", " spawn_subprocess requires an agent defined with background: true.")},
            "message": {"type": "string", "description": "spawn or spawn_subprocess: task prompt. send_input: follow-up prompt for the child."},
            "items": {
                "type": "array",
                "description": ITEMS_DESCRIPTION,
                "items": collaboration_input_item_schema()
            },
            "fork_context": {"type": "boolean", "description": "spawn: seed the child with the current thread history.", "default": false},
            "model": {"type": "string", "description": "spawn or spawn_subprocess: model override. Omit to use parent model."},
            "reasoning_effort": reasoning_effort_schema("spawn or spawn_subprocess: "),
            "background": {"type": "boolean", "description": "spawn: run the child agent in background and return immediately.", "default": false},
            "max_turns": {"type": "integer", "description": "spawn or spawn_subprocess: optional turn limit for the child."},
            "id": {"type": "string", "description": "send_input, resume, or close: child agent id."},
            "interrupt": {"type": "boolean", "description": "send_input: abort current child work and restart with this input; false queues it.", "default": false},
            "ids": {
                "type": "array",
                "items": {"type": "string"},
                "description": "wait: child agent ids to wait for, including managed background subprocess ids (background-<name>). Blocks the current foreground turn until one target reaches a terminal state or the wait times out."
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
            "agent_type": {"type": "string", "description": agent_type_description("", "")},
            "message": {"type": "string", "description": "Task prompt for the child agent."},
            "items": {
                "type": "array",
                "description": ITEMS_DESCRIPTION,
                "items": collaboration_input_item_schema()
            },
            "fork_context": {"type": "boolean", "description": "Seed the child with the current thread history.", "default": false},
            "model": {
                "type": "string",
                "description": "Model override. Omit to use parent model."
            },
            "reasoning_effort": reasoning_effort_schema(""),
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
            "agent_type": {"type": "string", "description": agent_type_description("", " Requires an agent defined with background: true.")},
            "message": {"type": "string", "description": "Task prompt for the background subprocess."},
            "items": {
                "type": "array",
                "description": ITEMS_DESCRIPTION,
                "items": collaboration_input_item_schema()
            },
            "model": {
                "type": "string",
                "description": "Model override. Omit to use parent model."
            },
            "reasoning_effort": reasoning_effort_schema(""),
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
                "description": ITEMS_DESCRIPTION,
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
                "description": "Child agent ids to wait for, including managed background subprocess ids (background-<name>). This blocks the current foreground turn until one target reaches a terminal state or the wait times out."
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

/// Model-visible description of the `ask_session` tool.
pub const ASK_SESSION_DESCRIPTION: &str = "Ask a question of a peer CLI session running in this workspace. The peer sees the question on its next turn and can reply with answer_session; answers arrive back as notices. Use list_sessions to discover live peers first. Delivery is best-effort: the peer must be running to receive it.";

#[must_use]
pub fn ask_session_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["session", "question"],
        "properties": {
            "session": {"type": "string", "description": "Target peer session id (see list_sessions)."},
            "question": {"type": "string", "description": "Question text for the peer session."},
            "ttl_secs": {"type": "integer", "description": "Seconds the question stays deliverable. Defaults to 3600; clamped to 60..86400."}
        }
    })
}

/// Model-visible description of the `answer_session` tool.
pub const ANSWER_SESSION_DESCRIPTION: &str = "Answer a question received from a peer CLI session. Pass the message_id of the ask_session message you are replying to so the asker can match the reply.";

#[must_use]
pub fn answer_session_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["message_id", "answer"],
        "properties": {
            "message_id": {"type": "string", "description": "Message id of the question being answered."},
            "answer": {"type": "string", "description": "Answer text for the asking session."},
            "ttl_secs": {"type": "integer", "description": "Seconds the answer stays deliverable. Defaults to 3600; clamped to 60..86400."}
        }
    })
}

/// Model-visible description of the `list_sessions` tool.
pub const LIST_SESSIONS_DESCRIPTION: &str = "List live peer CLI sessions in this workspace that can receive ask_session messages. Returns session ids with their last-seen times.";

#[must_use]
pub fn list_sessions_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "include_self": {"type": "boolean", "description": "Include the calling session in the results.", "default": false}
        }
    })
}

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

/// Reasoning effort values accepted for a child agent override: every value
/// `ReasoningEffortLevel::parse` accepts, including `none` (the child sends no
/// reasoning configuration), taken from the single list in `vtcode-commons`.
pub const SUBAGENT_REASONING_EFFORT_VALUES: &[&str] = vtcode_commons::reasoning::constants::PARSEABLE_LEVELS;

const ITEMS_DESCRIPTION: &str = "Structured context items for the child. Each item carries one content field. Items are used only when message is empty; each item contributes its first non-empty field in the order text, path, name, image_url.";

fn agent_type_description(prefix: &str, suffix: &str) -> String {
    format!(
        "{prefix}Subagent name listed in the Subagents section of the system prompt (built-ins: default, explorer, worker; custom agents come from .vtcode/agents, .claude/agents, or .codex/agents in the workspace or home directory). Defaults to the agent the user explicitly mentioned, otherwise default.{suffix}"
    )
}

fn reasoning_effort_schema(prefix: &str) -> Value {
    json!({
        "type": "string",
        "enum": SUBAGENT_REASONING_EFFORT_VALUES,
        "description": format!(
            "{prefix}reasoning effort override for the child. When omitted, the child uses its agent definition's reasoning effort, or the parent's when the definition sets none."
        )
    })
}

fn collaboration_input_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "type": {
                "type": "string",
                "description": "Optional label naming the content field this item carries."
            },
            "text": {"type": "string", "description": "Inline text passed to the child as-is."},
            "path": {"type": "string", "description": "Workspace file path the child should read; rendered as \"Reference: <path>\"."},
            "name": {"type": "string", "description": "Name of a referenced symbol, agent, or resource, passed as-is."},
            "image_url": {"type": "string", "description": "Image URL; rendered as \"Image: <url>\"."}
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
        // The label is informational; the child reads only the content fields.
        assert!(spawn_items["properties"]["type"].get("enum").is_none());
        for field in ["type", "text", "path", "name", "image_url"] {
            let description = spawn_items["properties"][field]["description"].as_str().unwrap_or_default();
            assert!(!description.is_empty(), "item field {field} needs a description");
        }
    }

    #[test]
    fn collaboration_schemas_document_agent_type_sources_and_reasoning_enum() {
        let schemas = [
            agent_parameters(),
            spawn_agent_parameters(),
            spawn_background_subprocess_parameters(),
        ];
        for schema in &schemas {
            let agent_type = schema["properties"]["agent_type"]["description"].as_str().unwrap_or_default();
            assert!(agent_type.contains("Subagents section of the system prompt"));
            assert!(agent_type.contains("built-ins: default, explorer, worker"));
            assert!(agent_type.contains(".vtcode/agents"));
            assert!(agent_type.contains("Defaults to the agent the user explicitly mentioned"));

            let reasoning = &schema["properties"]["reasoning_effort"];
            assert_eq!(reasoning["enum"], json!(SUBAGENT_REASONING_EFFORT_VALUES));
            assert!(reasoning["description"].as_str().unwrap_or_default().contains("When omitted"));
        }
        assert_eq!(
            agent_parameters()["properties"]["items"]["description"],
            send_input_parameters()["properties"]["items"]["description"]
        );
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
                "Child agent ids to wait for, including managed background subprocess ids (background-<name>). This blocks the current foreground turn until one target reaches a terminal state or the wait times out."
            )
        );
        assert_eq!(
            wait["properties"]["timeout_ms"]["description"],
            json!("Optional wait timeout in milliseconds. Uses the session default timeout when omitted.")
        );
    }

    #[test]
    fn session_messaging_schemas_require_routing_fields() {
        let ask = ask_session_parameters();
        assert_eq!(ask["required"], json!(["session", "question"]));
        assert_eq!(ask["properties"]["session"]["type"], json!("string"));
        assert_eq!(ask["properties"]["question"]["type"], json!("string"));

        let answer = answer_session_parameters();
        assert_eq!(answer["required"], json!(["message_id", "answer"]));
        assert_eq!(answer["properties"]["message_id"]["type"], json!("string"));

        let list = list_sessions_parameters();
        assert!(list.get("required").is_none(), "list takes optional filters only");
        assert_eq!(list["properties"]["include_self"]["type"], json!("boolean"));
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
