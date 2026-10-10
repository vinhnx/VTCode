#![allow(
    missing_docs,
    dead_code,
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

//! Passive JSON schemas for utility, file, scheduling, and collaboration tool surfaces.

#![recursion_limit = "256"]
use serde_json::{Value, json};

mod collaboration;
mod json_schema;
mod matrix;
#[cfg(feature = "mcp")]
mod mcp_tool;
mod responses_api;
mod tool_kind;

pub use collaboration::{
    AGENT_DESCRIPTION, SUBAGENT_REASONING_EFFORT_VALUES, agent_parameters, close_agent_parameters,
    request_user_input_description, request_user_input_parameters, resume_agent_parameters, send_input_parameters,
    spawn_agent_parameters, spawn_background_subprocess_parameters, wait_agent_parameters,
};
pub use json_schema::{AdditionalProperties, JsonSchema, parse_tool_input_schema};
pub use matrix::{MATRIX_DESCRIPTION, matrix_parameters};
#[cfg(feature = "mcp")]
pub use mcp_tool::{ParsedMcpTool, parse_mcp_tool};
pub use responses_api::{FreeformTool, FreeformToolFormat, ResponsesApiTool};
pub(crate) use tool_kind::{CanonicalToolMeta, TokenBucket, ToolKind, ToolNamespace};

pub const SEMANTIC_ANCHOR_GUIDANCE: &str =
    "Prefer stable semantic @@ anchors such as function, class, method, or impl names.";

/// Model-visible description of the `patch` alias field. The old value
/// ("Alias for input") gave the model no format guidance, so it often placed
/// a standard unified diff (`---`/`+++`) there — which `apply_patch` rejects
/// (checkpoint turn_615). The full envelope and path rules live on the tool
/// description and the `input` field; this alias keeps only the
/// turn_615-critical rejection signal plus the alias relationship, so the
/// same guidance is not sent three times per request.
pub const APPLY_PATCH_ALIAS_DESCRIPTION: &str = "Alias for 'input': same VT Code patch envelope (*** Begin Patch … *** End Patch); unified diffs (---/+++ format) are rejected.";
pub const DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION: &str = "Patch in VT Code format: *** Begin Patch, *** Update File: path, @@ hunk, -/+ lines, *** End Patch. Every patch path must be workspace-relative; absolute paths, `..`, and traversal-like forms are rejected.";
/// Model-visible description of the `apply_patch` tool. It leads with the
/// accepted envelope so the model writes the right format on the first try,
/// and states the unified-diff rejection and path rules as plain facts
/// instead of shouted warnings. Registration sites append
/// [`SEMANTIC_ANCHOR_GUIDANCE`] via [`with_semantic_anchor_guidance`].
pub const APPLY_PATCH_TOOL_DESCRIPTION: &str = "Apply a patch in VT Code format (*** Begin Patch / *** Update File: path / @@ hunks with -/+ lines / *** End Patch); standard unified diffs (---/+++ format) are rejected. *** Add File: path, *** Delete File: path, and *** Move to: path (after *** Update File) are also supported. Every patch path must be workspace-relative; absolute paths, `..`, and traversal-like forms are rejected. Changes are applied after permission checks. Call this tool directly instead of through a shell; JSON calls use input (patch is an alias). Use complete current context/deletion lines, preserving internal whitespace; boundary whitespace and Unicode punctuation normalization are supported, but partial lines are rejected. A typed context mismatch permits one fresh bounded file read range per affected path per turn; other limits remain authoritative.";

/// Default model-visible preview budget for function-tool results.
pub const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;

