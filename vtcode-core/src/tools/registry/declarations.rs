use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Cache key combining documentation mode ordinal and optional capability level ordinal
/// Using u8 tuple for efficient hashing (both enums have < 256 variants)
type DeclarationCacheKey = (u8, Option<u8>);

/// Cached declarations by (mode, level) to avoid per-turn rebuilds
static DECLARATION_CACHE: OnceLock<
    RwLock<HashMap<DeclarationCacheKey, Arc<Vec<FunctionDeclaration>>>>,
> = OnceLock::new();

fn get_declaration_cache()
-> &'static RwLock<HashMap<DeclarationCacheKey, Arc<Vec<FunctionDeclaration>>>> {
    DECLARATION_CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Convert ToolDocumentationMode to cache key component
#[inline]
fn mode_to_key(mode: ToolDocumentationMode) -> u8 {
    match mode {
        ToolDocumentationMode::Minimal => 0,
        ToolDocumentationMode::Progressive => 1,
        ToolDocumentationMode::Full => 2,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedExecAction {
    Run,
    Write,
    Poll,
    Inspect,
    List,
    Close,
    Code,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedFileAction {
    Read,
    Write,
    Edit,
    Patch,
    Delete,
    Move,
    Copy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedSearchAction {
    Grep,
    List,
    Intelligence,
    Tools,
    Errors,
    Agent,
    Web,
    Skill,
}

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use crate::tool_policy::ToolPolicy;
use serde_json::{Map, Value, json};
use vtcode_config::ToolDocumentationMode;

use super::builtins::builtin_tool_registrations;
use super::progressive_docs::minimal_tool_signatures;

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

fn insert_integer_property(properties: &mut Map<String, Value>, key: &str, description: &str) {
    insert_integer_property_with_default(properties, key, description, None);
}

fn insert_integer_property_with_default(
    properties: &mut Map<String, Value>,
    key: &str,
    description: &str,
    default: Option<i32>,
) {
    let mut value = json!({
        "type": "integer",
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

fn read_file_requirements() -> &'static [Value] {
    static REQUIREMENTS: OnceLock<Vec<Value>> = OnceLock::new();
    REQUIREMENTS
        .get_or_init(|| required_single(path_keys()))
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
            description: "Regex code search (ripgrep). Find patterns, TODOs. Supports globs, file types, context. literal:true for exact match.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex or literal. Examples: 'TODO|FIXME', 'fn \\\\w+\\\\('"},
                    "path": {"type": "string", "description": "Directory (relative). Default: current.", "default": "."},
                    "max_results": {"type": "integer", "description": "Max results (1-1000).", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive. Default: smart-case.", "default": false},
                    "literal": {"type": "boolean", "description": "Literal match, not regex.", "default": false},
                    "glob_pattern": {"type": "string", "description": "File glob. Ex: '**/*.rs', 'src/**/*.ts'"},
                    "context_lines": {"type": "integer", "description": "Context lines (0-20).", "default": 0},
                    "respect_ignore_files": {"type": "boolean", "description": "Respect .gitignore.", "default": true},
                    "include_hidden": {"type": "boolean", "description": "Include dotfiles.", "default": false},
                    "max_file_size": {"type": "integer", "description": "Max file size (bytes)."},
                    "search_hidden": {"type": "boolean", "description": "Search hidden dirs.", "default": false},
                    "search_binary": {"type": "boolean", "description": "Search binaries.", "default": false},
                    "files_with_matches": {"type": "boolean", "description": "Return filenames only.", "default": false},
                    "type_pattern": {"type": "string", "description": "File type: rust, python, typescript, etc."},
                    "invert_match": {"type": "boolean", "description": "Return non-matching lines.", "default": false},
                    "word_boundaries": {"type": "boolean", "description": "Match word boundaries only.", "default": false},
                    "line_number": {"type": "boolean", "description": "Include line numbers.", "default": true},
                    "column": {"type": "boolean", "description": "Include column numbers.", "default": false},
                    "only_matching": {"type": "boolean", "description": "Show matched part only.", "default": false},
                    "trim": {"type": "boolean", "description": "Trim whitespace.", "default": false},
                    "response_format": {"type": "string", "description": "'concise' or 'detailed'.", "default": "concise"}
                },
                "required": ["pattern"]
            }),
        },

        FunctionDeclaration {
            name: "list_files".to_string(),
            description: "Explore workspace. Modes: list, recursive, find_name, find_content, largest. Paginate large dirs.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory (relative). Default: root."},
                    "mode": {"type": "string", "description": "list|recursive|find_name|find_content|largest.", "default": "list"},
                    "max_items": {"type": "integer", "description": "Max items to scan.", "default": 1000},
                    "page": {"type": "integer", "description": "Page number.", "default": 1},
                    "per_page": {"type": "integer", "description": "Items per page.", "default": 50},
                    "response_format": {"type": "string", "description": "'concise' or 'detailed'.", "default": "concise"},
                    "include_hidden": {"type": "boolean", "description": "Include dotfiles.", "default": false},
                    "name_pattern": {"type": "string", "description": "Glob for find_name mode.", "default": "*"},
                    "content_pattern": {"type": "string", "description": "Regex for find_content mode."},
                    "file_extensions": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Filter by extensions."
                    },
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive patterns.", "default": true}
                },
                "required": ["path"]
            }),
        },

        FunctionDeclaration {
            name: tools::UNIFIED_EXEC.to_string(),
            description: "Run commands and manage PTY sessions. Use inspect for one-call output preview/filtering from session or spool file.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Raw command (no shell redirections)."},
                    "input": {"type": "string", "description": "stdin content for write action."},
                    "session_id": {"type": "string", "description": "Session id for write/poll/inspect/close."},
                    "spool_path": {"type": "string", "description": "Spool file path for inspect action."},
                    "query": {"type": "string", "description": "Optional line filter for inspect output or run output."},
                    "head_lines": {"type": "integer", "description": "Inspect head preview lines."},
                    "tail_lines": {"type": "integer", "description": "Inspect tail preview lines."},
                    "max_matches": {"type": "integer", "description": "Max filtered matches for inspect or run query.", "default": 200},
                    "literal": {"type": "boolean", "description": "Treat query as literal text.", "default": false},
                    "code": {"type": "string", "description": "Code to execute for code action."},
                    "language": {
                        "type": "string",
                        "enum": ["python3", "javascript"],
                        "description": "Language for code action.",
                        "default": "python3"
                    },
                    "action": {
                        "type": "string",
                        "enum": ["run", "write", "poll", "inspect", "list", "close", "code"],
                        "description": "Action. Inferred from command/code/input/session_id/spool_path when omitted."
                    },
                    "workdir": {"type": "string", "description": "Working directory for new sessions."},
                    "cwd": {"type": "string", "description": "Alias for workdir."},
                    "shell": {"type": "string", "description": "Shell binary."},
                    "login": {"type": "boolean", "description": "Use login shell.", "default": true},
                    "timeout_secs": {"type": "integer", "description": "Timeout in seconds.", "default": 180},
                    "yield_time_ms": {"type": "integer", "description": "Time to wait for output (ms).", "default": 1000},
                    "confirm": {"type": "boolean", "description": "Confirm destructive ops.", "default": false},
                    "max_output_tokens": {"type": "integer", "description": "Max output tokens."},
                    "track_files": {"type": "boolean", "description": "Track file changes during code execution.", "default": false}
                }
            }),
        },

        FunctionDeclaration {            name: tools::UNIFIED_FILE.to_string(),
            description: "Unified file ops: read, write, edit, patch, delete, move, copy. For edit, `old_str` must match exactly. For patch, use VT Code patch format (`*** Begin Patch`), not unified diff.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["read", "write", "edit", "patch", "delete", "move", "copy"],
                        "description": "Action to perform. If not provided, inferred: 'edit' if old_str present, 'patch' if patch/input patch content present, 'write' if content present, 'move' if destination present, 'read' if a path key is present."
                    },
                    "path": {"type": "string", "description": "File path (relative to workspace root)."},
                    "content": {"type": "string", "description": "New content for 'write' action."},
                    "old_str": {"type": "string", "description": "EXACT text to replace for 'edit' action. Must match file content exactly including whitespace and newlines."},
                    "new_str": {"type": "string", "description": "Replacement text for 'edit' action."},
                    "patch": {"type": "string", "description": "Patch content for 'patch' action. Use '*** Update File: path' format with @@ hunks, NOT unified diff (---/+++ format)."},
                    "destination": {"type": "string", "description": "Target path for 'move' or 'copy' actions."},
                    "start_line": {"type": "integer", "description": "Start line for 'read' action (1-indexed)."},
                    "end_line": {"type": "integer", "description": "End line for 'read' action (inclusive)."},
                    "offset": {"type": "integer", "description": "Alias for start_line."},
                    "limit": {"type": "integer", "description": "Number of lines to read."},
                    "mode": {"type": "string", "description": "Mode for 'read' (e.g., 'head', 'tail') or 'write' (e.g., 'fail_if_exists')."},
                    "indentation": {"type": "boolean", "description": "Include indentation info in 'read' output.", "default": false}
                },
                "required": ["path"]
            }),
        },

        FunctionDeclaration {
            name: tools::UNIFIED_SEARCH.to_string(),
            description: "Unified discovery tool: grep, list, tool discovery, errors, agent status, web fetch, and skills.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["grep", "list", "tools", "errors", "agent", "web", "skill"],
                        "description": "Action to perform."
                    },
                    "pattern": {"type": "string", "description": "Regex or literal pattern for 'grep' or 'errors' search."},
                    "path": {"type": "string", "description": "Directory or file path to search in.", "default": "."},
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
                    "context_lines": {"type": "integer", "description": "Context lines for 'grep' results.", "default": 0},
                    "scope": {"type": "string", "description": "Scope for 'errors' action (archive|all).", "default": "archive"},
                    "max_bytes": {"type": "integer", "description": "Maximum bytes to fetch for 'web' action.", "default": 500000},
                    "timeout_secs": {"type": "integer", "description": "Timeout in seconds.", "default": 30}
                }
            }),
        },

        // ============================================================
        // FILE OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::READ_FILE.to_string(),
            description: "Read file content with optional offsets, limits, and chunked reads (1-indexed).".to_string(),
            parameters: {
                let mut properties = Map::new();
                insert_string_with_aliases(
                    &mut properties,
                    "path",
                    "Workspace-relative or absolute path to the file to read",
                    PATH_ALIAS_WITH_TARGET,
                );
                insert_integer_property(&mut properties, "offset_lines", "Start line number (1-based)");
                insert_integer_property(&mut properties, "limit", "Maximum number of lines to read");
                insert_integer_property(&mut properties, "max_bytes", "Maximum bytes to read");
                insert_integer_property_with_default(&mut properties, "chunk_lines", "Line threshold for chunking", Some(2000));
                insert_integer_property(&mut properties, "max_lines", "Deprecated - use limit instead");
                insert_integer_property(&mut properties, "max_tokens", "Approximate token budget for response");

                json!({
                    "type": "object",
                    "properties": properties,
                    "anyOf": read_file_requirements()
                })
            },
        },

        // NOTE: create_file removed - use write_file with mode=fail_if_exists

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
            description: "Write file. Modes: fail_if_exists (default), overwrite, append, skip_if_exists. Use edit_file for partial edits.".to_string(),
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
            description: "Replace existing text in a file by EXACT string match. CRITICAL: old_str must match file content exactly - including whitespace, newlines, and indentation. Read the file first to copy the exact text.".to_string(),
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
                    "EXACT text to replace - must match file content precisely including whitespace/newlines",
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
            description: "Apply patches to files. IMPORTANT: Use VT Code patch format (*** Begin Patch, *** Update File: path, @@ hunks with -/+ lines, *** End Patch), NOT standard unified diff (---/+++ format).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Patch in VT Code format: *** Begin Patch, *** Update File: path, @@ hunk, -/+ lines, *** End Patch"},
                    "patch": {"type": "string", "description": "Alias for input"}
                },
                "anyOf": [
                    {"required": ["input"]},
                    {"required": ["patch"]}
                ]
            }),
        },

        // NOTE: search_replace removed - use edit_file instead
        // NOTE: PTY session tools (create/list/close/send/read/resize) are routed through unified_exec (run_pty_cmd alias)
        // NOTE: skill, execute_code, web_fetch merged into unified tools

        // ============================================================
        // NETWORK OPERATIONS
        // ============================================================

        // ============================================================
        // HUMAN-IN-THE-LOOP (HITL)
        // ============================================================
        FunctionDeclaration {
            name: tools::TASK_TRACKER.to_string(),
            description: "Track implementation progress with a structured checklist. Actions: create, update, list, add.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "update", "list", "add"],
                        "description": "Action to perform."
                    },
                    "title": {"type": "string", "description": "Checklist title (create)."},
                    "items": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Task items (create)."
                    },
                    "index": {"type": "integer", "description": "1-based index (update)."},
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed", "blocked"],
                        "description": "New status (update)."
                    },
                    "description": {"type": "string", "description": "Task text (add)."},
                    "notes": {"type": "string", "description": "Optional notes."}
                },
                "required": ["action"]
            }),
        },

        FunctionDeclaration {
            name: tools::PLAN_TASK_TRACKER.to_string(),
            description: "Plan-mode scoped hierarchical checklist persisted under .vtcode/plans/<plan>.tasks.md. Actions: create, update, list, add.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "update", "list", "add"],
                        "description": "Action to perform."
                    },
                    "title": {"type": "string", "description": "Checklist title (create)."},
                    "items": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Task items (create). Leading 2-space indentation indicates nesting."
                    },
                    "index_path": {"type": "string", "description": "Hierarchical path for update (example: 2.1)."},
                    "status": {
                        "type": "string",
                        "enum": ["pending", "in_progress", "completed", "blocked"],
                        "description": "New status (update)."
                    },
                    "description": {"type": "string", "description": "Task text (add)."},
                    "parent_index_path": {"type": "string", "description": "Parent path for add (example: 2)."},
                    "notes": {"type": "string", "description": "Optional notes."}
                },
                "required": ["action"]
            }),
        },

        FunctionDeclaration {
            name: tools::REQUEST_USER_INPUT.to_string(),
            description: "Canonical HITL tool: ask the user 1-3 structured questions with optional multiple-choice options.".to_string(),
            parameters: json!({
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
                                    "description": "Optional 1-3 mutually exclusive choices. Put the recommended option first and suffix its label with '(Recommended)'. Do not include an 'Other' option; the UI adds that automatically. If omitted, the UI may auto-suggest options using question text and hints.",
                                    "minItems": 1,
                                    "maxItems": 3,
                                    "items": {
                                        "type": "object",
                                        "additionalProperties": false,
                                        "required": ["label", "description"],
                                        "properties": {
                                            "label": {"type": "string", "description": "User-facing label (1-5 words)."},
                                            "description": {"type": "string", "description": "One short sentence explaining impact."}
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }),
        },
    ]
}

