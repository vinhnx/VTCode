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
            name: tools::RUN_PTY_CMD.to_string(),
            description: "Execute shell commands (git, cargo, npm). PTY terminal emulation. Respects command policies. Output auto-truncated.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "description": "Shell command (string or array).",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    },
                    "cwd": {"type": "string", "description": "Working directory."},
                    "timeout_secs": {"type": "integer", "description": "Timeout (seconds).", "default": 180},
                    "confirm": {"type": "boolean", "description": "Confirm destructive ops.", "default": false},
                    "max_tokens": {"type": "integer", "description": "Max output tokens before truncation."}
                },
                "required": ["command"]
            }),
        },

        FunctionDeclaration {
            name: tools::SEARCH_TOOLS.to_string(),
            description: "Search MCP tools by keyword. Returns names, descriptions, or full schemas based on detail_level.".to_string(),
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
            description: "Load a skill by name. Skills are subagents with instructions and scripts from .claude/skills/.".to_string(),
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
            description: "Execute Python or JavaScript code. Access MCP tools as functions. Return results via `result = {...}`.".to_string(),
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
            name: tools::AGENT_INFO.to_string(),
            description: "Agent diagnostics: tools, workspace, usage stats. mode: debug|analyze|full.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["debug", "analyze", "full"],
                        "description": "debug (config/state), analyze (metrics), full (both)",
                        "default": "full"
                    }
                }
            }),
        },

        // ============================================================
        // FILE OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::READ_FILE.to_string(),
            description: "Read file contents. Returns content/status/message JSON. Supports chunked reads. Don't retry if status='success'.".to_string(),
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
            description: "Apply unified diff patches to files. Create, update, or delete content using *** Begin/End Patch markers.".to_string(),
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

        // NOTE: search_replace removed - use edit_file instead
        // NOTE: PTY session tools (create/list/close/send/read/resize) hidden from LLM - use run_pty_cmd

        // ============================================================
        // NETWORK OPERATIONS
        // ============================================================
        FunctionDeclaration {
            name: tools::WEB_FETCH.to_string(),
            description: "Fetch URL content, convert HTML to markdown, process with prompt. Returns AI-analyzed web content.".to_string(),
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
            description: "Update task plan. Phases: understanding, design (3-7 steps with file:line), review, final_plan.".to_string(),
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
                            "anyOf": [
                                {
                                    "type": "string",
                                    "description": "Simple step description"
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "step": {"type": "string", "description": "Step description with file path and line numbers"},
                                        "status": {
                                            "type": "string",
                                            "enum": ["pending", "in_progress", "completed"],
                                            "description": "Step status (defaults to 'pending' if not specified)"
                                        }
                                    },
                                    "required": ["step"],
                                    "additionalProperties": false
                                }
                            ]
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