/// Wire schema for optional public decision recording.
pub fn record_decision_parameters() -> Value {
    serde_json::json!({"type":"object","additionalProperties":false,"required":["summary","rationale"],"properties":{
        "summary":{"type":"string","minLength":1,"maxLength":240},
        "rationale":{"type":"string","minLength":1,"maxLength":1000},
        "alternatives":{"type":"array","maxItems":3,"items":{"type":"string","maxLength":1000}},
        "evidence_ids":{"type":"array","maxItems":8,"items":{"type":"string","minLength":1,"maxLength":240}}
    }})
}
/// Smallest valid model-visible preview budget for a function-tool result.
pub const MIN_MAX_OUTPUT_TOKENS: usize = 1;
/// Largest valid model-visible preview budget for a function-tool result.
pub const MAX_MAX_OUTPUT_TOKENS: usize = 50_000;
/// Canonical JSON property name for the common result-preview budget field.
///
/// Centralized so the strict dedicated validator in `vtcode-core` and any
/// compatibility coercion both reference one name and cannot drift.
pub const MAX_OUTPUT_TOKENS_FIELD: &str = "max_output_tokens";

/// Adds the common result-preview budget field to an object-shaped tool schema.
///
/// The returned schema deliberately preserves every existing constraint,
/// including `additionalProperties`, so callers can use it for legacy schemas
/// without weakening their argument validation.
#[must_use]
pub fn with_max_output_tokens_parameter(mut schema: Value) -> Value {
    let Some(schema_object) = schema.as_object_mut() else {
        return schema;
    };
    let properties = schema_object
        .entry("properties")
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let Some(properties_object) = properties.as_object_mut() else {
        return schema;
    };
    {
        let _entry = properties_object.entry(MAX_OUTPUT_TOKENS_FIELD).or_insert_with(|| {
            json!({
                "type": "integer",
                "minimum": MIN_MAX_OUTPUT_TOKENS,
                "maximum": MAX_MAX_OUTPUT_TOKENS,
                "default": DEFAULT_MAX_OUTPUT_TOKENS,
                "description": "Maximum number of result tokens returned to the model. Full oversized output is spooled when available."
            })
        });
    }
    schema
}

#[must_use]
pub fn with_semantic_anchor_guidance(base: &str) -> String {
    let trimmed = base.trim_end();
    if trimmed.contains(SEMANTIC_ANCHOR_GUIDANCE) {
        trimmed.to_string()
    } else if trimmed.ends_with('.') {
        format!("{trimmed} {SEMANTIC_ANCHOR_GUIDANCE}")
    } else {
        format!("{trimmed}. {SEMANTIC_ANCHOR_GUIDANCE}")
    }
}

#[must_use]
pub fn apply_patch_parameter_schema(input_description: &str) -> Value {
    json!({
        "type": "object",
        "properties": {
            "input": {
                "type": "string",
                "description": with_semantic_anchor_guidance(input_description)
            },
            "patch": {
                "type": "string",
                "description": with_semantic_anchor_guidance(APPLY_PATCH_ALIAS_DESCRIPTION)
            }
        },
        "anyOf": [
            {"required": ["input"]},
            {"required": ["patch"]}
        ]
    })
}

#[must_use]
pub fn apply_patch_parameters() -> Value {
    apply_patch_parameter_schema(DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION)
}

#[must_use]
pub fn cron_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "additionalProperties": false,
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "list", "delete"],
                "description": "create: schedule a prompt (requires prompt and exactly one of cron, delay_minutes, or run_at). list: show scheduled prompts. delete: remove one by id."
            },
            "prompt": {"type": "string", "description": "create: prompt to run when the task fires."},
            "name": {"type": "string", "description": "create: optional short label for the task."},
            "cron": {"type": "string", "description": "create: five-field cron expression for recurring tasks."},
            "delay_minutes": {"type": "integer", "description": "create: fixed recurring interval in minutes."},
            "run_at": {"type": "string", "description": "create: one-shot fire time in RFC3339 or local datetime form."},
            "id": {"type": "string", "description": "delete: session scheduled task id to delete."}
        }
    })
}

