use std::collections::HashMap;

use crate::config::constants::tools;
use crate::config::types::CapabilityLevel;
use crate::gemini::FunctionDeclaration;
use serde_json::json;

use super::builtins::builtin_tool_registrations;

fn base_function_declarations() -> Vec<FunctionDeclaration> {
    vec![
        // Ripgrep search tool
        FunctionDeclaration {
            name: tools::GREP_SEARCH.to_string(),
            description: "Fast code search using ripgrep. Find code patterns, function definitions, TODOs, or text across files. Supports exact/fuzzy/multi-pattern/similarity modes. Use 'concise' format and reasonable max_results to manage token budget. Search first, read second—avoid loading full files until you've identified relevant matches.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Search pattern. Example: 'fn \\w+' or 'TODO|FIXME'"},
                    "path": {"type": "string", "description": "Directory path to search in (relative). Default: '.'", "default": "."},
                    "mode": {"type": "string", "description": "Search mode: 'exact' | 'fuzzy' | 'multi' | 'similarity'", "default": "exact"},
                    "max_results": {"type": "integer", "description": "Max results (token efficiency). Default: 100", "default": 100},
                    "case_sensitive": {"type": "boolean", "description": "Case sensitive search. Default: true", "default": true},
                    // Multi-pattern search parameters
                    "patterns": {"type": "array", "items": {"type": "string"}, "description": "For mode='multi'. Example: ['fn \\w+','use \\w+']"},
                    "logic": {"type": "string", "description": "For mode='multi': 'AND' or 'OR'", "default": "AND"},
                    // Fuzzy search parameters
                    "fuzzy_threshold": {"type": "number", "description": "Fuzzy matching threshold (0.0-1.0)", "default": 0.7},
                    // Similarity search parameters
                    "reference_file": {"type": "string", "description": "For mode='similarity': reference file path"},
                    "content_type": {"type": "string", "description": "For mode='similarity': 'structure'|'imports'|'functions'|'all'", "default": "all"},
                    "response_format": {"type": "string", "description": "'concise' (default) or 'detailed' (raw rg JSON)", "default": "concise"}
                },
                "required": ["pattern"]
            }),
        },

        // Consolidated file operations tool
        FunctionDeclaration {
            name: "list_files".to_string(),
            description: "Explore workspace structure. Modes: 'list' (dir contents), 'recursive' (deep scan), 'find_name' (filename search), 'find_content' (content-based discovery). Use metadata as references—avoid reading full contents until necessary. Always paginate large dirs (per_page=50 default). Monitor 'has_more' flag. Use 'concise' format to minimize token usage.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path to search from (relative). Example: 'src'"},
                    "mode": {"type": "string", "description": "'list' | 'recursive' | 'find_name' | 'find_content'", "default": "list"},
                    "max_items": {"type": "integer", "description": "Cap total items scanned (token safety). Default: 1000", "default": 1000},
                    "page": {"type": "integer", "description": "Page number (1-based). Default: 1", "default": 1},
                    "per_page": {"type": "integer", "description": "Items per page. Default: 50", "default": 50},
                    "response_format": {"type": "string", "description": "'concise' (default) omits low-signal fields; 'detailed' includes them", "default": "concise"},
                    "include_hidden": {"type": "boolean", "description": "Include hidden files", "default": false},
                    "name_pattern": {"type": "string", "description": "Optional pattern for 'recursive'/'find_name' modes. Use '*' or omit for all files. Example: '*.rs'", "default": "*"},
                    "content_pattern": {"type": "string", "description": "For 'find_content' mode. Example: 'fn main'"},
                    "file_extensions": {"type": "array", "items": {"type": "string"}, "description": "Filter by file extensions"},
                    "case_sensitive": {"type": "boolean", "description": "Case sensitive pattern matching", "default": true},
                    "ast_grep_pattern": {"type": "string", "description": "Optional AST pattern to filter files"}
                },
                "required": ["path"]
            }),
        },

        // File reading tool
        FunctionDeclaration {
            name: tools::READ_FILE.to_string(),
            description: "Read file contents. Auto-chunks large files (>2000 lines): first 800 + last 800 lines. Adjust with chunk_lines/max_lines. Only read after search identifies relevance.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to read"},
                    "max_bytes": {"type": "integer", "description": "Maximum bytes to read (optional)", "default": null},
                    "chunk_lines": {"type": "integer", "description": "Line threshold for chunking (optional, default: 2000)", "default": 2000},
                    "max_lines": {"type": "integer", "description": "Alternative parameter for chunk_lines (optional)", "default": null}
                },
                "required": ["path"]
            }),
        },

        // File writing tool
        FunctionDeclaration {
            name: tools::WRITE_FILE.to_string(),
            description: "Create or modify files. Modes: 'overwrite' (replace), 'append' (add to end), 'skip_if_exists' (safe create). Ensure directory exists first. Validate after write.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to write to"},
                    "content": {"type": "string", "description": "Content to write to the file"},
                    "mode": {"type": "string", "description": "Write mode: 'overwrite' (default) or 'append'", "default": "overwrite"}
                },
                "required": ["path", "content"]
            }),
        },

        // File editing tool
        FunctionDeclaration {
            name: tools::EDIT_FILE.to_string(),
            description: "Precise text replacement via exact string match. Read file first to capture old_str with exact whitespace/indentation. Preferred over write_file for targeted changes. Cannot handle pattern-based refactoring—use ast_grep for that.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to edit"},
                    "old_str": {"type": "string", "description": "Exact text to replace (must match exactly)"},
                    "new_str": {"type": "string", "description": "New text to replace with"}
                },
                "required": ["path", "old_str", "new_str"]
            }),
        },

        // Consolidated command execution tool
        FunctionDeclaration {
            name: tools::RUN_TERMINAL_CMD.to_string(),
            description: "Execute shell commands. Auto-truncates large output (>10k lines): first 5k + last 5k. Mode: 'terminal' (default). Requests for 'pty' are delegated to the interactive bash tool. Set timeouts. Prefer specialized tools for file ops.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "array", "items": {"type": "string"}, "description": "Program + args as array"},
                    "working_dir": {"type": "string", "description": "Working directory relative to workspace"},
                    "timeout_secs": {"type": "integer", "description": "Command timeout in seconds (default: 30)", "default": 30},
                    "mode": {"type": "string", "description": "Execution mode: 'terminal' | 'pty' (delegates to interactive shell)", "default": "terminal"},
                    "response_format": {"type": "string", "description": "'concise' (default) or 'detailed'", "default": "concise"}
                },
                "required": ["command"]
            }),
        },
        FunctionDeclaration {
            name: tools::RUN_PTY_CMD.to_string(),
            description: "Execute a command inside a pseudo-terminal. Use for interactive programs (e.g., REPLs, full-screen TUIs) that require TTY semantics. Provide the command as a string plus optional args array or as an array of program + args.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "description": "Command to run (string or [program, ...args])",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Arguments to append when command is provided as string"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory relative to the workspace"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Timeout before forcefully terminating the command",
                        "default": 300
                    },
                    "rows": {
                        "type": "integer",
                        "description": "Pseudo-terminal rows",
                        "default": 24
                    },
                    "cols": {
                        "type": "integer",
                        "description": "Pseudo-terminal columns",
                        "default": 80
                    }
                },
                "required": ["command"]
            }),
        },
        FunctionDeclaration {
            name: tools::CREATE_PTY_SESSION.to_string(),
            description: "Spawn and register a persistent PTY session (e.g., bash shell) that can be reused across tool calls. Requires a unique session_id.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Unique identifier for the PTY session"},
                    "command": {
                        "description": "Command to start in the session (string or [program, ...args])",
                        "oneOf": [
                            {"type": "string"},
                            {"type": "array", "items": {"type": "string"}}
                        ]
                    },
                    "args": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "Arguments to append when command is provided as string"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory relative to the workspace"
                    },
                    "rows": {
                        "type": "integer",
                        "description": "Pseudo-terminal rows",
                        "default": 24
                    },
                    "cols": {
                        "type": "integer",
                        "description": "Pseudo-terminal columns",
                        "default": 80
                    }
                },
                "required": ["session_id", "command"]
            }),
        },
        FunctionDeclaration {
            name: tools::LIST_PTY_SESSIONS.to_string(),
            description: "List active PTY sessions created via create_pty_session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }),
        },
        FunctionDeclaration {
            name: tools::CLOSE_PTY_SESSION.to_string(),
            description: "Terminate and remove a persistent PTY session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Identifier returned by create_pty_session"}
                },
                "required": ["session_id"]
            }),
        },
        FunctionDeclaration {
            name: tools::SEND_PTY_INPUT.to_string(),
            description: "Send keystrokes or raw bytes to an existing PTY session and optionally wait for new output.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Identifier returned by create_pty_session"},
                    "input": {"type": "string", "description": "UTF-8 text to send to the session (use input_base64 for binary/control sequences)."},
                    "input_base64": {"type": "string", "description": "Base64 encoded bytes to send to the session."},
                    "append_newline": {"type": "boolean", "description": "Append a newline after sending input (simulates pressing Enter).", "default": false},
                    "wait_ms": {"type": "integer", "description": "Milliseconds to wait after writing before capturing output.", "default": 0},
                    "drain": {"type": "boolean", "description": "When true, return and clear newly produced output from the session buffer.", "default": true}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
        FunctionDeclaration {
            name: tools::READ_PTY_SESSION.to_string(),
            description: "Inspect the current state of a PTY session, including optional scrollback and live screen contents.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Identifier returned by create_pty_session"},
                    "drain": {"type": "boolean", "description": "When true, return and clear newly produced output since the last drain.", "default": false},
                    "include_screen": {"type": "boolean", "description": "Include the parsed screen buffer snapshot.", "default": true},
                    "include_scrollback": {"type": "boolean", "description": "Include retained scrollback history.", "default": true}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
        FunctionDeclaration {
            name: tools::RESIZE_PTY_SESSION.to_string(),
            description: "Adjust the terminal rows and columns for an active PTY session.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "session_id": {"type": "string", "description": "Identifier returned by create_pty_session"},
                    "rows": {"type": "integer", "description": "New terminal row count", "minimum": 1},
                    "cols": {"type": "integer", "description": "New terminal column count", "minimum": 1}
                },
                "required": ["session_id"],
                "additionalProperties": false
            }),
        },
        FunctionDeclaration {
            name: tools::CURL.to_string(),
            description: "Fetch HTTPS content (sandboxed). Public hosts only—blocks localhost/private IPs. Size-limited. Returns security_notice. Use for documentation or small JSON payloads.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "HTTPS URL to fetch (public hosts only)."},
                    "method": {"type": "string", "description": "HTTP method: 'GET' (default) or 'HEAD'.", "default": "GET"},
                    "max_bytes": {"type": "integer", "description": "Maximum response bytes to read (must respect policy cap).", "default": 65536},
                    "timeout_secs": {"type": "integer", "description": "Request timeout in seconds (<=30)", "default": 10},
                    "save_response": {"type": "boolean", "description": "When true, saves the body to /tmp/vtcode-curl and returns the path so you can inspect then delete it.", "default": false}
                },
                "required": ["url"]
            }),
        },

        // AST-grep search and transformation tool
        FunctionDeclaration {
            name: tools::AST_GREP_SEARCH.to_string(),
            description: "Syntax-aware code search and refactoring using AST patterns. Operations: 'search', 'transform', 'lint', 'refactor'. Use when text search is insufficient (structural patterns, multi-lang support). Preview transforms before applying. Set reasonable limits for token efficiency.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "description": "Operation type: 'search', 'transform', 'lint', 'refactor', 'custom'", "default": "search"},
                    "pattern": {"type": "string", "description": "AST-grep pattern to search for"},
                    "path": {"type": "string", "description": "File or directory path to search in", "default": "."},
                    "language": {"type": "string", "description": "Programming language (auto-detected if not specified)"},
                    "replacement": {"type": "string", "description": "Replacement pattern for transform operations"},
                    "refactor_type": {"type": "string", "description": "Type of refactoring: 'extract_function', 'remove_console_logs', 'simplify_conditions', 'extract_constants', 'modernize_syntax'"},
                    "context_lines": {"type": "integer", "description": "Number of context lines to show", "default": 0},
                    "max_results": {"type": "integer", "description": "Maximum number of results", "default": 100},
                    "preview_only": {"type": "boolean", "description": "Preview changes without applying (transform only)", "default": true},
                    "update_all": {"type": "boolean", "description": "Update all matches (transform only)", "default": false},
                    "interactive": {"type": "boolean", "description": "Interactive mode (custom only)", "default": false},
                    "severity_filter": {"type": "string", "description": "Filter lint results by severity"}
                },
                "required": ["pattern", "path"]
            }),
        },

        // Simple bash-like search tool
        FunctionDeclaration {
            name: tools::SIMPLE_SEARCH.to_string(),
            description: "Provides simple bash-like file operations and searches for quick, straightforward tasks. This tool offers direct access to common Unix commands like grep, find, ls, cat, head, tail, and file indexing. Use this tool when you need basic file operations without the complexity of advanced search features. It is ideal for quick file content previews, directory listings, or simple pattern matching. The tool supports various commands: 'grep' for text searching, 'find' for file discovery, 'ls' for directory listing, 'cat' for full file reading, 'head'/'tail' for partial file reading, and 'index' for file indexing. This tool is less powerful than specialized search tools but provides fast, intuitive access to common operations. Use appropriate max_results limits to prevent excessive output, especially with recursive operations.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "Command to execute: 'grep', 'find', 'ls', 'cat', 'head', 'tail', 'index'", "default": "grep"},
                    "pattern": {"type": "string", "description": "Search pattern for grep/find commands"},
                    "file_pattern": {"type": "string", "description": "File pattern filter for grep"},
                    "file_path": {"type": "string", "description": "File path for cat/head/tail commands"},
                    "path": {"type": "string", "description": "Directory path for ls/find/index commands", "default": "."},
                    "start_line": {"type": "integer", "description": "Start line number for cat command"},
                    "end_line": {"type": "integer", "description": "End line number for cat command"},
                    "lines": {"type": "integer", "description": "Number of lines for head/tail commands", "default": 10},
                    "max_results": {"type": "integer", "description": "Maximum results to return", "default": 50},
                    "show_hidden": {"type": "boolean", "description": "Show hidden files for ls command", "default": false}
                },
                "required": []
            }),
        },

        // Bash-like command tool
        FunctionDeclaration {
            name: tools::BASH.to_string(),
            description: "Executes bash-like commands through a pseudo-terminal interface for interactive operations. This tool provides access to common shell commands with enhanced terminal emulation. Use this tool when you need interactive command execution, complex shell pipelines, or commands that require a proper terminal environment. It supports essential commands like 'ls' for directory listing, 'pwd' for current directory, 'grep' for text search, 'find' for file discovery, 'cat'/'head'/'tail' for file content viewing, and file manipulation commands like 'mkdir', 'rm', 'cp', 'mv'. The tool includes safety restrictions and should be used as a complement to specialized tools rather than a replacement. Prefer 'run_terminal_cmd' for non-interactive commands and file/search tools for file operations. Always use appropriate flags and be aware of the recursive and force options which can affect multiple files.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "bash_command": {"type": "string", "description": "Bash command to execute: 'ls', 'pwd', 'grep', 'find', 'cat', 'head', 'tail', 'mkdir', 'rm', 'cp', 'mv', 'stat', 'run'", "default": "ls"},
                    "path": {"type": "string", "description": "Path for file/directory operations"},
                    "source": {"type": "string", "description": "Source path for cp/mv operations"},
                    "dest": {"type": "string", "description": "Destination path for cp/mv operations"},
                    "pattern": {"type": "string", "description": "Search pattern for grep/find"},
                    "recursive": {"type": "boolean", "description": "Recursive operation", "default": false},
                    "show_hidden": {"type": "boolean", "description": "Show hidden files", "default": false},
                    "parents": {"type": "boolean", "description": "Create parent directories", "default": false},
                    "force": {"type": "boolean", "description": "Force operation", "default": false},
                    "lines": {"type": "integer", "description": "Number of lines for head/tail", "default": 10},
                    "start_line": {"type": "integer", "description": "Start line for cat"},
                    "end_line": {"type": "integer", "description": "End line for cat"},
                    "name_pattern": {"type": "string", "description": "Name pattern for find"},
                    "type_filter": {"type": "string", "description": "Type filter for find (f=file, d=directory)"},
                    "command": {"type": "string", "description": "Command to run for arbitrary execution"},
                    "args": {"type": "array", "items": {"type": "string"}, "description": "Arguments for command execution"}
                },
                "required": []
            }),
        },

        // Apply patch tool (Codex patch format)
        FunctionDeclaration {
            name: tools::APPLY_PATCH.to_string(),
            description: "Applies Codex-style patch blocks to modify multiple files in the workspace. This tool is specialized for applying structured patches that contain changes to multiple files or complex modifications. Use this tool when you receive patch content in the Codex format (marked with '*** Begin Patch' and '*** End Patch') instead of making individual file edits. The tool parses the patch format, validates the changes, and applies them atomically to prevent partial updates. It is particularly useful for applying code review suggestions, automated refactoring changes, or complex multi-file modifications. The tool provides detailed feedback on which files were modified and any issues encountered during application. Always ensure the patch content is complete and properly formatted before using this tool.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "input": {"type": "string", "description": "Patch content in Codex patch format"}
                },
                "required": ["input"]
            }),
        },
        FunctionDeclaration {
            name: tools::UPDATE_PLAN.to_string(),
            description: "Records or updates the agent's current multi-step plan. Provide a concise explanation (optional) and a list of plan steps with their status. Exactly one step may be marked 'in_progress'; all other steps must be 'pending' or 'completed'. Use this tool to keep the user informed about your approach for complex tasks, render the plan as a Markdown TODO list with checkboxes, and update it whenever progress changes.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "explanation": {
                        "type": "string",
                        "description": "Optional summary explaining the plan or changes made."
                    },
                    "plan": {
                        "type": "array",
                        "description": "Ordered list of plan steps with status metadata.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {
                                    "type": "string",
                                    "description": "Description of the work to perform."
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "completed"],
                                    "description": "Current state of the step."
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
