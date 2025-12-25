use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
use serde_json::{Map, Value, json};
use vtcode_config::ToolDocumentationMode;

use super::builtins::builtin_tool_registrations;
use super::progressive_docs::{
    build_minimal_declarations, build_progressive_declarations, minimal_tool_signatures,
};

const PATH_ALIAS_WITH_TARGET: &[(&str, &str)] = &[
    ("file_path", "Alias for path"),
    ("filepath", "Alias for path"),
    ("target_path", "Alias for path"),
];

const CONTENT_ALIASES: &[(&str, &str)] = &[
    ("contents", "Alias for content"),
    ("text", "Alias for content"),
];

const OLD_STR_ALIASES: &[(&str, &str)] = &[
    ("old_text", "Alias for old_str"),
    ("original", "Alias for old_str"),
    ("target", "Alias for old_str"),
    ("from", "Alias for old_str"),
];

const NEW_STR_ALIASES: &[(&str, &str)] = &[
    ("new_text", "Alias for new_str"),
    ("replacement", "Alias for new_str"),
    ("to", "Alias for new_str"),
];

fn insert_string_property(properties: &mut Map<String, Value>, key: &str, description: &str) {
    insert_string_property_with_default(properties, key, description, None);
}

fn insert_string_property_with_default(
    properties: &mut Map<String, Value>,
    key: &str,
    description: &str,
    default: Option<&str>,
) {
    let mut value = json!({
        "type": "string",
        "description": description,
    });

    if let Some(default_value) = default
        && let Value::Object(ref mut obj) = value
    {
        obj.insert("default".to_string(), json!(default_value));
    }

    properties.insert(key.to_string(), value);
}

fn insert_string_with_aliases(
    properties: &mut Map<String, Value>,
    key: &str,
    description: &str,
    aliases: &[(&str, &str)],
) {
    insert_string_property(properties, key, description);
    for (alias, alias_description) in aliases {
        insert_string_property(properties, alias, alias_description);
    }
}

fn insert_bool_property(
    properties: &mut Map<String, Value>,
    key: &str,
    description: &str,
    default: Option<bool>,
) {
    let mut value = json!({
        "type": "boolean",
        "description": description,
    });

    if let Some(default_value) = default
        && let Value::Object(ref mut obj) = value
    {
        obj.insert("default".to_string(), json!(default_value));
    }

    properties.insert(key.to_string(), value);
}

fn required_pairs(left: &[&str], right: &[&str]) -> Vec<Value> {
    let mut entries = Vec::with_capacity(left.len() * right.len());
    for lhs in left {
        for rhs in right {
            entries.push(json!({ "required": [lhs, rhs] }));
        }
    }
    entries
}

fn required_triples(first: &[&str], second: &[&str], third: &[&str]) -> Vec<Value> {
    let mut entries = Vec::with_capacity(first.len() * second.len() * third.len());
    for a in first {
        for b in second {
            for c in third {
                entries.push(json!({ "required": [a, b, c] }));
            }
        }
    }
    entries
}

fn required_single(keys: &[&str]) -> Vec<Value> {
    keys.iter()
        .map(|key| json!({ "required": [key] }))
        .collect()
}

fn build_alias_keys(
    base: &'static str,
    aliases: &'static [(&'static str, &'static str)],
) -> Vec<&'static str> {
    let mut keys = Vec::with_capacity(1 + aliases.len());
    keys.push(base);
    keys.extend(aliases.iter().map(|(alias, _)| *alias));
    keys
}

fn path_keys() -> &'static [&'static str] {
    static PATH_KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    PATH_KEYS
        .get_or_init(|| build_alias_keys("path", PATH_ALIAS_WITH_TARGET))
        .as_slice()
}

fn content_keys() -> &'static [&'static str] {
    static CONTENT_KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    CONTENT_KEYS
        .get_or_init(|| build_alias_keys("content", CONTENT_ALIASES))
        .as_slice()
}

fn old_str_keys() -> &'static [&'static str] {
    static OLD_KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    OLD_KEYS
        .get_or_init(|| build_alias_keys("old_str", OLD_STR_ALIASES))
        .as_slice()
}