pub fn build_function_declarations() -> Vec<FunctionDeclaration> {
    build_function_declarations_with_mode(ToolDocumentationMode::default())
}

/// Get cached declarations or build and cache them (avoids per-turn rebuilds)
///
/// This is the preferred API for runloop/agent code - it caches declarations
/// per documentation mode to avoid expensive per-turn rebuilds.
pub fn build_function_declarations_cached(
    tool_documentation_mode: ToolDocumentationMode,
) -> Arc<Vec<FunctionDeclaration>> {
    let cache_key = (mode_to_key(tool_documentation_mode), None);

    // Fast path: check read lock first
    {
        let cache = get_declaration_cache().read();
        if let Some(cached) = cache.get(&cache_key) {
            tracing::trace!(mode = ?tool_documentation_mode, "Declaration cache hit");
            return Arc::clone(cached);
        }
    }

    // Slow path: build and cache
    tracing::debug!(mode = ?tool_documentation_mode, "Building and caching declarations");
    let declarations = Arc::new(build_function_declarations_with_mode(
        tool_documentation_mode,
    ));
    {
        let mut cache = get_declaration_cache().write();
        cache.insert(cache_key, Arc::clone(&declarations));
    }

    declarations
}

pub fn build_function_declarations_with_mode(
    tool_documentation_mode: ToolDocumentationMode,
) -> Vec<FunctionDeclaration> {
    let mut declarations = base_function_declarations();

    match tool_documentation_mode {
        ToolDocumentationMode::Full => {
            tracing::debug!(
                mode = "full",
                "Building full tool declarations (~3,000 tokens total)"
            );
            apply_metadata_overrides(&mut declarations);
        }
        ToolDocumentationMode::Progressive => {
            tracing::debug!(
                mode = "progressive",
                "Building progressive tool declarations from canonical schema"
            );
            compact_declarations_for_mode(&mut declarations, tool_documentation_mode);
        }
        ToolDocumentationMode::Minimal => {
            tracing::debug!(
                mode = "minimal",
                "Building minimal tool declarations from canonical schema"
            );
            compact_declarations_for_mode(&mut declarations, tool_documentation_mode);
        }
    }

    declarations
}