/// Model-visible description of the `mcp` tool. The `action` field description
/// owns the per-action enumeration; this description keeps only the purpose,
/// the server-search-vs-catalog-search distinction, and the disconnect guard.
pub const MCP_DESCRIPTION: &str = "Discover and manage Model Context Protocol capabilities. Use the `action` field to find tools, fetch one tool schema, list configured servers, and connect or disconnect by name. action=search_tools searches only tools exposed by configured MCP servers; the separate search_tools tool searches the whole session catalog, including deferred built-in tools. Do not disconnect a server while one of its tool calls is active.";

#[must_use]
pub fn mcp_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "properties": {
            "action": {
                "type": "string",
                "enum": ["search_tools", "get_tool_details", "list_servers", "connect", "disconnect"],
                "description": "search_tools: find MCP tools by natural-language query. get_tool_details: fetch the full input schema for one MCP tool name. list_servers: list configured servers and their connection state. connect or disconnect: manage one configured MCP server by name."
            },
            "query": {"type": "string", "description": "search_tools: natural language query describing the MCP capability to find."},
            "detail_level": {"type": "string", "enum": ["name", "name_description", "full"], "description": "search_tools: response detail level."},
            "limit": {"type": "integer", "minimum": 1, "maximum": 25, "description": "search_tools: maximum number of results to return."},
            "name": {"type": "string", "description": "get_tool_details: exact MCP tool name. connect or disconnect: configured MCP server name."}
        },
        "additionalProperties": false
    })
}

#[must_use]
pub fn cron_create_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["prompt"],
        "additionalProperties": false,
        "properties": {
            "prompt": {"type": "string", "description": "Prompt to run when the task fires."},
            "name": {"type": "string", "description": "Optional short label for the task."},
            "cron": {"type": "string", "description": "Five-field cron expression for recurring tasks."},
            "delay_minutes": {"type": "integer", "description": "Fixed recurring interval in minutes."},
            "run_at": {
                "type": "string",
                "description": "One-shot fire time in RFC3339 or local datetime form. Use this instead of `cron` or `delay_minutes` for reminders."
            }
        }
    })
}

#[must_use]
pub fn cron_list_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {},
        "additionalProperties": false
    })
}

#[must_use]
pub fn cron_delete_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["id"],
        "properties": {
            "id": {"type": "string", "description": "Session scheduled task id to delete."}
        }
    })
}

/// Model-visible description of the `exec_command` tool. The escalation
/// justification requirement is stated once, on the `justification` field.
pub const EXEC_COMMAND_DESCRIPTION: &str = "Run a shell command through the active sandbox policy and permission checks. Put normal shell tools such as ls, rg, find, cat, sed, awk, build tools, and test tools in cmd. Returns output, exit status, and a reusable session id when the command is still running. For file edits, use apply_patch instead of shell redirection or in-place editors such as `sed -i`. Expanded sandbox_permissions modes trigger an approval check before the command runs.";

