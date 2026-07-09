#![allow(missing_docs)]
//! Passive JSON schemas for utility, file, scheduling, and collaboration tool surfaces.

#![recursion_limit = "256"]

use serde_json::{Value, json};

mod collaboration;
mod json_schema;
#[cfg(feature = "mcp")]
mod mcp_tool;
mod responses_api;

pub use collaboration::{
    close_agent_parameters, request_user_input_description, request_user_input_parameters,
    resume_agent_parameters, send_input_parameters, spawn_agent_parameters,
    spawn_background_subprocess_parameters, wait_agent_parameters,
};
pub use json_schema::{AdditionalProperties, JsonSchema, parse_tool_input_schema};
#[cfg(feature = "mcp")]
pub use mcp_tool::{ParsedMcpTool, parse_mcp_tool};
pub use responses_api::{FreeformTool, FreeformToolFormat, ResponsesApiTool};

pub const SEMANTIC_ANCHOR_GUIDANCE: &str =
    "Prefer stable semantic @@ anchors such as function, class, method, or impl names.";

/// Explicit, format-bearing description for the `patch` alias field. The old
/// value ("Alias for input") gave the model no format guidance, so it often
/// placed a standard unified diff (`---`/`+++`) there — which `apply_patch`
/// rejects. This mirrors the `input` description so both alias fields carry
/// identical, complete format guidance (see checkpoint turn_615 for the
/// failure this prevents).
pub const APPLY_PATCH_ALIAS_DESCRIPTION: &str = "Patch in VT Code format (*** Begin Patch, *** Update File: path, @@ hunk, -/+ lines, *** End Patch). Same envelope as 'input'; do NOT use unified diff (--- /+++ format).";
pub const DEFAULT_APPLY_PATCH_INPUT_DESCRIPTION: &str = "Patch in VT Code format: *** Begin Patch, *** Update File: path, @@ hunk, -/+ lines, *** End Patch";

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

#[must_use]
pub fn exec_command_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["cmd"],
        "properties": {
            "cmd": {"type": "string", "description": "Shell command to execute, subject to command policy. Examples include `ls`, `rg`, `find`, `cat`, `sed`, `awk`, build tools, and test tools."},
            "yield_time_ms": {"type": "integer", "description": "Wait before returning output (ms). If the command is still running, the response includes a session_id for write_stdin.", "default": 1000},
            "max_output_tokens": {"type": "integer", "description": "Output token cap. Large or truncated output can return a spool_path for the full output."},
            "workdir": {"type": "string", "description": "Working directory."},
            "tty": {"type": "boolean", "description": "Run the command in PTY mode for interactive or terminal-sensitive commands.", "default": false}
        },
        "additionalProperties": false
    })
}

#[must_use]
pub fn write_stdin_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["session_id", "chars"],
        "properties": {
            "session_id": {"type": "string", "description": "Active execution session id."},
            "chars": {"type": "string", "description": "Bytes to write to stdin."},
            "yield_time_ms": {"type": "integer", "description": "Wait before returning fresh session output (ms).", "default": 1000},
            "max_output_tokens": {"type": "integer", "description": "Output token cap for the continuation response. Large or truncated output can return a spool_path for the full output."}
        },
        "additionalProperties": false
    })
}