fn compact_declarations_for_mode(
    declarations: &mut [FunctionDeclaration],
    mode: ToolDocumentationMode,
) {
    let signatures = minimal_tool_signatures();
    for decl in declarations {
        decl.description = compact_tool_description(
            decl.name.as_str(),
            decl.description.as_str(),
            mode,
            &signatures,
        );
        remove_schema_descriptions(&mut decl.parameters);
    }
}

fn compact_tool_description(
    name: &str,
    original: &str,
    mode: ToolDocumentationMode,
    signatures: &std::collections::HashMap<&str, super::ToolSignature>,
) -> String {
    if let Some(sig) = signatures.get(name) {
        return sig.brief.to_string();
    }

    let max_len = match mode {
        ToolDocumentationMode::Minimal => 64,
        ToolDocumentationMode::Progressive => 120,
        ToolDocumentationMode::Full => usize::MAX,
    };

    first_sentence_with_limit(original, max_len)
}

fn first_sentence_with_limit(text: &str, max_len: usize) -> String {
    let sentence = text
        .split('.')
        .next()
        .unwrap_or(text)
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if sentence.len() <= max_len {
        sentence
    } else {
        let target = max_len.saturating_sub(1);
        let end = sentence
            .char_indices()
            .map(|(i, _)| i)
            .rfind(|&i| i <= target)
            .unwrap_or(0);
        format!("{}…", &sentence[..end])
    }
}