#[must_use]
pub fn exec_command_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["cmd"],
        "properties": {
            "cmd": {"type": "string", "description": "Shell command to execute, subject to command policy. The tool description lists covered tools."},
            "yield_time_ms": {"type": "integer", "description": "Wait before returning output (ms). If the command is still running, the response includes a session_id for write_stdin. Values above 10000 turn this into a single-call long run: no outer timeout applies and the response returns after the yield window or command exit, whichever is first.", "default": 10000},
            "background": {"type": "boolean", "description": "Start a retained background process and return after a bounded initial output window. At most three live background processes are allowed per VT Code runtime; use the returned session_id with `write_stdin` for the session lifecycle.", "default": false},
            "stdin": {"type": "boolean", "description": "Keep pipe stdin open for later write_stdin input. Defaults to false (EOF); enable only for commands that need input. PTY input is always available.", "default": false},
            "max_output_tokens": {"type": "integer", "minimum": MIN_MAX_OUTPUT_TOKENS, "maximum": MAX_MAX_OUTPUT_TOKENS, "default": DEFAULT_MAX_OUTPUT_TOKENS, "description": "Output token cap. Large or truncated output can return a spool_path; an active session may set spool_complete=false for a readable partial snapshot, while an exited pending spool is withheld until a later wait."},
            "workdir": {"type": "string", "description": "Working directory."},
            "tty": {"type": "boolean", "description": "Run the command in PTY mode for interactive or terminal-sensitive commands.", "default": false},
            "sandbox_permissions": {
                "type": "string",
                "enum": ["use_default", "with_additional_permissions", "require_escalated", "bypass_sandbox"],
                "description": "Sandbox permission mode for this command. Omit it or use `use_default` for the normal sandbox. Non-empty `additional_permissions` is normalized to `with_additional_permissions`. `require_escalated` and `bypass_sandbox` require a non-empty `justification` and cannot be combined with `additional_permissions`.",
                "default": "use_default"
            },
            "additional_permissions": {
                "type": "object",
                "description": "Optional extra filesystem roots to grant inside the sandbox. Non-empty `additional_permissions` implicitly requests `with_additional_permissions`; every path is normalized and must stay within allowed workspace or temp roots.",
                "properties": {
                    "fs_read": {"type": "array", "items": {"type": "string"}},
                    "fs_write": {"type": "array", "items": {"type": "string"}}
                },
                "additionalProperties": false
            },
            "justification": {"type": "string", "description": "Short approval question for expanded sandbox scope. Required when sandbox_permissions is `require_escalated` or `bypass_sandbox`."}
        },
        "additionalProperties": false
    })
}

/// Shared model-facing description for execution session controls.
pub const WRITE_STDIN_DESCRIPTION: &str = "Control an owned exec session using its exact session_id (copy verbatim from next_wait_args/next_continue_args; never guess): action=\"write\" sends stdin (pipe runs require stdin:true at launch), action=\"wait\" blocks until exit or wait_timeout_seconds (preferred over polling for long runs; never kills), action=\"poll\" returns the latest output immediately, action=\"inspect\" reads a bounded snapshot, action=\"terminate\" kills the process group, action=\"close\" releases the session.";

#[must_use]
pub fn write_stdin_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["session_id"],
        "properties": {
            "session_id": {"type": "string", "description": "Active execution session id copied verbatim from the run response."},
            "action": {"type": "string", "enum": ["write", "poll", "wait", "inspect", "terminate", "close"], "description": "write sends chars to stdin; wait blocks until exit or wait_timeout_seconds without killing (preferred for long runs); poll returns latest output immediately and sends no input; inspect reads a bounded snapshot; terminate kills the process group and captures output; close cancels and releases the session."},
            "chars": {"type": "string", "description": "Bytes to write to stdin; only for action=\"write\". Omit only when action is wait/poll/inspect/terminate/close; an empty string sends no input (polls)."},
            "yield_time_ms": {"type": "integer", "description": "Wait before returning fresh session output (ms).", "default": 1000},
            "wait_timeout_seconds": {"type": "integer", "minimum": 1, "description": "Deadline for action=\"wait\" in seconds. A deadline-expired wait returns an in-progress session; call wait again with the same session_id. wait/inspect are exempt from the per-turn tool-call budget."},
            "max_output_tokens": {"type": "integer", "minimum": MIN_MAX_OUTPUT_TOKENS, "maximum": MAX_MAX_OUTPUT_TOKENS, "default": DEFAULT_MAX_OUTPUT_TOKENS, "description": "Output token cap for the continuation response. Large or truncated output can return a spool_path; the response reports whether an active session has finished writing it."}
        },
        "anyOf": [
            {"required": ["chars"]},
            {"required": ["action"], "properties": {"action": {"enum": ["poll", "wait", "inspect", "terminate", "close"]}}}
        ],
        "additionalProperties": false
    })
}

/// Model-visible description of the `search_tools` tool.
pub const SEARCH_TOOLS_DESCRIPTION: &str = "Search the session tool catalog by capability and return ranked matches. Use it to find tools whose definitions are deferred and not yet sent to you, such as code_search, web_fetch, web_search, cron, and MCP server tools. Deferred matches are listed in `expanded_for_next_segment` and become callable on the next request. It is not needed for tools already defined in the current request; call those directly.";

