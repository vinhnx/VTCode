use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolDefinition;

use crate::acp::zed::constants::{MODE_ID_ARCHITECT, MODE_ID_ASK, MODE_ID_CODE};

pub use super::tooling_provider::ToolRegistryProvider;

pub const TOOL_READ_FILE_DESCRIPTION: &str =
    "Read the contents of a text file accessible to the IDE workspace";
pub const TOOL_READ_FILE_URI_ARG: &str = "uri";
pub const TOOL_READ_FILE_PATH_ARG: &str = "path";
pub const TOOL_READ_FILE_LINE_ARG: &str = "line";
pub const TOOL_READ_FILE_LIMIT_ARG: &str = "limit";

pub const TOOL_LIST_FILES_DESCRIPTION: &str = "Explore workspace files in a SUBDIRECTORY (root path is blocked). Requires path like 'src/' or 'vtcode-core/'. For root overview, use shell commands via run_pty_cmd.";
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

/// Enum of tools available via the Agent Client Protocol (ACP) integration
///
/// Only a subset of VT Code tools are exposed via ACP for security and integration reasons:
/// - **ReadFile**: Safe, non-invasive file reading within workspace bounds
/// - **ListFiles**: Safe file discovery and pattern matching within workspace
///
/// Tools NOT exposed via ACP (and why):
/// - Terminal/PTY operations (run_pty_cmd, etc.): Requires sandboxing not available in Zed context
/// - Code execution: Potential security risk in editor integration
/// - Patch application: Complex state management not suitable for ACP
/// - Write operations: Reserved for local-only agent to prevent unintended edits
/// - Skill management: VT Code-specific feature, not relevant to Zed integration
/// - Diagnostic tools (debug_agent, analyze_agent): Internal agent state, not for editor
/// - Web fetch: Network access restricted in editor context
/// - Search tools: Integrated into Zed's own search functionality
/// - Plan mode tools (enter_plan_mode, exit_plan_mode): ACP has native session mode support - see https://agentclientprotocol.com/protocol/session-modes.md
/// - HITL tools (request_user_input and legacy aliases): ACP has native permission request mechanism
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedTool {
    ReadFile,
    ListFiles,
    SwitchMode,
}

impl SupportedTool {
    pub fn kind(&self) -> agent_client_protocol::ToolKind {
        match self {
            Self::ReadFile => agent_client_protocol::ToolKind::Read,
            Self::ListFiles => agent_client_protocol::ToolKind::Search,
            Self::SwitchMode => agent_client_protocol::ToolKind::Other,
        }
    }

    pub fn default_title(&self) -> &'static str {
        match self {
            Self::ReadFile => "Read file",
            Self::ListFiles => "List files",
            Self::SwitchMode => "Switch session mode",
        }
    }

    pub fn function_name(&self) -> &'static str {
        match self {
            Self::ReadFile => tools::READ_FILE,
            Self::ListFiles => tools::LIST_FILES,
            Self::SwitchMode => "switch_mode",
        }
    }

    pub fn sort_key(&self) -> u8 {
        match self {
            Self::ReadFile => 0,
            Self::ListFiles => 1,
            Self::SwitchMode => 2,
        }
    }
}

#[derive(Clone, Copy)]
pub enum ToolDescriptor {
    Acp(SupportedTool),
    Local,
}

impl ToolDescriptor {
    pub fn kind(self) -> agent_client_protocol::ToolKind {
        match self {
            Self::Acp(tool) => tool.kind(),
            Self::Local => agent_client_protocol::ToolKind::Other,
        }
    }
}

struct ToolRegistryEntry {
    tool: SupportedTool,
    definition: ToolDefinition,
}

pub struct AcpToolRegistry {
    entries: Vec<ToolRegistryEntry>,
    local_definitions: HashMap<String, ToolDefinition>,
    mapping: HashMap<String, ToolDescriptor>,
}