fn remove_schema_descriptions(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("description");
            for nested in map.values_mut() {
                remove_schema_descriptions(nested);
            }
        }
        Value::Array(items) => {
            for item in items {
                remove_schema_descriptions(item);
            }
        }
        _ => {}
    }
}

/// Build function declarations filtered by capability level
pub fn build_function_declarations_for_level(level: CapabilityLevel) -> Vec<FunctionDeclaration> {
    let tool_capabilities: HashMap<&'static str, CapabilityLevel> =
        builtin_tool_registrations(None)
            .into_iter()
            .filter(|registration| registration.expose_in_llm())
            .map(|registration| (registration.name(), registration.capability()))
            .collect();

    let declarations = build_function_declarations_cached(ToolDocumentationMode::default());

    declarations
        .iter()
        .filter(|fd| {
            tool_capabilities
                .get(fd.name.as_str())
                .map(|required| level >= *required)
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}
fn apply_metadata_overrides(declarations: &mut [FunctionDeclaration]) {
    let registrations = builtin_tool_registrations(None);
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
        insert_if_allowed(map, 
            "x-default-permission".to_string(),
            json!(permission_label(&permission)),
        );
    }

    if !meta.aliases().is_empty() {
        insert_if_allowed(map, "x-aliases".to_string(), json!(meta.aliases().to_vec()));
    }

    if let Some(schema) = meta.config_schema() {
        insert_if_allowed(map, "x-config-schema".to_string(), schema.clone());
    }

    if let Some(schema) = meta.state_schema() {
        insert_if_allowed(map, "x-state-schema".to_string(), schema.clone());
    }
}

fn permission_label(permission: &ToolPolicy) -> &'static str {
    match permission {
        ToolPolicy::Allow => "allow",
        ToolPolicy::Deny => "deny",
        ToolPolicy::Prompt => "prompt",
    }
}

