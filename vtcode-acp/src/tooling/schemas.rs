use std::path::Path;

use serde_json::json;

use vtcode_core::config::constants::tools;
use vtcode_core::core::interfaces::SessionMode;
use vtcode_core::llm::provider::ToolDefinition;

pub const TOOL_READ_FILE_DESCRIPTION: &str =
    "Read the contents of a text file accessible to the IDE workspace";
pub const TOOL_READ_FILE_URI_ARG: &str = "uri";
pub const TOOL_READ_FILE_PATH_ARG: &str = "path";
pub const TOOL_READ_FILE_LINE_ARG: &str = "line";
pub const TOOL_READ_FILE_LIMIT_ARG: &str = "limit";

pub const TOOL_LIST_FILES_DESCRIPTION: &str = "Explore workspace files in a SUBDIRECTORY (root path is blocked). Requires path like 'src/' or 'vtcode-core/'. For root overview, use shell commands via unified_exec.";
pub const TOOL_LIST_FILES_PATH_ARG: &str = "path";
pub const TOOL_LIST_FILES_MODE_ARG: &str = "mode";
pub const TOOL_LIST_FILES_PAGE_ARG: &str = "page";
pub const TOOL_LIST_FILES_PER_PAGE_ARG: &str = "per_page";
pub const TOOL_LIST_FILES_MAX_ITEMS_ARG: &str = "max_items";
pub const TOOL_LIST_FILES_INCLUDE_HIDDEN_ARG: &str = "include_hidden";
pub const TOOL_LIST_FILES_RESPONSE_FORMAT_ARG: &str = "response_format";
pub const TOOL_LIST_FILES_URI_ARG: &str = "uri";
pub const TOOL_LIST_FILES_NAME_PATTERN_ARG: &str = "name_pattern";
pub const TOOL_LIST_FILES_CONTENT_PATTERN_ARG: &str = "content_pattern";
pub const TOOL_LIST_FILES_FILE_EXTENSIONS_ARG: &str = "file_extensions";
pub const TOOL_LIST_FILES_CASE_SENSITIVE_ARG: &str = "case_sensitive";
pub const TOOL_LIST_FILES_ITEMS_KEY: &str = "items";
pub const TOOL_LIST_FILES_MESSAGE_KEY: &str = "message";
pub const TOOL_LIST_FILES_RESULT_KEY: &str = "result";
pub const TOOL_LIST_FILES_SUMMARY_MAX_ITEMS: usize = 20;

pub(super) fn build_read_file_definition(workspace_root: &Path) -> ToolDefinition {
    let workspace_display = workspace_root.display().to_string();
    let sample_path = workspace_root.join("README.md");
    let sample_path_string = sample_path.to_string_lossy().into_owned();
    let sample_uri = format!("file://{}", sample_path_string);
    let description = format!(
        "{TOOL_READ_FILE_DESCRIPTION}. Workspace root: {workspace}. Provide {path} or {uri} inside the workspace. Paths must be absolute (see ACP file system spec). Optional {line} and {limit} control slicing.",
        workspace = workspace_display,
        path = TOOL_READ_FILE_PATH_ARG,
        uri = TOOL_READ_FILE_URI_ARG,
        line = TOOL_READ_FILE_LINE_ARG,
        limit = TOOL_READ_FILE_LIMIT_ARG,
    );
    let examples = vec![
        json!({
            TOOL_READ_FILE_PATH_ARG: &sample_path_string,
        }),
        json!({
            TOOL_READ_FILE_PATH_ARG: &sample_path_string,
            TOOL_READ_FILE_LINE_ARG: 1,
            TOOL_READ_FILE_LIMIT_ARG: 200,
        }),
        json!({
            TOOL_READ_FILE_URI_ARG: sample_uri,
        }),
    ];
    let schema = json!({
        "type": "object",
        "minProperties": 1,
        "properties": {
            TOOL_READ_FILE_PATH_ARG: {
                "type": "string",
                "description": "Absolute path to the file within the workspace",
                "minLength": 1,
            },
            TOOL_READ_FILE_URI_ARG: {
                "type": "string",
                "description": "File URI using file:// or editor-specific schemes",
                "minLength": 1,
            },
            TOOL_READ_FILE_LINE_ARG: {
                "type": "integer",
                "minimum": 1,
                "description": "1-based line number to start reading from",
            },
            TOOL_READ_FILE_LIMIT_ARG: {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum number of lines to read",
            }
        },
        "additionalProperties": false,
        "description": description,
        "examples": examples,
    });

    ToolDefinition::function(tools::READ_FILE.to_string(), description, schema)
}

