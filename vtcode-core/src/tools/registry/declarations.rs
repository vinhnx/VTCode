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
            description: "Fast regex-based code search using ripgrep (replaces ast-grep). Find patterns, functions, definitions, TODOs, errors, imports, and API calls across files. Respects .gitignore/.ignore by default. Supports glob patterns, file-type filtering, context lines, and regex/literal matching. Essential for code navigation and analysis.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern or literal string to search for. Examples: 'fn \\\\w+\\\\(', 'TODO|FIXME', '^import\\\\s', '\\\\.get\\\\(' for HTTP verbs"},
                    "path": {"type": "string", "description": "Directory path (relative). Defaults to current directory", "default": "."},
                    "max_results": {"type": "integer", "description": "Maximum number of results to return (1-1000)", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching. Default uses smart-case: lowercase pattern = case-insensitive, with uppercase = case-sensitive", "default": false},
                    "literal": {"type": "boolean", "description": "Treat pattern as literal string, not regex. Use for exact string matching", "default": false},
                    "glob_pattern": {"type": "string", "description": "Filter files by glob pattern. Examples: '**/*.rs' (all Rust), 'src/**/*.ts' (TypeScript in src), '*.test.js'"},
                    "context_lines": {"type": "integer", "description": "Lines of context before/after matches (0-20). Use 3-5 to see surrounding code for understanding. Default 0 for concise output", "default": 0},
                    "respect_ignore_files": {"type": "boolean", "description": "Respect .gitignore and .ignore files. Set false to search all files including ignored ones", "default": true},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files (those starting with dot) in search results", "default": false},
                    "max_file_size": {"type": "integer", "description": "Maximum file size to search in bytes. Skips files larger than this. Example: 5242880 for 5MB"},
                    "search_hidden": {"type": "boolean", "description": "Search inside hidden directories (those starting with dot)", "default": false},
                    "search_binary": {"type": "boolean", "description": "Search binary files. Usually false to avoid noise from compiled code/media", "default": false},
                    "files_with_matches": {"type": "boolean", "description": "Return only filenames containing matches, not the match lines themselves", "default": false},
                    "type_pattern": {"type": "string", "description": "Filter by file type: 'rust', 'python', 'typescript', 'javascript', 'java', 'go', etc. Faster than glob for language filtering"},
                    "invert_match": {"type": "boolean", "description": "Invert matching: return lines that do NOT match the pattern", "default": false},
                    "word_boundaries": {"type": "boolean", "description": "Match only at word boundaries (\\\\b in regex). Prevents partial word matches", "default": false},
                    "line_number": {"type": "boolean", "description": "Include line numbers in output. Recommended true for file navigation", "default": true},
                    "column": {"type": "boolean", "description": "Include column numbers in output for precise positioning", "default": false},
                    "only_matching": {"type": "boolean", "description": "Show only the matched part of each line, not the full line", "default": false},
                    "trim": {"type": "boolean", "description": "Trim leading/trailing whitespace from output lines", "default": false},
                    "response_format": {"type": "string", "description": "Output format: 'concise' (compact JSON) or 'detailed' (with metadata)", "default": "concise"}
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
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching", "default": true}
                },
                "required": ["path"]
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
            name: tools::EXECUTE_CODE.to_string(),
            description: "Execute Python or JavaScript code in a sandboxed environment with access to MCP tools as library functions. Supports loops, conditionals, data filtering, and aggregation. Results are returned as JSON via `result = {...}` assignment.".to_string(),
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
            name: tools::GET_ERRORS.to_string(),
            description: "Aggregate recent error traces from session archives and tool outputs. Useful for diagnosing runtime failures, patterns, and suggested recovery actions. Use 'scope' to specify 'archive' or 'session' and 'limit' to control the number of sessions to analyze.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "scope": {"type": "string", "description": "Scope to analyze: 'archive' or 'session'", "default": "archive"},
                    "limit": {"type": "integer", "description": "How many recent sessions to analyze for errors", "default": 5}
                }
            }),
        },
        FunctionDeclaration {
            name: tools::DEBUG_AGENT.to_string(),
            description: "Return a lightweight diagnostic snapshot of the agent environment and available tools; useful for quick introspection.".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },
        FunctionDeclaration {
            name: tools::ANALYZE_AGENT.to_string(),
            description: "Return a brief analysis summary for agent behavior such as tool usage counts and available tools for diagnosing behavior patterns.".to_string(),
            parameters: json!({"type": "object", "properties": {}}),
        },

        FunctionDeclaration {
            name: "save_skill".to_string(),
            description: "Save a reusable skill (code function) to .vtcode/skills/ for later use. Skills can be loaded across conversations.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name in snake_case (e.g., 'filter_test_files')"},
                    "code": {"type": "string", "description": "Function implementation (Python 3 or JavaScript)"},
                    "language": {
                        "type": "string",
                        "enum": ["python3", "javascript"],
                        "description": "Programming language"
                    },
                    "description": {"type": "string", "description": "Brief description of what the skill does"},
                    "inputs": {
                        "type": "array",
                        "description": "List of input parameters",
                        "items": {
                            "type": "object",
                            "properties": {
                                "name": {"type": "string"},
                                "type": {"type": "string"},
                                "description": {"type": "string"},
                                "required": {"type": "boolean"}
                            }
                        }
                    },
                    "output": {"type": "string", "description": "What the skill returns"},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Tags for categorizing skills (e.g., ['files', 'filtering'])"
                    },
                    "examples": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Usage examples"
                    }
                },
                "required": ["name", "code", "language", "description", "output"]
            }),
        },

        FunctionDeclaration {
            name: "load_skill".to_string(),
            description: "Load a saved skill by name and get its code and documentation.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Skill name to load"}
                },
                "required": ["name"]
            }),
        },

        FunctionDeclaration {
            name: "list_skills".to_string(),
            description: "List all available saved skills in the workspace.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },

        FunctionDeclaration {
            name: "search_skills".to_string(),
            description: "Search for skills by keyword or tag.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "Search term (skill name, description, or tag)"}
                },
                "required": ["query"]
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
            description: "Track multi-step plan with status (pending|in_progress|completed). Follow GPT-5.1 format: 2-5 milestone items with one in_progress at a time for complex tasks.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "explanation": {"type": "string", "description": "Plan summary"},
                    "plan": {
                        "type": "array",
                        "description": "Plan steps with status, following GPT-5.1 recommended format: 2-5 milestone items with one in_progress at a time",
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