fn new_str_keys() -> &'static [&'static str] {
    static NEW_KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    NEW_KEYS
        .get_or_init(|| build_alias_keys("new_str", NEW_STR_ALIASES))
        .as_slice()
}

fn create_file_requirements() -> &'static [Value] {
    static REQUIREMENTS: OnceLock<Vec<Value>> = OnceLock::new();
    REQUIREMENTS
        .get_or_init(|| required_pairs(path_keys(), content_keys()))
        .as_slice()
}

fn delete_file_requirements() -> &'static [Value] {
    static REQUIREMENTS: OnceLock<Vec<Value>> = OnceLock::new();
    REQUIREMENTS
        .get_or_init(|| required_single(path_keys()))
        .as_slice()
}

fn write_file_requirements() -> &'static [Value] {
    static REQUIREMENTS: OnceLock<Vec<Value>> = OnceLock::new();
    REQUIREMENTS
        .get_or_init(|| required_pairs(path_keys(), content_keys()))
        .as_slice()
}

fn edit_file_requirements() -> &'static [Value] {
    static REQUIREMENTS: OnceLock<Vec<Value>> = OnceLock::new();
    REQUIREMENTS
        .get_or_init(|| required_triples(path_keys(), old_str_keys(), new_str_keys()))
        .as_slice()
}

