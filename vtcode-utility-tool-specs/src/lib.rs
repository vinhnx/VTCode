//! Passive JSON schemas for utility, file, and scheduling tool surfaces.

use serde_json::{Value, json};

pub const SEMANTIC_ANCHOR_GUIDANCE: &str =
    "Prefer stable semantic @@ anchors such as function, class, method, or impl names.";
pub const APPLY_PATCH_ALIAS_DESCRIPTION: &str = "Alias for input";
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
                "description": APPLY_PATCH_ALIAS_DESCRIPTION
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
        "properties": {}
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
pub fn unified_exec_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "description": "Command as a shell string or argv array.",
                "anyOf": [
                    {"type": "string"},
                    {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                ]
            },
            "input": {"type": "string", "description": "stdin for write or continue."},
            "session_id": {"type": "string", "description": "Session id. Compact alias: `s`."},
            "spool_path": {"type": "string", "description": "Spool path for inspect."},
            "query": {"type": "string", "description": "Line filter for inspect or run output."},
            "head_lines": {"type": "integer", "description": "Head preview lines."},
            "tail_lines": {"type": "integer", "description": "Tail preview lines."},
            "max_matches": {"type": "integer", "description": "Max filtered matches.", "default": 200},
            "literal": {"type": "boolean", "description": "Treat query as literal text.", "default": false},
            "code": {"type": "string", "description": "Raw Python or JavaScript source for `action=code`. Send the source directly, not JSON or markdown fences."},
            "language": {
                "type": "string",
                "enum": ["python3", "javascript"],
                "description": "Language for `action=code`. Defaults to `python3`; set `javascript` to run Node-based code execution instead.",
                "default": "python3"
            },
            "action": {
                "type": "string",
                "enum": ["run", "write", "poll", "continue", "inspect", "list", "close", "code"],
                "description": "Optional; inferred from command/code/input/session_id/spool_path. Use `code` to run a fresh Python or JavaScript snippet through the local code executor."
            },
            "workdir": {"type": "string", "description": "Working directory."},
            "cwd": {"type": "string", "description": "Alias for workdir."},
            "tty": {"type": "boolean", "description": "Use PTY mode.", "default": false},
            "shell": {"type": "string", "description": "Shell binary."},
            "login": {"type": "boolean", "description": "Use a login shell.", "default": false},
            "sandbox_permissions": {
                "type": "string",
                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                "description": "Sandbox mode. Use `require_escalated` only when needed."
            },
            "additional_permissions": {
                "type": "object",
                "description": "Extra sandboxed filesystem access.",
                "properties": {
                    "fs_read": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Extra readable paths."
                    },
                    "fs_write": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Extra writable paths."
                    }
                },
                "additionalProperties": false
            },
            "justification": {"type": "string", "description": "Approval question for `require_escalated`."},
            "prefix_rule": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Optional persisted approval prefix for `command`."
            },
            "timeout_secs": {"type": "integer", "description": "Timeout seconds.", "default": 180},
            "yield_time_ms": {"type": "integer", "description": "Wait before returning output (ms).", "default": 1000},
            "confirm": {"type": "boolean", "description": "Confirm destructive ops.", "default": false},
            "max_output_tokens": {"type": "integer", "description": "Output token cap."},
            "track_files": {"type": "boolean", "description": "Track file changes.", "default": false}
        }
    })
}

#[must_use]
pub fn unified_file_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["read", "write", "edit", "patch", "delete", "move", "copy"],
                "description": "Optional; inferred from old_str/patch/content/destination/path."
            },
            "path": {"type": "string", "description": "File path. Compact alias: `p`."},
            "content": {"type": "string", "description": "Content for write."},
            "old_str": {"type": "string", "description": "Exact text to replace for edit."},
            "new_str": {"type": "string", "description": "Replacement text for edit."},
            "patch": {"type": "string", "description": "Patch text in `*** Update File:` format, not unified diff."},
            "destination": {"type": "string", "description": "Destination for move or copy."},
            "start_line": {"type": "integer", "description": "Read start line (1-indexed)."},
            "end_line": {"type": "integer", "description": "Read end line (inclusive)."},
            "offset": {"type": "integer", "description": "Read start line. Compact alias: `o`."},
            "limit": {"type": "integer", "description": "Read line count. Compact alias: `l`."},
            "mode": {"type": "string", "description": "Read mode or write mode.", "default": "slice"},
            "condense": {"type": "boolean", "description": "Condense long output.", "default": true},
            "indentation": {
                "description": "Indentation config. `true` uses defaults.",
                "anyOf": [
                    {"type": "boolean"},
                    {
                        "type": "object",
                        "properties": {
                            "anchor_line": {"type": "integer", "description": "Anchor line; defaults to offset."},
                            "max_levels": {"type": "integer", "description": "Indent depth cap; 0 means unlimited."},
                            "include_siblings": {"type": "boolean", "description": "Include sibling blocks."},
                            "include_header": {"type": "boolean", "description": "Include header lines."},
                            "max_lines": {"type": "integer", "description": "Optional line cap."}
                        },
                        "additionalProperties": false
                    }
                ]
            }
        }
    })
}