#[must_use]
pub fn search_tools_parameters() -> Value {
    with_max_output_tokens_parameter(json!({
        "type": "object",
        "required": ["query"],
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "description": "Natural-language description of the capability to discover."
            },
            "limit": {
                "type": "integer",
                "minimum": 1,
                "maximum": 25,
                "default": 5,
                "description": "Maximum number of ranked matches to return (default: 5, max: 25)."
            },
            "detail_level": {
                "type": "string",
                "enum": ["name", "name_description", "full"],
                "default": "name_description",
                "description": "Fields returned per match (default: name_description). name returns the tool name and score; name_description adds the description; full also adds the parameter schema."
            }
        },
        "additionalProperties": false
    }))
}

#[must_use]
pub fn code_search_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["query"],
        "additionalProperties": false,
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "pattern": "\\S",
                "description": "Literal code or path query. Smart-case applies to content and exact symbol-name matching: a wholly lower-case query matches case-insensitively, while an upper-case character makes matching case-sensitive. Path matching remains fuzzy and case-insensitive."
            },
            "path": {
                "type": "string",
                "minLength": 1,
                "pattern": "\\S",
                "description": "Workspace-relative file or directory to search. Omit to search the workspace root."
            },
            "file_types": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "string",
                    "minLength": 1,
                    "pattern": "\\S"
                },
                "description": "Language names or common file extensions, with or without one leading dot."
            },
            "result_types": {
                "type": "array",
                "minItems": 1,
                "items": {
                    "type": "string",
                    "enum": ["definition", "usage", "text", "path"]
                },
                "description": "Result categories to include. Omit to include all four categories."
            },
            "max_results": {
                "type": "integer",
                "minimum": 1,
                "maximum": 100,
                "description": "Maximum number of merged results to return. Omit for 20."
            }
        }
    })
}