fn base_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        // ============================================================
        // SEARCH & DISCOVERY TOOLS
        // ============================================================
        FunctionDeclaration {
            name: tools::GREP_FILE.to_owned(),
            description: "Fast regex-based code search using ripgrep (replaces ast-grep). Find patterns, functions, definitions, TODOs, errors, imports, and API calls across files. Respects .gitignore/.ignore by default. Supports glob patterns, file-type filtering, context lines, and regex/literal matching. Essential for code navigation and analysis. Note: pattern is required; use literal: true for exact string matching. Invalid regex patterns will be rejected with helpful error messages.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern or literal string to search for. Examples: 'fn \\\\w+\\\\(', 'TODO|FIXME', '^import\\\\s', '\\\\.get\\\\(' for HTTP verbs"},
                    "path": {"type": "string", "description": "Directory path (relative). Defaults to current directory.", "default": "."},
                    "max_results": {"type": "integer", "description": "Maximum number of results to return (1-1000).", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching. Default uses smart-case: lowercase pattern = case-insensitive, with uppercase = case-sensitive.", "default": false},
                    "literal": {"type": "boolean", "description": "Treat pattern as literal string, not regex. Use for exact string matching.", "default": false},
                    "glob_pattern": {"type": "string", "description": "Filter files by glob pattern. Examples: '**/*.rs' (all Rust), 'src/**/*.ts' (TypeScript in src), '*.test.js'."},
                    "context_lines": {"type": "integer", "description": "Lines of context before/after matches (0-20). Use 3-5 to see surrounding code. Default 0 for concise output.", "default": 0},
                    "respect_ignore_files": {"type": "boolean", "description": "Respect .gitignore and .ignore files. Set false to search all files including ignored ones.", "default": true},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files (those starting with dot) in search results.", "default": false},
                    "max_file_size": {"type": "integer", "description": "Maximum file size to search in bytes. Skips files larger than this. Example: 5242880 for 5MB."},
                    "search_hidden": {"type": "boolean", "description": "Search inside hidden directories (those starting with dot).", "default": false},
                    "search_binary": {"type": "boolean", "description": "Search binary files. Usually false to avoid noise from compiled code/media.", "default": false},
                    "files_with_matches": {"type": "boolean", "description": "Return only filenames containing matches, not the match lines themselves.", "default": false},
                    "type_pattern": {"type": "string", "description": "Filter by file type: 'rust', 'python', 'typescript', 'javascript', 'java', 'go', etc. Faster than glob for language filtering."},
                    "invert_match": {"type": "boolean", "description": "Invert matching: return lines that do NOT match the pattern.", "default": false},
                    "word_boundaries": {"type": "boolean", "description": "Match only at word boundaries (\\\\b in regex). Prevents partial word matches.", "default": false},
                    "line_number": {"type": "boolean", "description": "Include line numbers in output. Recommended true for file navigation.", "default": true},
                    "column": {"type": "boolean", "description": "Include column numbers in output for precise positioning.", "default": false},
                    "only_matching": {"type": "boolean", "description": "Show only the matched part of each line, not the full line.", "default": false},
                    "trim": {"type": "boolean", "description": "Trim leading/trailing whitespace from output lines.", "default": false},
                    "response_format": {"type": "string", "description": "Output format: 'concise' (compact JSON) or 'detailed' (with metadata).", "default": "concise"}
                },
                "required": ["pattern"]
            }),
        },

        FunctionDeclaration {
            name: "list_files".to_string(),
            description: "Explore workspace. Modes: list (directory contents), recursive (full tree), find_name (by filename), find_content (by content), largest (by file size). Use pagination for large directories.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path (relative). Defaults to workspace root."},
                    "mode": {"type": "string", "description": "Operation mode: 'list' (directory contents), 'recursive' (full tree), 'find_name' (find by filename), 'find_content' (find by content), 'largest' (find largest files).", "default": "list"},
                    "max_items": {"type": "integer", "description": "Maximum items to scan (higher = slower but more complete).", "default": 1000},
                    "page": {"type": "integer", "description": "Page number for pagination (1-based).", "default": 1},
                    "per_page": {"type": "integer", "description": "Items per page.", "default": 50},
                    "response_format": {"type": "string", "description": "Output format: 'concise' (compact JSON) or 'detailed' (with metadata).", "default": "concise"},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files and directories (starting with dot).", "default": false},
                    "name_pattern": {"type": "string", "description": "Glob pattern for filenames when mode='find_name' (e.g., '*.rs').", "default": "*"},
                    "content_pattern": {"type": "string", "description": "Regex pattern to search file contents when mode='find_content'."},
                    "file_extensions": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Filter results by file extensions (e.g., ['rs', 'toml'])."
                    },
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching for patterns.", "default": true}
                },
                "required": ["path"]
            }),
        },

        FunctionDeclaration {
            name: tools::RUN_PTY_CMD.to_string(),
            description: "Execute shell commands (git, cargo, npm, shell scripts, etc). Full terminal emulation with PTY support for both one-off and interactive modes. Respects command policies for safety. Output is automatically truncated to prevent context overflow.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "description": "Shell command to execute (e.g., 'git diff', 'cargo test', 'npm install'). Can be string or array of strings.",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    },
                    "cwd": {"type": "string", "description": "Working directory for the command (relative or absolute)."},
                    "timeout_secs": {"type": "integer", "description": "Timeout in seconds. Default 180 for most commands, longer for cargo/build commands.", "default": 180},
                    "confirm": {"type": "boolean", "description": "Require confirmation before executing destructive commands (rm, git reset, etc).", "default": false},
                    "max_tokens": {"type": "integer", "description": "Maximum output tokens before truncation (default: 8000). Set to 0 to disable truncation (not recommended). Helps prevent context window overflow for verbose commands like 'cargo clippy'."}
                },
                "required": ["command"]
            }),
        },

        FunctionDeclaration {
            name: tools::SEARCH_TOOLS.to_string(),
            description: "Search available MCP tools by keyword with progressive disclosure. Saves context by returning only tool names, descriptions, or full schemas as needed.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "keyword": {"type": "string", "description": "Search term to find relevant tools (searches name and description)"},
                    "detail_level": {
                        "type": "string",
                        "enum": ["name-only", "name-and-description", "full"],
                        "description": "Detail level: 'name-only' (minimal context), 'name-and-description' (default), or 'full' (includes input schema)",
                        "default": "name-and-description"
                    }
                },
                "required": ["keyword"]
            }),
        },

        FunctionDeclaration {
            name: tools::SKILL.to_string(),
            description: "Load a Claude Agent Skill by name. Skills are specialized subagents with instructions, reference files, and scripts stored in .claude/skills/. Returns skill instructions and available resources. Use search_tools to discover available skills first.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name (e.g., 'spreadsheet-generator', 'doc-generator', 'pdf-report-generator')"}
                },
                "required": ["name"]
            }),
        },

        FunctionDeclaration {
            name: tools::EXECUTE_CODE.to_string(),
            description: "Execute Python or JavaScript code with access to MCP tools as library functions. Supports loops, conditionals, data filtering, and aggregation. Results are returned as JSON via `result = {...}` assignment.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "code": {"type": "string", "description": "Python 3 or JavaScript code to execute"},
                    "language": {
                        "type": "string",
                        "enum": ["python3", "javascript"],
                        "description": "Programming language: 'python3' or 'javascript'",
                        "default": "python3"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Maximum execution time in seconds (default: 30)",
                        "default": 30
                    }
                },
                "required": ["code", "language"]
            }),
        },
        FunctionDeclaration {
            name: tools::DEBUG_AGENT.to_string(),
            description: "Return diagnostic information about the agent environment: current configuration, available tools, workspace state, and system info. Useful for troubleshooting setup issues.".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        FunctionDeclaration {
            name: tools::ANALYZE_AGENT.to_string(),
            description: "Return analysis of agent behavior: tool usage patterns, failure rates, context window usage, and performance metrics. Useful for understanding tool interaction patterns.".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },

        // ============================================================
        // FILE OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::READ_FILE.to_string(),
            description: "Read file contents safely. Returns JSON with 'content', 'status', and 'message'. Supports chunked reads/offset. IMPORTANT: If 'status' is 'success', the file has been successfully read. DO NOT retry reading the same file with identical keys, even if the content seems short or empty.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path (absolute or workspace-relative)."},
                    "offset_lines": {"type": "integer", "description": "Start reading from this 1-based line offset (use with limit for paging)."},
                    "limit": {"type": "integer", "description": "Number of lines to read from offset_lines (default reads from start)."},
                    "max_bytes": {"type": "integer", "description": "Hard cap on bytes read; if exceeded, content is truncated and was_truncated=true."},
                    "chunk_lines": {"type": "integer", "description": "Auto-chunking threshold for large files; smaller values increase paging.", "default": 2000},
                    "max_lines": {"type": "integer", "description": "Deprecated: prefer limit or max_tokens."},
                    "max_tokens": {"type": "integer", "description": "Token budget for content; prefer this over line counts for large files."}
                },
                "required": ["path"]
            }),
        },

        FunctionDeclaration {
            name: tools::CREATE_FILE.to_string(),
            description: "Create a new file. Fails if file already exists to prevent accidental overwrites.".to_string(),
            parameters: {
                let mut properties = Map::new();

                insert_string_with_aliases(
                    &mut properties,
                    "path",
                    "Workspace-relative path to create",
                    PATH_ALIAS_WITH_TARGET,
                );
                insert_string_with_aliases(
                    &mut properties,
                    "content",
                    "Initial file contents",
                    CONTENT_ALIASES,
                );
                insert_string_property(
                    &mut properties,
                    "encoding",
                    "Text encoding (utf-8 default)",
                );

                json!({
                    "type": "object",
                    "properties": properties,
                    "anyOf": create_file_requirements()
                })
            },
        },

        FunctionDeclaration {
            name: tools::DELETE_FILE.to_string(),
            description: "Delete a file or directory (with recursive flag).".to_string(),
            parameters: {
                let mut properties = Map::new();
                insert_string_with_aliases(
                    &mut properties,
                    "path",
                    "Workspace-relative path to delete",
                    PATH_ALIAS_WITH_TARGET,
                );
                insert_bool_property(
                    &mut properties,
                    "recursive",
                    "Allow deleting directories",
                    Some(false),
                );
                insert_bool_property(
                    &mut properties,
                    "force",
                    "Ignore missing files",
                    Some(false),
                );

                json!({
                    "type": "object",
                    "properties": properties,
                    "anyOf": delete_file_requirements()
                })
            },
        },

        FunctionDeclaration {
            name: tools::WRITE_FILE.to_string(),
            description: "Create or modify a file safely. Defaults to fail if the file exists; set mode=\"overwrite\" to replace. Prefer search_replace/edit_file for partial edits and read_file before overwriting. Modes: fail_if_exists (default), overwrite, append, skip_if_exists.".to_string(),
            parameters: {
                let mut properties = Map::new();
                insert_string_with_aliases(
                    &mut properties,
                    "path",
                    "Workspace-relative path to write",
                    PATH_ALIAS_WITH_TARGET,
                );
                insert_string_with_aliases(
                    &mut properties,
                    "content",
                    "Full file contents to persist",
                    CONTENT_ALIASES,
                );
                insert_bool_property(
                    &mut properties,
                    "overwrite",
                    "Alias: set true to overwrite (maps to mode=overwrite)",
                    Some(false),
                );
                insert_string_property_with_default(
                    &mut properties,
                    "mode",
                    "fail_if_exists|overwrite|append|skip_if_exists",
                    Some("fail_if_exists"),
                );
                insert_string_property(
                    &mut properties,
                    "write_mode",
                    "Alias for mode",
                );
                insert_string_property(
                    &mut properties,
                    "encoding",
                    "Text encoding (utf-8 default)",
                );

                json!({
                    "type": "object",
                    "properties": properties,
                    "anyOf": write_file_requirements()
                })
            },
        },

        FunctionDeclaration {
            name: tools::EDIT_FILE.to_string(),
            description: "Replace existing text in a file by exact string match. Best for surgical updates to preserve surrounding code.".to_string(),
            parameters: {
                let mut properties = Map::new();
                insert_string_with_aliases(
                    &mut properties,
                    "path",
                    "Workspace-relative file path",
                    PATH_ALIAS_WITH_TARGET,
                );
                insert_string_with_aliases(
                    &mut properties,
                    "old_str",
                    "Exact text to replace",
                    OLD_STR_ALIASES,
                );
                insert_string_with_aliases(
                    &mut properties,
                    "new_str",
                    "Replacement text",
                    NEW_STR_ALIASES,
                );
                insert_string_property(
                    &mut properties,
                    "encoding",
                    "Text encoding (utf-8 default)",
                );

                json!({
                    "type": "object",
                    "properties": properties,
                    "anyOf": edit_file_requirements()
                })
            },
        },

        FunctionDeclaration {
            name: tools::APPLY_PATCH.to_string(),
            description: "Apply structured diffs to modify files. Use this tool to create, update, or delete file content using unified diff format. The tool enables iterative, multi-step code editing workflows by applying patches and reporting results back. GPT-5.1 specific tool optimized for precise file modifications.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Patch content in unified diff format with *** Begin/End Patch markers"},
                    "patch": {"type": "string", "description": "Alias for input - unified diff format patch"},
                    "diff": {"type": "string", "description": "Alias for input - unified diff format patch"}
                },
                "anyOf": [
                    {"required": ["input"]},
                    {"required": ["patch"]},
                    {"required": ["diff"]}
                ]
            }),
        },

        FunctionDeclaration {
            name: tools::SEARCH_REPLACE.to_string(),
            description: "Search for a literal block in a file and replace it with new text. Validates workspace paths, supports optional backup, and can constrain matches using before/after context hints.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to file to edit"},
                    "search": {"type": "string", "description": "Literal text to find"},
                    "replace": {"type": "string", "description": "Replacement text"},
                    "max_replacements": {"type": "integer", "description": "Optional cap on replacements"},
                    "backup": {"type": "boolean", "description": "Create .bak backup before writing", "default": true},
                    "before": {"type": "string", "description": "Optional required prefix immediately before the match"},
                    "after": {"type": "string", "description": "Optional required suffix immediately after the match"}
                },
                "required": ["path", "search", "replace"]
            }),
        },


        // ============================================================
        // PTY SESSION MANAGEMENT
        // ============================================================

        FunctionDeclaration {
            name: tools::CREATE_PTY_SESSION.to_string(),
            description: "Create persistent PTY session for reuse across calls. For GPT-5.1 models, use shell tool for direct command-line interface interactions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Unique session ID"},
                    "command": {
                        "description": "Command (string or array)",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Arguments when command is string"
                    },
                    "working_dir": {"type": "string", "description": "Working directory"},
                    "rows": {"type": "integer", "description": "Terminal rows", "default": 24},
                    "cols": {"type": "integer", "description": "Terminal columns", "default": 80}
                },
                "required": ["session_id", "command"]
            }),
        },

        FunctionDeclaration {
            name: tools::LIST_PTY_SESSIONS.to_string(),
            description: "List active PTY sessions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },

        FunctionDeclaration {
            name: tools::CLOSE_PTY_SESSION.to_string(),
            description: "Terminate PTY session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID to close"}
                },
                "required": ["session_id"]
            }),
        },

        FunctionDeclaration {
            name: tools::SEND_PTY_INPUT.to_string(),
            description: "Send input to PTY session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID"},
                    "input": {"type": "string", "description": "UTF-8 text to send"},
                    "input_base64": {"type": "string", "description": "Base64 encoded bytes"},
                    "append_newline": {"type": "boolean", "description": "Append newline", "default": false},
                    "wait_ms": {"type": "integer", "description": "Wait before capture (ms)", "default": 0},
                    "drain": {"type": "boolean", "description": "Clear buffer after capture", "default": true},
                    "max_tokens": {"type": "integer", "description": "Maximum tokens to include in output (per-call token budget). Prefer token-based limits for large outputs."}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },

        FunctionDeclaration {
            name: tools::READ_PTY_SESSION.to_string(),
            description: "Read PTY session state (screen + scrollback).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID"},
                    "drain": {"type": "boolean", "description": "Clear new output", "default": false},
                    "include_screen": {"type": "boolean", "description": "Include screen buffer", "default": true},
                    "include_scrollback": {"type": "boolean", "description": "Include scrollback", "default": true},
                    "max_tokens": {"type": "integer", "description": "Maximum tokens to include in output (per-call token budget). Prefer token-based limits for large outputs."}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },

        FunctionDeclaration {
            name: tools::RESIZE_PTY_SESSION.to_string(),
            description: "Resize PTY session terminal dimensions.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Session ID"},
                    "rows": {"type": "integer", "description": "Terminal rows", "minimum": 1},
                    "cols": {"type": "integer", "description": "Terminal columns", "minimum": 1}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },

        // ============================================================
        // NETWORK OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::WEB_FETCH.to_string(),
            description: "Fetches content from a specified URL and processes it using an AI model. Takes a URL and a prompt as input, fetches the URL content, converts HTML to markdown, processes the content with the prompt using a small, fast model, and returns the model's response about the content. Use this tool when you need to retrieve and analyze web content.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "format": "uri", "description": "The URL to fetch content from"},
                    "prompt": {"type": "string", "description": "The prompt to run on the fetched content"},
                    "max_bytes": {"type": "integer", "description": "Maximum bytes to fetch", "default": 500000},
                    "timeout_secs": {"type": "integer", "description": "Timeout in seconds", "default": 30}
                },
                "required": ["url", "prompt"]
            }),
        },

        // ============================================================
        // PLANNING
        // ============================================================
        FunctionDeclaration {
            name: tools::UPDATE_PLAN.to_string(),
            description: "Update the task plan with progress tracking and planning phases. For non-trivial tasks (4+ steps), use this to create precise plans with quality standards: (1) Understanding phase: explore 5-10 files, find similar patterns; (2) Design phase: break into 3-7 steps with specific file paths (e.g., src/tools/plan.rs:280); (3) Review phase: verify dependencies and ordering; (4) Final plan: ensure file paths, complexity estimates, and acceptance criteria are included. Good plans specify WHAT file WHERE at WHICH lines. Keep tools read-only during planning phases (understanding/design/review).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "explanation": {"type": "string", "description": "Plan summary or context from exploration phase - what files were read, what patterns were found"},
                    "phase": {
                        "type": "string",
                        "description": "Current planning phase: understanding (gather context, read 5-10 files), design (propose approaches with file paths), review (validate plan quality), or final_plan (ready to execute - requires file paths and proper breakdown)",
                        "enum": ["understanding", "design", "review", "final_plan"]
                    },
                    "plan": {
                        "type": "array",
                        "description": "Plan steps with status. Required: 3-7 steps for quality. Include specific file paths with line numbers (e.g., 'Update validate_plan in plan.rs:395-417'). Order by dependencies. One in_progress at a time.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {"type": "string", "description": "Step description with file path and line numbers"},
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Step status"
                                }
                            },
                            "required": ["step", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["plan"],
                "additionalProperties": false
            }),
        },
    ]
}

