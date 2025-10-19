use std::collections::HashMap;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use serde_json::json;

use super::builtins::builtin_tool_registrations;

fn base_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        // ============================================================
        // SEARCH & DISCOVERY TOOLS
        // ============================================================
        FunctionDeclaration {
            name: tools::GREP_FILE.to_string(),
            description: "Search code using ripgrep. Find patterns, functions, TODOs across files. Modes: exact|fuzzy|multi|similarity. Use concise format for efficiency.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Search pattern (e.g. 'fn \\w+', 'TODO')"},
                    "path": {"type": "string", "description": "Directory path (relative)", "default": "."},
                    "mode": {"type": "string", "description": "exact|fuzzy|multi|similarity", "default": "exact"},
                    "max_results": {"type": "integer", "description": "Maximum results", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case-sensitive matching", "default": true},
                    "patterns": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Multiple patterns for multi mode"
                    },
                    "logic": {"type": "string", "description": "AND|OR logic", "default": "AND"},
                    "fuzzy_threshold": {"type": "number", "description": "Threshold 0.0-1.0", "default": 0.7},
                    "reference_file": {"type": "string", "description": "File for similarity mode"},
                    "content_type": {"type": "string", "description": "structure|imports|functions|all", "default": "all"},
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
            name: tools::WRITE_FILE.to_string(),
            description: "Create or modify files. Modes: overwrite|append.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "content": {"type": "string", "description": "File content"},
                    "mode": {"type": "string", "description": "overwrite|append", "default": "overwrite"}
                },
                "required": ["path", "content"]
            }),
        },

        FunctionDeclaration {
            name: tools::EDIT_FILE.to_string(),
            description: "Precise text replacement via exact string match. Preferred for targeted changes.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "old_str": {"type": "string", "description": "Text to replace (exact match)"},
                    "new_str": {"type": "string", "description": "Replacement text"}
                },
                "required": ["path", "old_str", "new_str"]
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
        // TERMINAL EXECUTION
        // ============================================================
        FunctionDeclaration {
            name: tools::RUN_TERMINAL_CMD.to_string(),
            description: "Execute shell commands. Auto-truncates large output (>10k lines). Mode: terminal|pty.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "description": "Command (array or string)",
                        "oneOf": [
                            {"type": "array", "items": {"type": "string"}},
                            {"type": "string"}
                        ]
                    },
                    "working_dir": {"type": "string", "description": "Working directory"},
                    "cwd": {"type": "string", "description": "Alias for working_dir"},
                    "timeout_secs": {"type": "integer", "description": "Timeout seconds", "default": 30},
                    "timeout": {
                        "oneOf": [{"type": "integer"}, {"type": "number"}],
                        "description": "Alias for timeout_secs"
                    },
                    "mode": {"type": "string", "description": "terminal|pty", "default": "terminal"},
                    "tty": {"type": "boolean", "description": "Alias for mode=pty"},
                    "response_format": {"type": "string", "description": "concise|detailed", "default": "concise"},
                    "shell": {"type": "string", "description": "Shell executable"},
                    "login": {"type": "boolean", "description": "Use login shell semantics"}
                },
                "required": ["command"]
            }),
        },

        // ============================================================
        // PTY SESSION MANAGEMENT
        // ============================================================
        FunctionDeclaration {
            name: tools::RUN_PTY_CMD.to_string(),
            description: "Execute command in pseudo-terminal (for interactive programs like REPLs).".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
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
                    "timeout_secs": {"type": "integer", "description": "Timeout seconds", "default": 300},
                    "rows": {"type": "integer", "description": "Terminal rows", "default": 24},
                    "cols": {"type": "integer", "description": "Terminal columns", "default": 80}
                },
                "required": ["command"]
            }),
        },

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
        // SIMPLE UTILITIES
        // ============================================================
        FunctionDeclaration {
            name: tools::SIMPLE_SEARCH.to_string(),
            description: "Simple bash-like operations: grep|find|ls|cat|head|tail|index. Quick utility tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "grep|find|ls|cat|head|tail|index", "default": "grep"},
                    "pattern": {"type": "string", "description": "Search pattern"},
                    "file_pattern": {"type": "string", "description": "File filter"},
                    "file_path": {"type": "string", "description": "File path"},
                    "path": {"type": "string", "description": "Directory path", "default": "."},
                    "start_line": {"type": "integer", "description": "Start line"},
                    "end_line": {"type": "integer", "description": "End line"},
                    "lines": {"type": "integer", "description": "Line count", "default": 10},
                    "max_results": {"type": "integer", "description": "Max results", "default": 50},
                    "show_hidden": {"type": "boolean", "description": "Show hidden files", "default": false}
                },
                "required": []
            }),
        },

        FunctionDeclaration {
            name: tools::BASH.to_string(),
            description: "Execute bash commands via pseudo-terminal. Commands: ls|pwd|grep|find|cat|head|tail|mkdir|rm|cp|mv|stat|run.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "bash_command": {"type": "string", "description": "Command type", "default": "ls"},
                    "path": {"type": "string", "description": "Target path"},
                    "source": {"type": "string", "description": "Source path"},
                    "dest": {"type": "string", "description": "Destination path"},
                    "pattern": {"type": "string", "description": "Search pattern"},
                    "recursive": {"type": "boolean", "description": "Recursive operation", "default": false},
                    "show_hidden": {"type": "boolean", "description": "Show hidden files", "default": false},
                    "parents": {"type": "boolean", "description": "Create parents", "default": false},
                    "force": {"type": "boolean", "description": "Force operation", "default": false},
                    "lines": {"type": "integer", "description": "Line count", "default": 10},
                    "start_line": {"type": "integer", "description": "Start line"},
                    "end_line": {"type": "integer", "description": "End line"},
                    "name_pattern": {"type": "string", "description": "Name pattern"},
                    "type_filter": {"type": "string", "description": "Type (f|d)"},
                    "command": {"type": "string", "description": "Command for run"},
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Arguments"
                    }
                },
                "required": []
            }),
        },

        // ============================================================
        // PATCH & PLANNING
        // ============================================================
        FunctionDeclaration {
            name: tools::APPLY_PATCH.to_string(),
            description: "Apply Codex-style patch blocks to multiple files atomically.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Patch content"}
                },
                "required": ["input"]
            }),
        },

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