#[must_use]
pub fn read_file_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "description": "File path. Accepts file_path/filepath/target_path/p."},
            "offset": {"type": "integer", "description": "1-indexed line offset. Compact alias: `o`.", "minimum": 1},
            "limit": {"type": "integer", "description": "Max lines for this chunk. Compact alias: `l`.", "minimum": 1},
            "mode": {"type": "string", "enum": ["slice", "indentation"], "description": "Read mode.", "default": "slice"},
            "indentation": {
                "description": "Indentation-aware block selection.",
                "anyOf": [
                    {"type": "boolean"},
                    {
                        "type": "object",
                        "properties": {
                            "anchor_line": {"type": "integer", "description": "Anchor line; defaults to offset."},
                            "max_levels": {"type": "integer", "description": "Indent depth cap; 0 means unlimited."},
                            "include_siblings": {"type": "boolean", "description": "Include sibling blocks."},
                            "include_header": {"type": "boolean", "description": "Include header lines."},
                            "max_lines": {"type": "integer", "description": "Optional line cap."}
                        },
                        "additionalProperties": false
                    }
                ]
            },
            "offset_lines": {"type": "integer", "description": "Legacy alias for line offset.", "minimum": 1},
            "page_size_lines": {"type": "integer", "description": "Legacy alias for line chunk size.", "minimum": 1},
            "offset_bytes": {"type": "integer", "description": "Byte offset for binary or byte-paged reads.", "minimum": 0},
            "page_size_bytes": {"type": "integer", "description": "Byte page size for binary or byte-paged reads.", "minimum": 1},
            "max_bytes": {"type": "integer", "description": "Maximum bytes to return.", "minimum": 1},
            "max_lines": {"type": "integer", "description": "Maximum lines to return in legacy mode.", "minimum": 1},
            "chunk_lines": {"type": "integer", "description": "Legacy alias for chunk size in lines.", "minimum": 1},
            "max_tokens": {"type": "integer", "description": "Optional token budget for large reads.", "minimum": 1},
            "condense": {"type": "boolean", "description": "Condense long outputs to head/tail.", "default": true}
        }
    })
}