#[must_use]
pub fn list_files_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "Directory or file path to inspect.", "default": "."},
            "mode": {
                "type": "string",
                "enum": ["list", "recursive", "tree", "find_name", "find_content", "largest", "file", "files"],
                "description": "Listing mode. Use page/per_page to continue paginated results.",
                "default": "list"
            },
            "pattern": {"type": "string", "description": "Optional glob-style path filter."},
            "name_pattern": {"type": "string", "description": "Optional name filter for list/find_name modes."},
            "content_pattern": {"type": "string", "description": "Content query for find_content mode."},
            "page": {"type": "integer", "description": "1-indexed results page.", "minimum": 1},
            "per_page": {"type": "integer", "description": "Items per page.", "minimum": 1},
            "max_results": {"type": "integer", "description": "Maximum total results to consider before pagination.", "minimum": 1},
            "include_hidden": {"type": "boolean", "description": "Include dotfiles and hidden entries.", "default": false},
            "response_format": {"type": "string", "enum": ["concise", "detailed"], "description": "Verbosity of the listing output.", "default": "concise"},
            "case_sensitive": {"type": "boolean", "description": "Case-sensitive name matching.", "default": false}
        }
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn apply_patch_parameter_schema_keeps_alias_and_guidance_consistent() {
        let schema = apply_patch_parameter_schema("Patch in VT Code format");

        // Both `input` and `patch` alias fields now carry the format
        // description AND the semantic-anchor guidance, preventing the model
        // from placing a unified diff in `patch` (see checkpoint turn_615).
        assert_eq!(
            schema["properties"]["patch"]["description"],
            with_semantic_anchor_guidance(APPLY_PATCH_ALIAS_DESCRIPTION)
        );
        let patch_description = schema["properties"]["patch"]["description"]
            .as_str()
            .expect("patch description");
        assert!(patch_description.contains("*** Begin Patch"));
        assert!(patch_description.contains("unified diff"));
        assert!(patch_description.contains(SEMANTIC_ANCHOR_GUIDANCE));

        let input_description = schema["properties"]["input"]["description"]
            .as_str()
            .expect("input description");
        assert!(input_description.contains(SEMANTIC_ANCHOR_GUIDANCE));
    }

    #[test]
    fn search_tools_schema_documents_limit_and_detail_level_defaults() {
        let schema = search_tools_parameters();
        let limit = &schema["properties"]["limit"];
        assert_eq!(limit["default"], json!(5));
        assert_eq!(limit["maximum"], json!(25));
        let limit_description = limit["description"].as_str().expect("limit description");
        assert!(limit_description.contains("default: 5"));
        assert!(limit_description.contains("max: 25"));

        let detail_level = &schema["properties"]["detail_level"];
        assert_eq!(detail_level["default"], json!("name_description"));
        let detail_description = detail_level["description"].as_str().expect("detail_level description");
        for level in ["name", "name_description", "full"] {
            assert!(detail_description.contains(level), "{level}");
        }

        assert!(SEARCH_TOOLS_DESCRIPTION.contains("deferred"));
        assert!(SEARCH_TOOLS_DESCRIPTION.contains("next request"));
        assert!(SEARCH_TOOLS_DESCRIPTION.contains("MCP"));
        assert!(SEARCH_TOOLS_DESCRIPTION.contains("not needed"));
    }

    #[test]
    fn apply_patch_tool_description_leads_with_format_and_stays_calm() {
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.starts_with("Apply a patch in VT Code format (*** Begin Patch"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("unified diffs"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("workspace-relative"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("permission checks"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("JSON calls use input (patch is an alias)"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("preserving internal whitespace"));
        assert!(APPLY_PATCH_TOOL_DESCRIPTION.contains("one fresh bounded file read range"));
        for description in [
            APPLY_PATCH_TOOL_DESCRIPTION,
            APPLY_PATCH_ALIAS_DESCRIPTION,
            DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION,
        ] {
            assert!(!description.contains("IMPORTANT"), "{description}");
            assert!(!description.contains("NOT"), "{description}");
            assert!(!description.contains("never"), "{description}");
        }
    }

    #[test]
    fn common_output_limit_parameter_preserves_strict_schema() {
        let schema = with_max_output_tokens_parameter(json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "additionalProperties": false
        }));
        assert_eq!(schema["additionalProperties"], json!(false));
        assert_eq!(schema["properties"]["max_output_tokens"]["default"], json!(DEFAULT_MAX_OUTPUT_TOKENS));
        assert_eq!(schema["properties"]["max_output_tokens"]["minimum"], json!(MIN_MAX_OUTPUT_TOKENS));
        assert_eq!(schema["properties"]["max_output_tokens"]["maximum"], json!(MAX_MAX_OUTPUT_TOKENS));
    }

    #[test]
    fn codex_baseline_exec_schemas_use_public_names_shape() {
        let exec_params = exec_command_parameters();
        assert_eq!(exec_params["required"], json!(["cmd"]));
        assert!(exec_params["properties"]["cmd"].is_object());
        assert!(exec_params["properties"]["workdir"].is_object());
        assert!(
            exec_params["properties"]["yield_time_ms"]["description"]
                .as_str()
                .expect("exec yield description")
                .contains("session_id")
        );
        assert!(
            exec_params["properties"]["yield_time_ms"]["description"]
                .as_str()
                .expect("exec yield description")
                .contains("single-call long run")
        );
        assert!(
            exec_params["properties"]["max_output_tokens"]["description"]
                .as_str()
                .expect("exec max output description")
                .contains("spool_path")
        );
        assert_eq!(exec_params["properties"]["tty"]["type"], "boolean");
        assert_eq!(exec_params["properties"]["tty"]["default"], false);
        assert_eq!(exec_params["properties"]["background"]["type"], "boolean");
        assert_eq!(exec_params["properties"]["background"]["default"], false);
        assert_eq!(exec_params["properties"]["yield_time_ms"]["default"], 10000);
        assert_eq!(
            exec_params["properties"]["sandbox_permissions"]["enum"],
            json!([
                "use_default",
                "with_additional_permissions",
                "require_escalated",
                "bypass_sandbox"
            ])
        );
        assert_eq!(exec_params["properties"]["sandbox_permissions"]["default"], "use_default");
        assert_eq!(
            exec_params["properties"]["additional_permissions"]["properties"]["fs_read"]["items"]["type"],
            "string"
        );
        assert_eq!(
            exec_params["properties"]["additional_permissions"]["properties"]["fs_write"]["items"]["type"],
            "string"
        );
        assert_eq!(exec_params["properties"]["additional_permissions"]["additionalProperties"], false);
        assert_eq!(exec_params["properties"]["justification"]["type"], "string");
        assert!(
            exec_params["properties"]["sandbox_permissions"]["description"]
                .as_str()
                .expect("sandbox_permissions description")
                .contains("normalized to `with_additional_permissions`")
        );
        assert!(
            exec_params["properties"]["additional_permissions"]["description"]
                .as_str()
                .expect("additional_permissions description")
                .contains("allowed workspace or temp roots")
        );
        assert!(
            exec_params["properties"]["justification"]["description"]
                .as_str()
                .expect("justification description")
                .contains("Required when sandbox_permissions is `require_escalated` or `bypass_sandbox`")
        );
        assert_eq!(exec_params["additionalProperties"], false);
        // Example commands live once, in EXEC_COMMAND_DESCRIPTION; the `cmd`
        // property defers to it instead of repeating the list on every
        // request.
        assert!(
            exec_params["properties"]["cmd"]["description"]
                .as_str()
                .expect("cmd description")
                .contains("tool description lists covered tools"),
            "cmd description must defer the example list to the tool description"
        );
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                EXEC_COMMAND_DESCRIPTION.contains(command),
                "{command} should be described as an exec_command example"
            );
            assert!(
                exec_params["properties"].get(command).is_none(),
                "{command} must not be modelled as a separate exec_command field"
            );
        }

        let stdin_params = write_stdin_parameters();
        assert_eq!(stdin_params["required"], json!(["session_id"]));
        assert!(stdin_params["properties"]["session_id"].is_object());
        assert_eq!(stdin_params["properties"]["chars"]["type"], "string");
        assert_eq!(
            stdin_params["properties"]["action"]["enum"],
            json!(["write", "poll", "wait", "inspect", "terminate", "close"])
        );
        assert_eq!(exec_params["properties"]["stdin"]["default"], false);
        assert!(stdin_params["properties"]["wait_timeout_seconds"].is_object());
        assert!(
            stdin_params["properties"].get("timeout_seconds").is_none(),
            "write_stdin schema advertises only wait_timeout_seconds"
        );
        assert_eq!(stdin_params["anyOf"][1]["required"], json!(["action"]));
        assert_eq!(
            stdin_params["anyOf"][1]["properties"]["action"]["enum"],
            json!(["poll", "wait", "inspect", "terminate", "close"])
        );
        assert!(
            stdin_params["properties"]["chars"]["description"]
                .as_str()
                .is_some_and(|description| description.contains("empty string"))
        );
        assert!(stdin_params["properties"]["chars"].is_object());
        assert!(
            stdin_params["properties"]["yield_time_ms"]["description"]
                .as_str()
                .expect("stdin yield description")
                .contains("fresh session output")
        );
        assert!(
            stdin_params["properties"]["max_output_tokens"]["description"]
                .as_str()
                .expect("stdin max output description")
                .contains("spool_path")
        );
        assert_eq!(stdin_params["additionalProperties"], false);
    }

    #[test]
    fn mcp_description_distinguishes_server_search_from_catalog_search() {
        assert!(MCP_DESCRIPTION.starts_with("Discover and manage Model Context Protocol capabilities."));
        assert!(MCP_DESCRIPTION.contains("action=search_tools searches only tools exposed by configured MCP servers"));
        assert!(MCP_DESCRIPTION.contains("the separate search_tools tool searches the whole session catalog"));
    }

    #[test]
    fn agent_description_routes_shell_processes_to_exec_command() {
        assert!(AGENT_DESCRIPTION.starts_with("Spawn and steer delegated child agents."));
        assert!(AGENT_DESCRIPTION.contains("spawn_subprocess runs a subagent defined with background: true"));
        assert!(AGENT_DESCRIPTION.contains("go through exec_command, with background=true"));
        let action = agent_parameters()["properties"]["action"]["description"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        assert!(!action.contains("daemons"), "{action}");
    }

    #[test]
    fn exec_command_description_states_edit_routing_and_escalation_rules() {
        assert!(EXEC_COMMAND_DESCRIPTION.starts_with("Run a shell command through the active sandbox policy"));
        assert!(EXEC_COMMAND_DESCRIPTION.contains("For file edits, use apply_patch"));
        assert!(EXEC_COMMAND_DESCRIPTION.contains("approval check"));
        // The justification requirement is stated once, on the `justification`
        // field description; the tool description keeps only the approval-check
        // routing rule instead of repeating the requirement on every request.
        assert!(
            !EXEC_COMMAND_DESCRIPTION.contains("non-empty justification"),
            "justification requirement belongs to the field description"
        );
        for mode in ["require_escalated", "bypass_sandbox"] {
            assert!(
                exec_command_parameters()["properties"]["sandbox_permissions"]["enum"]
                    .as_array()
                    .expect("sandbox_permissions enum")
                    .iter()
                    .any(|value| value == mode),
                "{mode} must stay a real sandbox_permissions value"
            );
        }
    }

    #[test]
    fn code_search_schema_exposes_exact_five_property_contract() {
        let params = code_search_parameters();
        let properties = params["properties"].as_object().expect("properties");
        let mut property_names = properties.keys().map(String::as_str).collect::<Vec<_>>();
        property_names.sort_unstable();

        assert_eq!(params["required"], json!(["query"]));
        assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);
        assert_eq!(params["additionalProperties"], false);
        assert_eq!(params["properties"]["query"]["pattern"], "\\S");
        assert_eq!(params["properties"]["file_types"]["minItems"], 1);
        assert_eq!(params["properties"]["result_types"]["minItems"], 1);
        assert_eq!(
            params["properties"]["result_types"]["items"]["enum"],
            json!(["definition", "usage", "text", "path"])
        );
        assert_eq!(params["properties"]["max_results"]["minimum"], 1);
        assert_eq!(params["properties"]["max_results"]["maximum"], 100);
        assert!(params.get("anyOf").is_none());
    }

    #[test]
    fn legacy_list_files_schema_exposes_pagination_fields() {
        let list_params = list_files_parameters();
        assert!(list_params["properties"]["page"].is_object());
        assert!(list_params["properties"]["per_page"].is_object());
        assert!(
            list_params["properties"]["mode"]["enum"]
                .as_array()
                .expect("mode enum")
                .iter()
                .any(|value| value == "recursive")
        );
    }

    #[test]
    fn semantic_anchor_guidance_is_appended_once() {
        let base = "Patch in VT Code format.";
        let with_guidance = with_semantic_anchor_guidance(base);

        assert!(with_guidance.contains(SEMANTIC_ANCHOR_GUIDANCE));
        assert_eq!(with_semantic_anchor_guidance(&with_guidance), with_guidance);
    }

    #[test]
    fn default_apply_patch_parameters_keep_expected_alias_shape() {
        let schema = apply_patch_parameters();

        assert_eq!(
            schema["anyOf"],
            json!([
                {"required": ["input"]},
                {"required": ["patch"]}
            ])
        );
    }
}