pub fn build_function_declarations() -> Vec<FunctionDeclaration> {
    build_function_declarations_with_mode(true, ToolDocumentationMode::default())
}

pub fn build_function_declarations_with_mode(
    todo_planning_enabled: bool,
    tool_documentation_mode: ToolDocumentationMode,
) -> Vec<FunctionDeclaration> {
    // Select base declarations based on documentation mode
    let mut declarations = match tool_documentation_mode {
        ToolDocumentationMode::Minimal => {
            tracing::debug!(
                mode = "minimal",
                "Building minimal tool declarations (~800 tokens total)"
            );
            let signatures = minimal_tool_signatures();
            build_minimal_declarations(&signatures)
        }
        ToolDocumentationMode::Progressive => {
            tracing::debug!(
                mode = "progressive",
                "Building progressive tool declarations (~1,200 tokens total)"
            );
            let signatures = minimal_tool_signatures();
            build_progressive_declarations(&signatures)
        }
        ToolDocumentationMode::Full => {
            tracing::debug!(
                mode = "full",
                "Building full tool declarations (~3,000 tokens total)"
            );
            base_function_declarations()
        }
    };

    // Apply metadata overrides only for full mode
    // (minimal/progressive already have optimized structure)
    if tool_documentation_mode == ToolDocumentationMode::Full {
        apply_metadata_overrides(&mut declarations);
    }

    // Remove update_plan tool if todo planning is disabled
    if !todo_planning_enabled {
        declarations.retain(|decl| decl.name != tools::UPDATE_PLAN);
    }

    declarations
}