#[must_use]
pub fn unified_search_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["grep", "list", "structural", "tools", "errors", "agent", "web", "skill"],
                "description": "Action to perform. Default to `structural` for code or syntax-aware search, including read-only ast-grep `run` query, project scan, and project test workflows; use `grep` for raw text and `list` for file discovery. Refine and retry `grep` or `structural` here before switching tools."
            },
            "workflow": {
                "type": "string",
                "enum": ["query", "scan", "test"],
                "description": "Structural workflow. `query` is the default parseable-pattern search and maps to read-only ast-grep `run`; `scan` maps to read-only ast-grep `scan` from config, and `test` runs ast-grep rule tests.",
                "default": "query"
            },
            "pattern": {"type": "string", "description": "For `grep` or `errors`, regex or literal text. For `list`, a glob filter for returned paths or names; nested globs such as `**/*.rs` promote `list` to recursive discovery. For `structural` `workflow=\"query\"`, valid parseable code for the selected language using ast-grep pattern syntax, not a raw code fragment; `$VAR` matches one named node, `$$$ARGS` matches zero or more nodes, `$$VAR` includes unnamed nodes, and `$_` suppresses capture. If a fragment fails, retry `action='structural'` with a larger parseable pattern such as a full function signature."},
            "path": {"type": "string", "description": "Directory or file path to search in. Used by `grep`, `list`, and structural `workflow=\"query\"|\"scan\"`. Public structural calls take one root per request even though raw ast-grep `run` can accept multiple paths.", "default": "."},
            "config_path": {"type": "string", "description": "Ast-grep config path for structural `workflow=\"scan\"` or `workflow=\"test\"`. Defaults to workspace `sgconfig.yml`."},
            "filter": {"type": "string", "description": "Ast-grep rule or test filter for structural `workflow=\"scan\"` or `workflow=\"test\"`. On `scan`, this maps to `--filter` over rule ids from config."},
            "lang": {"type": "string", "description": "Language for structural `workflow=\"query\"`. Set it whenever the code language is known; required for debug_query."},
            "selector": {"type": "string", "description": "Ast-grep selector for structural `workflow=\"query\"` when the real match is a subnode inside the parseable pattern."},
            "strictness": {
                "type": "string",
                "enum": ["cst", "smart", "ast", "relaxed", "signature", "template"],
                "description": "Pattern strictness for structural `workflow=\"query\"`."
            },
            "debug_query": {
                "type": "string",
                "enum": ["pattern", "ast", "cst", "sexp"],
                "description": "Print the structural query AST instead of matches for `workflow=\"query\"`. Requires lang."
            },
            "globs": {
                "description": "Optional include/exclude globs for structural `workflow=\"query\"` or `workflow=\"scan\"`. Maps to repeated ast-grep `--globs` flags.",
                "anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "skip_snapshot_tests": {"type": "boolean", "description": "Skip ast-grep snapshot tests for structural `workflow=\"test\"`.", "default": false},
            "keyword": {"type": "string", "description": "Keyword for 'tools' search."},
            "url": {"type": "string", "format": "uri", "description": "The URL to fetch content from (for 'web' action)."},
            "prompt": {"type": "string", "description": "The prompt to run on the fetched content (for 'web' action)."},
            "name": {"type": "string", "description": "Skill name to load (for 'skill' action)."},
            "detail_level": {
                "type": "string",
                "enum": ["name-only", "name-and-description", "full"],
                "description": "Detail level for 'tools' action.",
                "default": "name-and-description"
            },
            "mode": {
                "type": "string",
                "description": "Mode for 'list' (list|recursive|tree|etc) or 'agent' (debug|analyze|full) action.",
                "default": "list"
            },
            "max_results": {"type": "integer", "description": "Max results to return.", "default": 100},
            "case_sensitive": {"type": "boolean", "description": "Case-sensitive search.", "default": false},
            "context_lines": {"type": "integer", "description": "Context lines for `grep` or structural `workflow=\"query\"|\"scan\"` results. Structural maps this to ast-grep `--context`; raw `--before` and `--after` are not exposed separately.", "default": 0},
            "scope": {"type": "string", "description": "Scope for 'errors' action (archive|all).", "default": "archive"},
            "max_bytes": {"type": "integer", "description": "Maximum bytes to fetch for 'web' action.", "default": 500000},
            "timeout_secs": {"type": "integer", "description": "Timeout in seconds.", "default": 30}
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

        assert_eq!(
            schema["properties"]["patch"]["description"],
            APPLY_PATCH_ALIAS_DESCRIPTION
        );
        let input_description = schema["properties"]["input"]["description"]
            .as_str()
            .expect("input description");
        assert!(input_description.contains(SEMANTIC_ANCHOR_GUIDANCE));
    }

    #[test]
    fn unified_exec_schema_accepts_string_or_array_commands() {
        let params = unified_exec_parameters();
        let command = &params["properties"]["command"];
        let variants = command["anyOf"].as_array().expect("command anyOf");

        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0]["type"], "string");
        assert_eq!(variants[1]["type"], "array");
        assert_eq!(variants[1]["items"]["type"], "string");
        assert_eq!(params["properties"]["tty"]["type"], "boolean");
        assert_eq!(params["properties"]["tty"]["default"], false);
        assert!(
            params["properties"]["code"]["description"]
                .as_str()
                .expect("code description")
                .contains("Raw Python or JavaScript source")
        );
        assert!(
            params["properties"]["language"]["description"]
                .as_str()
                .expect("language description")
                .contains("set `javascript`")
        );
    }

    #[test]
    fn unified_search_schema_advertises_structural_and_hides_intelligence() {
        let params = unified_search_parameters();
        let actions = params["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");

        assert!(actions.iter().any(|value| value == "structural"));
        assert!(!actions.iter().any(|value| value == "intelligence"));
        assert!(
            params["properties"]["debug_query"]["enum"]
                .as_array()
                .expect("debug_query enum")
                .iter()
                .any(|value| value == "ast")
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("Default to `structural`")
        );
        assert!(
            params["properties"]["pattern"]["description"]
                .as_str()
                .expect("pattern description")
                .contains("valid parseable code")
        );
        assert!(
            params["properties"]["pattern"]["description"]
                .as_str()
                .expect("pattern description")
                .contains("$$$ARGS")
        );
        assert!(
            params["properties"]["pattern"]["description"]
                .as_str()
                .expect("pattern description")
                .contains("glob filter")
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("Refine and retry `grep` or `structural`")
        );
        assert_eq!(params["properties"]["workflow"]["enum"][1], "scan");
        assert_eq!(params["properties"]["workflow"]["enum"][2], "test");
        assert!(
            params["properties"]["config_path"]["description"]
                .as_str()
                .expect("config path description")
                .contains("Defaults to workspace `sgconfig.yml`")
        );
        assert!(
            params["properties"]["skip_snapshot_tests"]["description"]
                .as_str()
                .expect("skip snapshot description")
                .contains("workflow=\"test\"")
        );
    }

    #[test]
    fn legacy_browse_tool_schemas_expose_chunking_and_pagination_fields() {
        let read_params = read_file_parameters();
        assert!(read_params["properties"]["offset"].is_object());
        assert!(read_params["properties"]["limit"].is_object());
        assert!(read_params["properties"]["page_size_lines"].is_object());

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
