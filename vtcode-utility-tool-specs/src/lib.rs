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
                "enum": ["grep", "structural", "outline"],
                "description": "Code search action: grep for text search, structural for ast-grep pattern search, or outline for Tree-sitter symbol maps."
            },
            "workflow": {
                "type": "string",
                "enum": ["query", "scan", "test"],
                "description": "Structural workflow for ast-grep.",
                "default": "query"
            },
            "pattern": {"type": "string", "description": "For grep: regex or literal text. For structural: ast-grep pattern such as $VAR or $$$ARGS."},
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
            "case_sensitive": {"type": "boolean", "description": "Case-sensitive text search.", "default": false},
            "context_lines": {"type": "integer", "description": "Context lines for grep or structural results.", "default": 0},
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
            "workdir": {"type": "string", "description": "Working directory. Alias: cwd."},
            "tty": {"type": "boolean", "description": "Use PTY mode.", "default": false},
            "shell": {"type": "string", "description": "Shell binary."},
            "login": {"type": "boolean", "description": "Use a login shell.", "default": false},
            "sandbox_permissions": {
                "type": "string",
                "enum": ["use_default", "with_additional_permissions", "require_escalated"],
                "description": "Sandbox policy. Use `require_escalated` only when needed."
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
pub fn unified_search_parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["grep", "list", "structural", "outline", "tools", "errors", "agent", "web", "skill"],
                "description": "Search action: grep (text), list (files; paginated, default 20/page — use max_results for more), structural (ast-grep pattern search), outline (symbol map of a file/directory, no pattern needed — preferred for 'what's here?' / repo or directory overview), tools, errors, agent, web, skill. For 'web', provide 'query' to search the web, or 'url' to fetch a specific page."
            },
            "workflow": {
                "type": "string",
                "enum": ["query", "scan", "test", "rewrite", "new", "apply"],
                "description": "Structural workflow: query (search), scan (config rules), test (rule tests), rewrite (preview), apply (write), new (scaffold).",
                "default": "query"
            },
            "pattern": {"type": "string", "description": "For grep: regex/literal. For list: glob filter. For structural: ast-grep pattern ($VAR=node, $$$ARGS=many). At least one of pattern or kind required for structural query."},
            "kind": {"type": "string", "description": "Ast-grep node kind (e.g. function_item, call_expression). Supports >, +, ~, :has(), :not() selectors. Use alone or with pattern."},
            "path": {"type": "string", "description": "Directory or file path to search in. Used by `grep`, `list`, structural `workflow=\"query\"|\"scan\"`, and `outline`. Public structural calls take one root per request even though raw ast-grep `run` can accept multiple paths.", "default": "."},
            "config_path": {"type": "string", "description": "Ast-grep config path for structural `workflow=\"scan\"` or `workflow=\"test\"`. Defaults to workspace `sgconfig.yml`."},
            "filter": {"type": "string", "description": "Ast-grep rule or test filter for structural `workflow=\"scan\"` or `workflow=\"test\"`. On `scan`, this maps to `--filter` over rule ids from config."},
            "lang": {"type": "string", "description": "Language for structural `workflow=\"query\"` or `workflow=\"rewrite\"`, and for `outline`. Set it whenever the code language is known; required for debug_query and recommended for rewrite."},
            "selector": {"type": "string", "description": "Ast-grep selector when match is a subnode. Supports :has(), :not(), :is(), :nth-child()."},
            "strictness": {
                "type": "string",
                "enum": ["cst", "smart", "ast", "relaxed", "signature", "template"],
                "description": "Pattern strictness for structural `workflow=\"query\"`."
            },
            "view": {
                "type": "string",
                "enum": ["digest", "names", "full"],
                "description": "Output shape for `outline`: digest (symbols grouped by kind, default for single-file queries), names (flat name groups, default for directory queries, auto-applied when `view=full` is requested on a large directory to prevent truncation), full (per-symbol records with the raw zero-based `range`, a derived 1-based inclusive `lineRange` usable with shell inspection such as `sed -n`, `astKind`, signatures, and nested members, use on individual files, not large directories). Directory queries also receive a top-level `summary` block with `total_symbols`, `by_kind` (per-kind symbol counts), and a flat `all_symbols` array (capped at 200 entries; `truncated`/`visible_symbols` are set when the cap is hit).",
                "default": "digest"
            },
            "items": {
                "type": "string",
                "enum": ["auto", "structure", "exports", "imports", "all"],
                "description": "Which top-level symbols `outline` includes. `auto` (default) uses structure for file input and exports for directory input.",
                "default": "auto"
            },
            "type": {
                "description": "Comma-separated symbol types to keep in `outline` (e.g. \"function\", [\"class\",\"enum\"]).",
                "anyOf": [
                    {"type": "string"},
                    {"type": "array", "items": {"type": "string"}}
                ]
            },
            "match": {"type": "string", "description": "Regex for `outline` to filter item names/signatures/first lines."},
            "pub_members": {"type": "boolean", "description": "In `outline`, show only public members.", "default": false},
            "follow": {"type": "boolean", "description": "Follow symbolic links while traversing directories. Used by `outline` and structural workflows.", "default": false},
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
            "rewrite": {"type": "string", "description": "Replacement string for structural `workflow=\"rewrite\"`. Meta variables from `pattern` can be referenced (e.g. `$VAR`, `$$$ARGS`). For simple pattern-to-pattern rewrites. Either `rewrite` or `fix_config` is required for `workflow=\"rewrite\"`."},
            "fix_config": {
                "type": "object",
                "description": "Advanced fix configuration for structural `workflow=\"rewrite\"`. Use when replacing only the matched node is not enough, especially for deleting list items or key-value pairs that also need a surrounding comma removed. Either `rewrite` or `fix_config` is required for `workflow=\"rewrite\"`.",
                "properties": {
                    "template": {"type": "string", "description": "Replacement template string. Meta variables from `pattern` can be referenced."},
                    "expand_start": {
                        "type": "object",
                        "description": "Expand fix range start backwards. Requires at least one of: regex, kind, pattern.",
                        "properties": {
                            "regex": {"type": "string", "description": "Regex to match node text."},
                            "kind": {"type": "string", "description": "Tree-sitter node kind."},
                            "pattern": {"type": "string", "description": "Ast-grep pattern."},
                            "stop_by": {"description": "Expansion stop rule. String (\"line\"|\"end\") or rule object."}
                        }
                    },
                    "expand_end": {
                        "type": "object",
                        "description": "Expand fix range end forwards. Requires at least one of: regex, kind, pattern.",
                        "properties": {
                            "regex": {"type": "string", "description": "Regex to match node text."},
                            "kind": {"type": "string", "description": "Tree-sitter node kind."},
                            "pattern": {"type": "string", "description": "Ast-grep pattern."},
                            "stop_by": {"description": "Expansion stop rule. String (\"line\"|\"end\") or rule object."}
                        }
                    }
                },
                "required": ["template"]
            },
            "new_subcommand": {"type": "string", "enum": ["project", "rule", "test", "util"], "description": "Subcommand for structural `workflow=\"new\"`. `project` scaffolds sgconfig.yml and directories; `rule` creates a new rule YAML; `test` creates a new test YAML; `util` creates a new utility rule."},
            "new_name": {"type": "string", "description": "Name for the new rule, test, or utility. Required for `new` subcommands `rule`, `test`, and `util`."},
            "keyword": {"type": "string", "description": "Keyword for 'tools' search."},
            "url": {"type": "string", "format": "uri", "description": "URL to fetch content from (for 'web' action). Mutually exclusive with 'query'."},
            "query": {"type": "string", "description": "Search query for 'web' action. Uses keyless DuckDuckGo. Returns ranked results (title, url, snippet). Mutually exclusive with 'url'."},
            "prompt": {"type": "string", "description": "The prompt to run on the fetched content (for 'web' action with 'url')."},
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
            "severities": {
                "type": "array",
                "items": {"type": "string", "enum": ["error", "warning", "info", "hint"]},
                "description": "Post-run severity filter for structural `workflow=\"scan\"`. When present, only findings matching one of the listed severities are returned. Does not override rule severities at the CLI level."
            },
            "no_ignore": {
                "type": "array",
                "items": {"type": "string", "enum": ["hidden", "dot", "exclude", "global", "parent", "vcs"]},
                "description": "Ignore file overrides: hidden, dot, exclude, global, parent, vcs."
            },
            "threads": {"type": "integer", "description": "Number of threads for ast-grep scan parallelism. 0 means auto. Only for `workflow=\"scan\"`.", "minimum": 0, "maximum": 256, "default": 0},
            "format": {"type": "string", "enum": ["github", "sarif", "files_with_matches", "count"], "description": "Output format for structural `workflow=\"scan\"`. `github`/`sarif`: CI pipeline formats (raw output). `files_with_matches`: return only unique file paths. `count`: return match counts per file."},
            "report_style": {"type": "string", "enum": ["rich", "medium", "short"], "description": "Diagnostic report style for structural `workflow=\"scan\"`. Controls verbosity of diagnostic output."},
            "before_lines": {"type": "integer", "description": "Context lines before each match for structural workflows. Mutually exclusive with `context_lines`.", "minimum": 0, "maximum": 20},
            "after_lines": {"type": "integer", "description": "Context lines after each match for structural workflows. Mutually exclusive with `context_lines`.", "minimum": 0, "maximum": 20},
            "builtin_rules": {
                "type": "array",
                "items": {"type": "string"},
                "description": "Built-in ast-grep rules to activate for `workflow=\"scan\"`. Valid values: `unused-suppression` (reports stale ignore directives), `no-suppress-all` (reports suppress-all comments). Use `\"rule:severity\"` format to set severity (e.g. `\"unused-suppression:error\"`). Default severity is hint."
            },
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
    fn unified_search_schema_advertises_structural_and_hides_intelligence() {
        let params = unified_search_parameters();
        let actions = params["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");

        assert!(actions.iter().any(|value| value == "structural"));
        assert!(actions.iter().any(|value| value == "outline"));
        assert!(!actions.iter().any(|value| value == "intelligence"));
        assert!(
            params["properties"]["view"]["enum"]
                .as_array()
                .expect("view enum")
                .iter()
                .any(|value| value == "digest")
        );
        assert!(
            params["properties"]["items"]["enum"]
                .as_array()
                .expect("items enum")
                .iter()
                .any(|value| value == "auto")
        );
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
                .contains("structural")
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("preferred"),
            "action description should mark outline as preferred for repo overview"
        );
        assert!(
            params["properties"]["action"]["description"]
                .as_str()
                .expect("action description")
                .contains("paginated"),
            "action description should warn that list is paginated"
        );
        assert!(
            params["properties"]["pattern"]["description"]
                .as_str()
                .expect("pattern description")
                .contains("ast-grep pattern")
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
                .contains("grep")
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
    fn code_search_schema_exposes_only_code_search_actions() {
        let params = code_search_parameters();
        let actions = params["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");

        assert_eq!(params["required"], json!(["action"]));
        assert!(actions.iter().any(|value| value == "grep"));
        assert!(actions.iter().any(|value| value == "structural"));
        assert!(actions.iter().any(|value| value == "outline"));
        for removed_action in ["list", "tools", "errors", "agent", "web", "skill"] {
            assert!(
                !actions.iter().any(|value| value == removed_action),
                "{removed_action} must not be a code_search action"
            );
        }
        assert_eq!(params["additionalProperties"], false);
        assert!(params["properties"]["lang"].is_object());
        assert!(params["properties"]["selector"].is_object());
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