impl AcpToolRegistry {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace_root: &Path,
        read_file_enabled: bool,
        list_files_enabled: bool,
        local_definitions: Vec<ToolDefinition>,
    ) -> Self {
        let mut entries = Vec::with_capacity(5); // Pre-allocate for typical tool count (ReadFile, ListFiles + locals)
        let mut mapping = HashMap::with_capacity(10); // Pre-allocate for mapping entries
        let mut local_map = HashMap::with_capacity(local_definitions.len()); // Pre-allocate for local definitions

        if read_file_enabled {
            let workspace_display = workspace_root.display().to_string();
            let sample_path = workspace_root.join("README.md");
            let sample_path_string = sample_path.to_string_lossy().into_owned();
            let sample_uri = format!("file://{}", sample_path_string);
            let read_file_description = format!(
                "{TOOL_READ_FILE_DESCRIPTION}. Workspace root: {workspace}. Provide {path} or {uri} inside the workspace. Paths must be absolute (see ACP file system spec). Optional {line} and {limit} control slicing.",
                workspace = workspace_display,
                path = TOOL_READ_FILE_PATH_ARG,
                uri = TOOL_READ_FILE_URI_ARG,
                line = TOOL_READ_FILE_LINE_ARG,
                limit = TOOL_READ_FILE_LIMIT_ARG,
            );
            let read_file_examples = vec![
                json!({
                    TOOL_READ_FILE_PATH_ARG: &sample_path_string, // Use reference to avoid clone
                }),
                json!({
                    TOOL_READ_FILE_PATH_ARG: &sample_path_string, // Use reference to avoid clone
                    TOOL_READ_FILE_LINE_ARG: 1,
                    TOOL_READ_FILE_LIMIT_ARG: 200,
                }),
                json!({
                    TOOL_READ_FILE_URI_ARG: sample_uri,
                }),
            ];
            let read_file_schema = json!({
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
                "description": read_file_description,
                "examples": read_file_examples,
            });

            let read_file = ToolDefinition::function(
                tools::READ_FILE.to_string(),
                read_file_description, // Avoid clone since we own the value now
                read_file_schema,
            );
            mapping.insert(
                read_file.function_name().to_string(),
                ToolDescriptor::Acp(SupportedTool::ReadFile),
            );
            entries.push(ToolRegistryEntry {
                tool: SupportedTool::ReadFile,
                definition: read_file,
            });
        }
        if list_files_enabled {
            let list_files_description = format!(
                "{TOOL_LIST_FILES_DESCRIPTION}. Workspace root: {}. Provide {path} (relative) or {uri} inside the workspace. Defaults to '.' when omitted.",
                workspace_root.display(),
                path = TOOL_LIST_FILES_PATH_ARG,
                uri = TOOL_LIST_FILES_URI_ARG,
            );
            let workspace_display_str = workspace_root.display().to_string();
            let list_files_examples = vec![
                json!({
                    TOOL_LIST_FILES_MODE_ARG: "list",
                }),
                json!({
                    TOOL_LIST_FILES_PATH_ARG: "src",
                    TOOL_LIST_FILES_MODE_ARG: "recursive",
                    TOOL_LIST_FILES_PER_PAGE_ARG: 100,
                }),
                json!({
                    TOOL_LIST_FILES_URI_ARG: format!("file://{}/src", workspace_display_str),
                }),
            ];
            let list_files_schema = json!({
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
                "description": list_files_description,
                "examples": list_files_examples,
            });

            let list_files = ToolDefinition::function(
                tools::LIST_FILES.to_string(),
                list_files_description, // Avoid clone since we own the value now
                list_files_schema,
            );
            mapping.insert(
                list_files.function_name().to_string(),
                ToolDescriptor::Acp(SupportedTool::ListFiles),
            );
            entries.push(ToolRegistryEntry {
                tool: SupportedTool::ListFiles,
                definition: list_files,
            });
        }

        let switch_mode_description = format!(
            "Switch the current session mode (e.g., from {architect} to {code}). Possible modes: {ask}, {architect}, {code}.",
            ask = MODE_ID_ASK,
            architect = MODE_ID_ARCHITECT,
            code = MODE_ID_CODE
        );
        let switch_mode_schema = json!({
            "type": "object",
            "required": ["mode_id"],
            "properties": {
                "mode_id": {
                    "type": "string",
                    "enum": [MODE_ID_ASK, MODE_ID_ARCHITECT, MODE_ID_CODE],
                    "description": "The ID of the mode to switch to"
                }
            },
            "additionalProperties": false,
            "description": switch_mode_description,
        });
        let switch_mode = ToolDefinition::function(
            "switch_mode".to_string(),
            switch_mode_description.to_string(),
            switch_mode_schema,
        );
        mapping.insert(
            switch_mode.function_name().to_string(),
            ToolDescriptor::Acp(SupportedTool::SwitchMode),
        );
        entries.push(ToolRegistryEntry {
            tool: SupportedTool::SwitchMode,
            definition: switch_mode,
        });

        for definition in local_definitions {
            mapping.insert(
                definition.function_name().to_string(),
                ToolDescriptor::Local,
            );
            local_map.insert(definition.function_name().to_string(), definition);
        }

        entries.sort_unstable_by_key(|entry| entry.tool.sort_key());

        Self {
            entries,
            local_definitions: local_map,
            mapping,
        }
    }

    pub fn registered_tools(&self) -> Vec<SupportedTool> {
        self.entries.iter().map(|entry| entry.tool).collect()
    }

    pub fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        let mut definitions = Vec::new();
        for entry in &self.entries {
            if enabled_tools.contains(&entry.tool) {
                definitions.push(entry.definition.clone());
            }
        }

        if include_local {
            let mut local: Vec<_> = self.local_definitions.values().cloned().collect();
            local.sort_unstable_by(|left, right| left.function_name().cmp(right.function_name()));
            definitions.extend(local);
        }

        definitions
    }

    pub fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        match descriptor {
            ToolDescriptor::Acp(tool) => match tool {
                SupportedTool::ReadFile => args
                    .get(TOOL_READ_FILE_PATH_ARG)
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .map(|path| format!("Read file {}", Self::truncate_middle(path, 80)))
                    .or_else(|| {
                        args.get(TOOL_READ_FILE_URI_ARG)
                            .and_then(Value::as_str)
                            .filter(|value| !value.is_empty())
                            .map(|uri| format!("Read file {}", Self::truncate_middle(uri, 80)))
                    })
                    .unwrap_or_else(|| tool.default_title().to_string()),
                SupportedTool::ListFiles => {
                    if let Some(path) = args
                        .get(TOOL_LIST_FILES_PATH_ARG)
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        if path == "." {
                            "List files in workspace root".to_string()
                        } else {
                            format!("List files in {}", Self::truncate_middle(path, 60))
                        }
                    } else if let Some(pattern) = args
                        .get(TOOL_LIST_FILES_NAME_PATTERN_ARG)
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        format!("Find files named {}", Self::truncate_middle(pattern, 40))
                    } else if let Some(pattern) = args
                        .get(TOOL_LIST_FILES_CONTENT_PATTERN_ARG)
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                    {
                        format!("Search files for {}", Self::truncate_middle(pattern, 40))
                    } else {
                        tool.default_title().to_string()
                    }
                }
                SupportedTool::SwitchMode => args
                    .get("mode_id")
                    .and_then(Value::as_str)
                    .map(|mode| format!("Switch to {mode} mode"))
                    .unwrap_or_else(|| tool.default_title().to_string()),
            },
            ToolDescriptor::Local => Self::format_local_title(function_name),
        }
    }

    pub fn tool_kind(&self, function_name: &str) -> agent_client_protocol::ToolKind {
        match function_name {
            n if n == tools::READ_FILE => agent_client_protocol::ToolKind::Read,
            n if n == tools::GREP_FILE || n == tools::LIST_FILES => {
                agent_client_protocol::ToolKind::Search
            }
            n if n == tools::RUN_PTY_CMD => agent_client_protocol::ToolKind::Execute,
            n if n == tools::WRITE_FILE || n == tools::CREATE_FILE || n == tools::EDIT_FILE => {
                agent_client_protocol::ToolKind::Edit
            }
            n if n == tools::DELETE_FILE => agent_client_protocol::ToolKind::Delete,
            n if n == tools::WEB_FETCH => agent_client_protocol::ToolKind::Fetch,
            n if n == tools::CODE_INTELLIGENCE => agent_client_protocol::ToolKind::Search,
            _ => agent_client_protocol::ToolKind::Other,
        }
    }

    pub fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        self.mapping.get(function_name).copied()
    }

    pub fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        self.local_definitions.get(tool_name).cloned()
    }

    pub fn has_local_tools(&self) -> bool {
        !self.local_definitions.is_empty()
    }

    fn truncate_middle(input: &str, max_len: usize) -> String {
        let total = input.chars().count();
        if total <= max_len {
            return input.to_string();
        }

        if max_len < 3 {
            return input.chars().take(max_len).collect();
        }

        let front_len = max_len / 2;
        let back_len = max_len.saturating_sub(front_len + 1);
        let front: String = input.chars().take(front_len).collect();
        let back: String = input
            .chars()
            .rev()
            .take(back_len)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{front}…{back}")
    }

    fn format_local_title(name: &str) -> String {
        let formatted = name.replace('_', " ");
        let mut chars = formatted.chars();
        match chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            None => formatted,
        }
    }
}