#[cfg(test)]
mod tests {
    use super::build_function_declarations_with_mode;
    use crate::config::constants::tools;
    use crate::config::types::ToolDocumentationMode;
    use std::collections::BTreeSet;

    #[test]
    fn all_modes_expose_same_tool_names() {
        let full: BTreeSet<String> =
            build_function_declarations_with_mode(ToolDocumentationMode::Full)
                .into_iter()
                .map(|decl| decl.name)
                .collect();
        let minimal: BTreeSet<String> =
            build_function_declarations_with_mode(ToolDocumentationMode::Minimal)
                .into_iter()
                .map(|decl| decl.name)
                .collect();
        let progressive: BTreeSet<String> =
            build_function_declarations_with_mode(ToolDocumentationMode::Progressive)
                .into_iter()
                .map(|decl| decl.name)
                .collect();

        assert_eq!(full, minimal);
        assert_eq!(full, progressive);
    }

    #[test]
    fn unified_exec_schema_advertises_inspect_action() {
        let declarations = build_function_declarations_with_mode(ToolDocumentationMode::Full);
        let unified_exec = declarations
            .iter()
            .find(|decl| decl.name == tools::UNIFIED_EXEC)
            .expect("unified_exec declaration");

        let actions = unified_exec.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert!(
            actions
                .iter()
                .any(|value| value.as_str() == Some("inspect"))
        );
    }

    #[test]
    fn unified_search_schema_hides_intelligence_action() {
        let declarations = build_function_declarations_with_mode(ToolDocumentationMode::Full);
        let unified_search = declarations
            .iter()
            .find(|decl| decl.name == tools::UNIFIED_SEARCH)
            .expect("unified_search declaration");

        let actions = unified_search.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum");
        assert!(
            !actions
                .iter()
                .any(|value| value.as_str() == Some("intelligence"))
        );
    }
}

fn insert_if_allowed(map: &mut serde_json::Map<String, Value>, key: String, value: Value) {
    if !key.starts_with("x-") {
        map.insert(key, value);
    }
}
