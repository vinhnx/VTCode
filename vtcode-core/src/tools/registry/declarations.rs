use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use serde_json::{Map, Value, json};

use super::builtins::builtin_tool_registrations;

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

    if let Some(default_value) = default {
        if let Value::Object(ref mut obj) = value {
            obj.insert("default".to_string(), json!(default_value));
        }
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

    if let Some(default_value) = default {
        if let Value::Object(ref mut obj) = value {
            obj.insert("default".to_string(), json!(default_value));
        }
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
            name: tools::GREP_FILE.to_string(),
            description: "Search code using ripgrep. Find patterns, functions, TODOs across files. Respects .gitignore by default. Use concise format for efficiency.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Search pattern (e.g. 'fn \\w+', 'TODO')"},
                    "path": {"type": "string", "description": "Directory path (relative)", "default": "."},
                    "max_results": {"type": "integer", "description": "Maximum number of results to return", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching (smart-case is used by default if not specified)", "default": false},
                    "literal": {"type": "boolean", "description": "Treat pattern as literal string (disable regex)", "default": false},
                    "glob_pattern": {"type": "string", "description": "Glob pattern to filter files (e.g., '*.rs', 'src/**/*.js')"},
                    "context_lines": {"type": "integer", "description": "Number of context lines around matches", "default": 0},
                    "respect_ignore_files": {"type": "boolean", "description": "Respect .gitignore, .ignore files", "default": true},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files in search", "default": false},
                    "max_file_size": {"type": "integer", "description": "Maximum file size to search (in bytes)"},
                    "search_hidden": {"type": "boolean", "description": "Search in hidden directories", "default": false},
                    "search_binary": {"type": "boolean", "description": "Search binary files", "default": false},
                    "files_with_matches": {"type": "boolean", "description": "Only return filenames that contain matches", "default": false},
                    "type_pattern": {"type": "string", "description": "Search only files of specified type (e.g., 'rust', 'python', 'js')"},
                    "invert_match": {"type": "boolean", "description": "Invert the match (show non-matching lines)", "default": false},
                    "word_boundaries": {"type": "boolean", "description": "Match only on word boundaries", "default": false},
                    "line_number": {"type": "boolean", "description": "Show line numbers in output", "default": true},
                    "column": {"type": "boolean", "description": "Show column numbers in output", "default": false},
                    "only_matching": {"type": "boolean", "description": "Show only matching parts of lines", "default": false},
                    "trim": {"type": "boolean", "description": "Trim whitespace from output", "default": false},
                    "response_format": {"type": "string", "description": "concise|detailed", "default": "concise"}
                },
                "required": ["pattern"]
            }),
        },

        FunctionDeclaration {
            name: "list_files".to_string(),
            description: "Explore workspace. Modes: list|recursive|find_name|find_content|largest. Use pagination for large directories.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Starting directory (relative)"},
                    "mode": {"type": "string", "description": "list|recursive|find_name|find_content|largest", "default": "list"},
                    "max_items": {"type": "integer", "description": "Maximum items to scan", "default": 1000},
                    "page": {"type": "integer", "description": "Page number (1-based)", "default": 1},
                    "per_page": {"type": "integer", "description": "Items per page", "default": 50},
                    "response_format": {"type": "string", "description": "concise|detailed", "default": "concise"},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files", "default": false},
                    "name_pattern": {"type": "string", "description": "File pattern (e.g. *.rs)", "default": "*"},
                    "content_pattern": {"type": "string", "description": "Content search pattern"},
                    "file_extensions": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Filter by extensions"
                    },
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching", "default": true},
                    "ast_grep_pattern": {"type": "string", "description": "AST filter pattern"}
                },
                "required": ["path"]
            }),
        },

        // ============================================================
        // FILE OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::READ_FILE.to_string(),
            description: "Read file contents. Auto-chunks large files (>2000 lines).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "max_bytes": {"type": "integer", "description": "Maximum bytes to read"},
                    "chunk_lines": {"type": "integer", "description": "Chunking threshold", "default": 2000},
                    "max_lines": {"type": "integer", "description": "Alternative chunk parameter"}
                },
                "required": ["path"]
            }),
        },

        FunctionDeclaration {
            name: tools::CREATE_FILE.to_string(),
            description: "Create a new file. Fails if the file already exists.".to_string(),
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
            description: "Create or modify a file. Use for full-file rewrites or new files. Modes: overwrite|append|skip_if_exists.".to_string(),
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
                insert_string_property_with_default(
                    &mut properties,
                    "mode",
                    "overwrite|append|skip_if_exists",
                    Some("overwrite"),
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
            description: "Replace existing text in a file by exact match. Best for surgical updates.".to_string(),
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
            description: "Apply Codex-style patch blocks to multiple files atomically.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Patch content with *** Begin/End Patch markers"},
                    "patch": {"type": "string", "description": "Alias for input"},
                    "diff": {"type": "string", "description": "Alias for input"}
                },
                "anyOf": [
                    {"required": ["input"]},
                    {"required": ["patch"]},
                    {"required": ["diff"]}
                ]
            }),
        },

        // ============================================================
        // GIT OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::GIT_DIFF.to_string(),
            description: "Inspect git diffs (files to hunks to lines). Scope with 'paths' array.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "paths": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "File paths to diff"
                    },
                    "staged": {"type": "boolean", "description": "Show staged changes", "default": false},
                    "context_lines": {"type": "integer", "description": "Context lines around hunks", "default": 3},
                    "max_files": {"type": "integer", "description": "Maximum files in response"}
                }
            }),
        },

        // ============================================================
        // COMMAND EXECUTION
        // ============================================================
        FunctionDeclaration {
            name: tools::RUN_COMMAND.to_string(),
            description: "[DEPRECATED] Use PTY session tools instead (create_pty_session, send_pty_input, read_pty_session). Legacy unified command execution with auto-detection.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "description": "Command to execute (array or string)",
                        "oneOf": [
                            {"type": "array", "items": {"type": "string"}},
                            {"type": "string"}
                        ]
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Command arguments (when command is string)"
                    },
                    "working_dir": {"type": "string", "description": "Working directory"},
                    "cwd": {"type": "string", "description": "Alias for working_dir"},
                    "timeout_secs": {"type": "integer", "description": "Timeout seconds (auto: 30s terminal, 300s PTY)"},
                    "mode": {"type": "string", "description": "terminal|pty|auto", "default": "auto"},
                    "tty": {"type": "boolean", "description": "Alias for mode=pty"},
                    "interactive": {"type": "boolean", "description": "Force PTY mode for interactive programs"},
                    "response_format": {"type": "string", "description": "concise|detailed", "default": "concise"},
                    "shell": {"type": "string", "description": "Shell executable"},
                    "login": {"type": "boolean", "description": "Use login shell semantics"},
                    "rows": {"type": "integer", "description": "Terminal rows (PTY mode)", "default": 24},
                    "cols": {"type": "integer", "description": "Terminal columns (PTY mode)", "default": 80}
                },
                "required": ["command"]
            }),
        },

        // ============================================================
        // PTY SESSION MANAGEMENT
        // ============================================================

        FunctionDeclaration {
            name: tools::CREATE_PTY_SESSION.to_string(),
            description: "Create persistent PTY session for reuse across calls.".to_string(),
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
                    "drain": {"type": "boolean", "description": "Clear buffer after capture", "default": true}
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
                    "include_scrollback": {"type": "boolean", "description": "Include scrollback", "default": true}
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
            name: tools::CURL.to_string(),
            description: "Fetch HTTPS content (public hosts only). Size-limited responses.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "HTTPS URL (public hosts)"},
                    "method": {"type": "string", "description": "GET|HEAD", "default": "GET"},
                    "max_bytes": {"type": "integer", "description": "Max response bytes", "default": 65536},
                    "timeout_secs": {"type": "integer", "description": "Timeout (max 30 seconds)", "default": 10},
                    "save_response": {"type": "boolean", "description": "Save to /tmp/vtcode-curl", "default": false}
                },
                "required": ["url"]
            }),
        },

        // ============================================================
        // CODE ANALYSIS & TRANSFORMATION
        // ============================================================
        FunctionDeclaration {
            name: tools::AST_GREP_SEARCH.to_string(),
            description: "Syntax-aware code search/refactoring. Operations: search|transform|lint|refactor.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "description": "search|transform|lint|refactor", "default": "search"},
                    "pattern": {"type": "string", "description": "AST-grep pattern"},
                    "path": {"type": "string", "description": "File or directory", "default": "."},
                    "language": {"type": "string", "description": "Language (auto-detect if omitted)"},
                    "replacement": {"type": "string", "description": "Replacement pattern"},
                    "refactor_type": {"type": "string", "description": "Refactor type (e.g. extract_function)"},
                    "context_lines": {"type": "integer", "description": "Context lines", "default": 0},
                    "max_results": {"type": "integer", "description": "Max results", "default": 100},
                    "preview_only": {"type": "boolean", "description": "Preview without applying", "default": true},
                    "update_all": {"type": "boolean", "description": "Apply all matches", "default": false},
                    "interactive": {"type": "boolean", "description": "Interactive mode", "default": false},
                    "severity_filter": {"type": "string", "description": "Lint severity filter"}
                },
                "required": ["pattern", "path"]
            }),
        },
        // ============================================================
        // PLANNING
        // ============================================================
        FunctionDeclaration {
            name: tools::UPDATE_PLAN.to_string(),
            description: "Track multi-step plan with status (pending|in_progress|completed).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "explanation": {"type": "string", "description": "Plan summary"},
                    "plan": {
                        "type": "array",
                        "description": "Plan steps with status",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {"type": "string", "description": "Step description"},
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
    build_function_declarations_with_mode(true)
}

pub fn build_function_declarations_with_mode(
    todo_planning_enabled: bool,
) -> Vec<FunctionDeclaration> {
    let mut declarations = base_function_declarations();
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

    build_function_declarations()
        .into_iter()
        .filter(|fd| {
            tool_capabilities
                .get(fd.name.as_str())
                .map(|required| level >= *required)
                .unwrap_or(false)
        })
        .collect()
}