pub(super) fn build_list_files_definition(workspace_root: &Path) -> ToolDefinition {
    let description = format!(
        "{TOOL_LIST_FILES_DESCRIPTION}. Workspace root: {}. Provide {path} (relative) or {uri} inside the workspace. Defaults to '.' when omitted.",
        workspace_root.display(),
        path = TOOL_LIST_FILES_PATH_ARG,
        uri = TOOL_LIST_FILES_URI_ARG,
    );
    let workspace_display = workspace_root.display().to_string();
    let examples = vec![
        json!({
            TOOL_LIST_FILES_MODE_ARG: "list",
        }),
        json!({
            TOOL_LIST_FILES_PATH_ARG: "src",
            TOOL_LIST_FILES_MODE_ARG: "recursive",
            TOOL_LIST_FILES_PER_PAGE_ARG: 100,
        }),
        json!({
            TOOL_LIST_FILES_URI_ARG: format!("file://{}/src", workspace_display),
        }),
    ];
    let schema = json!({
        "type": "object",
        "properties": {
            TOOL_LIST_FILES_PATH_ARG: {
                "type": "string",
                "description": "Directory or file path relative to the workspace root",
                "default": ".",
            },
            TOOL_LIST_FILES_MODE_ARG: {
                "type": "string",
                "enum": ["list", "recursive", "find_name", "find_content"],
                "description": "Listing mode: list (default), recursive, find_name, or find_content",
            },
            TOOL_LIST_FILES_PAGE_ARG: {
                "type": "integer",
                "minimum": 1,
                "description": "Page number to return (1-based)",
            },
            TOOL_LIST_FILES_PER_PAGE_ARG: {
                "type": "integer",
                "minimum": 1,
                "description": "Items per page (default 50)",
            },
            TOOL_LIST_FILES_MAX_ITEMS_ARG: {
                "type": "integer",
                "minimum": 1,
                "description": "Maximum number of items to scan before truncation",
            },
            TOOL_LIST_FILES_INCLUDE_HIDDEN_ARG: {
                "type": "boolean",
                "description": "Whether to include dotfiles and ignored entries",
            },
            TOOL_LIST_FILES_RESPONSE_FORMAT_ARG: {
                "type": "string",
                "enum": ["concise", "detailed"],
                "description": "Choose concise (default) or detailed metadata",
            },
            TOOL_LIST_FILES_NAME_PATTERN_ARG: {
                "type": "string",
                "description": "Optional filename pattern used by recursive or find_name modes",
            },
            TOOL_LIST_FILES_CONTENT_PATTERN_ARG: {
                "type": "string",
                "description": "Pattern to search within files when using find_content mode",
            },
            TOOL_LIST_FILES_FILE_EXTENSIONS_ARG: {
                "type": "array",
                "items": {"type": "string"},
                "description": "Restrict results to files matching any extension",
            },
            TOOL_LIST_FILES_CASE_SENSITIVE_ARG: {
                "type": "boolean",
                "description": "Enable case sensitive matching for patterns",
            },
        },
        "additionalProperties": false,
        "description": description,
        "examples": examples,
    });

    ToolDefinition::function("list_files".to_string(), description, schema)
}

pub(super) fn build_switch_mode_definition() -> ToolDefinition {
    let description = format!(
        "Switch the current session mode. {ask} and {architect} are read-only; {code} enables local implementation tools. Possible modes: {ask}, {architect}, {code}.",
        ask = SessionMode::Ask.as_str(),
        architect = SessionMode::Architect.as_str(),
        code = SessionMode::Code.as_str()
    );
    let schema = json!({
        "type": "object",
        "required": ["mode_id"],
        "properties": {
            "mode_id": {
                "type": "string",
                "enum": [
                    SessionMode::Ask.as_str(),
                    SessionMode::Architect.as_str(),
                    SessionMode::Code.as_str()
                ],
                "description": "The ID of the mode to switch to"
            }
        },
        "additionalProperties": false,
        "description": description,
    });

    ToolDefinition::function("switch_mode".to_string(), description, schema)
}