#[must_use]
pub fn code_search_parameters() -> Value {
    json!({
        "type": "object",
        "required": ["action"],
        "additionalProperties": false,
        "properties": {
            "action": {
                "type": "string",
                "enum": ["structural", "outline"],
                "description": "Semantic code search action: structural for ast-grep pattern search, or outline for Tree-sitter symbol maps. Use exec_command.cmd with rg for plain text search."
            },
            "workflow": {
                "type": "string",
                "enum": ["query", "scan", "test"],
                "description": "Structural workflow for ast-grep.",
                "default": "query"
            },
            "pattern": {"type": "string", "description": "Ast-grep pattern such as $VAR or $$$ARGS for structural search."},
            "kind": {"type": "string", "description": "Ast-grep node kind, such as function_item or call_expression."},
            "path": {"type": "string", "description": "Directory or file path to search or outline.", "default": "."},
            "config_path": {"type": "string", "description": "Ast-grep config path for scan or test workflows. Defaults to workspace sgconfig.yml."},
            "filter": {"type": "string", "description": "Ast-grep rule or test filter for scan or test workflows."},
            "lang": {"type": "string", "description": "Language for structural search or outline. Set this when the language is known."},
            "selector": {"type": "string", "description": "Ast-grep selector when the match is a subnode."},
            "strictness": {
                "type": "string",
                "enum": ["cst", "smart", "ast", "relaxed", "signature", "template"],
                "description": "Pattern strictness for structural query workflow."
            },
            "view": {
                "type": "string",
                "enum": ["digest", "names", "full"],
                "description": "Output shape for outline results.",
                "default": "digest"
            },
            "items": {
                "type": "string",
                "enum": ["auto", "structure", "exports", "imports", "all"],
                "description": "Which top-level symbols outline includes.",
                "default": "auto"
            },
            "type": {
                "description": "Symbol types to keep in outline.",
                "anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "match": {"type": "string", "description": "Regex for outline to filter item names, signatures, or first lines."},
            "pub_members": {"type": "boolean", "description": "In outline, show only public members.", "default": false},
            "follow": {"type": "boolean", "description": "Follow symbolic links while traversing directories.", "default": false},
            "debug_query": {
                "type": "string",
                "enum": ["pattern", "ast", "cst", "sexp"],
                "description": "Print the structural query AST instead of matches. Requires lang."
            },
            "globs": {
                "description": "Optional include or exclude globs for structural workflows.",
                "anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "skip_snapshot_tests": {"type": "boolean", "description": "Skip ast-grep snapshot tests for test workflow.", "default": false},
            "max_results": {"type": "integer", "description": "Maximum results to return.", "default": 100},
            "context_lines": {"type": "integer", "description": "Context lines for structural results.", "default": 0},
            "severities": {
                "type": "array",
                "items": {"type": "string", "enum": ["error", "warning", "info", "hint"]},
                "description": "Post-run severity filter for structural scan workflow."
            },
            "no_ignore": {
                "type": "array",
                "items": {"type": "string", "enum": ["hidden", "dot", "exclude", "global", "parent", "vcs"]},
                "description": "Ignore file overrides."
            },
            "threads": {"type": "integer", "description": "Number of threads for ast-grep scan parallelism. 0 means auto.", "minimum": 0, "maximum": 256, "default": 0},
            "format": {"type": "string", "enum": ["github", "sarif", "files_with_matches", "count"], "description": "Output format for structural scan workflow."},
            "report_style": {"type": "string", "enum": ["rich", "medium", "short"], "description": "Diagnostic report style for structural scan workflow."},
            "before_lines": {"type": "integer", "description": "Context lines before each structural match.", "minimum": 0, "maximum": 20},
            "after_lines": {"type": "integer", "description": "Context lines after each structural match.", "minimum": 0, "maximum": 20},
            "builtin_rules": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Built-in ast-grep rules to activate for structural scan workflow."
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
    use super::*;
    use serde_json::json;

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
            exec_params["properties"]["max_output_tokens"]["description"]
                .as_str()
                .expect("exec max output description")
                .contains("spool_path")
        );
        assert_eq!(exec_params["properties"]["tty"]["type"], "boolean");
        assert_eq!(exec_params["properties"]["tty"]["default"], false);
        assert_eq!(exec_params["additionalProperties"], false);
        for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
            assert!(
                exec_params["properties"]["cmd"]["description"]
                    .as_str()
                    .expect("cmd description")
                    .contains(command),
                "{command} should be described as an exec_command.cmd example"
            );
            assert!(
                exec_params["properties"].get(command).is_none(),
                "{command} must not be modelled as a separate exec_command field"
            );
        }

        let stdin_params = write_stdin_parameters();
        assert_eq!(stdin_params["required"], json!(["session_id", "chars"]));
        assert!(stdin_params["properties"]["session_id"].is_object());
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
    fn code_search_schema_exposes_only_code_search_actions() {
        let params = code_search_parameters();
        let actions = params["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");

        assert_eq!(params["required"], json!(["action"]));
        assert_eq!(
            actions.as_slice(),
            json!(["structural", "outline"])
                .as_array()
                .expect("expected array")
        );
        for removed_action in ["grep", "list", "tools", "errors", "agent", "web", "skill"] {
            assert!(
                !actions.iter().any(|value| value == removed_action),
                "{removed_action} must not be a code_search action"
            );
        }
        assert_eq!(params["additionalProperties"], false);
        assert!(params["properties"].get("case_sensitive").is_none());
        assert!(params["properties"]["lang"].is_object());
        assert!(params["properties"]["selector"].is_object());
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("exec_command.cmd with rg")
        );
        let workflows = params["properties"]["workflow"]["enum"]
            .as_array()
            .expect("workflow enum");
        assert!(workflows.iter().any(|value| value == "query"));
        assert!(!workflows.iter().any(|value| value == "rewrite"));
        assert!(!workflows.iter().any(|value| value == "apply"));
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