/// Build function declarations filtered by capability level
pub fn build_function_declarations_for_level(level: CapabilityLevel) -> Vec<FunctionDeclaration> {
    let tool_capabilities: HashMap<&'static str, CapabilityLevel> = builtin_tool_registrations()
        .into_iter()
        .filter(|registration| registration.expose_in_llm())
        .map(|registration| (registration.name(), registration.capability()))
        .collect();

    let mut declarations = build_function_declarations();
    apply_metadata_overrides(&mut declarations);

    declarations
        .into_iter()
        .filter(|fd| {
            tool_capabilities
                .get(fd.name.as_str())
                .map(|required| level >= *required)
                .unwrap_or(false)
        })
        .collect()
}

fn apply_metadata_overrides(declarations: &mut [FunctionDeclaration]) {
    let registrations = builtin_tool_registrations();
    let mut metadata_by_name: HashMap<&str, _> = registrations
        .iter()
        .map(|r| (r.name(), r.metadata()))
        .collect();

    for decl in declarations.iter_mut() {
        if let Some(meta) = metadata_by_name.remove(decl.name.as_str()) {
            if let Some(schema) = meta.parameter_schema() {
                decl.parameters = schema.clone();
            }
            annotate_parameters(&mut decl.parameters, meta);
        }
    }
}

fn annotate_parameters(params: &mut Value, meta: &super::registration::ToolMetadata) {
    let Value::Object(map) = params else {
        return;
    };

    if let Some(permission) = meta.default_permission() {
        map.insert(
            "x-default-permission".to_string(),
            json!(permission_label(&permission)),
        );
    }

    if !meta.aliases().is_empty() {
        map.insert("x-aliases".to_string(), json!(meta.aliases().to_vec()));
    }

    if let Some(schema) = meta.config_schema() {
        map.insert("x-config-schema".to_string(), schema.clone());
    }

    if let Some(schema) = meta.state_schema() {
        map.insert("x-state-schema".to_string(), schema.clone());
    }
}

fn permission_label(permission: &ToolPolicy) -> &'static str {
    match permission {
        ToolPolicy::Allow => "allow",
        ToolPolicy::Deny => "deny",
        ToolPolicy::Prompt => "prompt",
    }
}
