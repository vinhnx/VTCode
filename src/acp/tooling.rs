use std::collections::HashMap;
use std::path::Path;

use serde_json::{Value, json};

use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolDefinition;

pub const TOOL_READ_FILE_DESCRIPTION: &str =
    "Read the contents of a text file accessible to the Zed workspace";
pub const TOOL_READ_FILE_URI_ARG: &str = "uri";
pub const TOOL_READ_FILE_PATH_ARG: &str = "path";
pub const TOOL_READ_FILE_LINE_ARG: &str = "line";
pub const TOOL_READ_FILE_LIMIT_ARG: &str = "limit";

pub const TOOL_LIST_FILES_DESCRIPTION: &str =
    "Explore workspace files, recursive matches, or pattern-based searches";
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
pub const TOOL_LIST_FILES_AST_GREP_PATTERN_ARG: &str = "ast_grep_pattern";
pub const TOOL_LIST_FILES_ITEMS_KEY: &str = "items";
pub const TOOL_LIST_FILES_MESSAGE_KEY: &str = "message";
pub const TOOL_LIST_FILES_RESULT_KEY: &str = "result";
pub const TOOL_LIST_FILES_SUMMARY_MAX_ITEMS: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedTool {
    ReadFile,
    ListFiles,
}

impl SupportedTool {
    pub fn kind(&self) -> agent_client_protocol::ToolKind {
        match self {
            Self::ReadFile => agent_client_protocol::ToolKind::Fetch,
            Self::ListFiles => agent_client_protocol::ToolKind::Search,
        }
    }

    pub fn default_title(&self) -> &'static str {
        match self {
            Self::ReadFile => "Read file",
            Self::ListFiles => "List files",
        }
    }

    pub fn function_name(&self) -> &'static str {
        match self {
            Self::ReadFile => tools::READ_FILE,
            Self::ListFiles => tools::LIST_FILES,
        }
    }

    pub fn sort_key(&self) -> u8 {
        match self {
            Self::ReadFile => 0,
            Self::ListFiles => 1,
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
        let mut entries = Vec::new();
        let mut mapping = HashMap::new();
        let mut local_map = HashMap::new();

        if read_file_enabled {
            let workspace_display = workspace_root.display().to_string();
            let sample_path = workspace_root.join("README.md");
            let sample_path_string = sample_path.to_string_lossy().into_owned();
            let sample_uri = format!("zed-fs://{}", sample_path_string);
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
                    TOOL_READ_FILE_PATH_ARG: sample_path_string.clone(),
                }),
                json!({
                    TOOL_READ_FILE_PATH_ARG: sample_path_string.clone(),
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
                        "description": "File URI using file://, zed://, or zed-fs:// schemes",
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
                read_file_description.clone(),
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
                    TOOL_LIST_FILES_URI_ARG: format!("zed-fs://{}/src", workspace_root.display()),
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
                    TOOL_LIST_FILES_AST_GREP_PATTERN_ARG: {
                        "type": "string",
                        "description": "tree-sitter based search pattern when mode is find_content",
                    },
                },
                "additionalProperties": false,
                "description": list_files_description,
                "examples": list_files_examples,
            });

            let list_files = ToolDefinition::function(
                tools::LIST_FILES.to_string(),
                list_files_description.clone(),
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
            },
            ToolDescriptor::Local => Self::format_local_title(function_name),
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

pub trait ToolRegistryProvider {
    fn registered_tools(&self) -> Vec<SupportedTool>;

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition>;

    fn render_title(&self, descriptor: ToolDescriptor, function_name: &str, args: &Value)
    -> String;

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor>;

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition>;

    fn has_local_tools(&self) -> bool;
}

impl ToolRegistryProvider for AcpToolRegistry {
    fn registered_tools(&self) -> Vec<SupportedTool> {
        AcpToolRegistry::registered_tools(self)
    }

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        AcpToolRegistry::definitions_for(self, enabled_tools, include_local)
    }

    fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        AcpToolRegistry::render_title(self, descriptor, function_name, args)
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        AcpToolRegistry::lookup(self, function_name)
    }

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        AcpToolRegistry::local_definition(self, tool_name)
    }

    fn has_local_tools(&self) -> bool {
        AcpToolRegistry::has_local_tools(self)
    }
}

impl<T> ToolRegistryProvider for std::rc::Rc<T>
where
    T: ToolRegistryProvider,
{
    fn registered_tools(&self) -> Vec<SupportedTool> {
        <T as ToolRegistryProvider>::registered_tools(&**self)
    }

    fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        <T as ToolRegistryProvider>::definitions_for(&**self, enabled_tools, include_local)
    }

    fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        <T as ToolRegistryProvider>::render_title(&**self, descriptor, function_name, args)
    }

    fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        <T as ToolRegistryProvider>::lookup(&**self, function_name)
    }

    fn local_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        <T as ToolRegistryProvider>::local_definition(&**self, tool_name)
    }

    fn has_local_tools(&self) -> bool {
        <T as ToolRegistryProvider>::has_local_tools(&**self)
    }
}
