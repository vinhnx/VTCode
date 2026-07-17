use hashbrown::HashMap;
use serde_json::Value;
use std::path::Path;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolDefinition;

use super::schemas::{build_list_files_definition, build_read_file_definition};
use super::titles::render_title;

/// Enum of tools available via the Agent Client Protocol (ACP) integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SupportedTool {
    ReadFile,
    ListFiles,
}

impl SupportedTool {
    pub fn kind(&self) -> crate::acp::ToolKind {
        match self {
            Self::ReadFile => crate::acp::ToolKind::Read,
            Self::ListFiles => crate::acp::ToolKind::Search,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolDescriptor {
    Acp(SupportedTool),
    Local,
}

impl ToolDescriptor {
    pub fn kind(self) -> crate::acp::ToolKind {
        match self {
            Self::Acp(tool) => tool.kind(),
            Self::Local => crate::acp::ToolKind::Other,
        }
    }
}

struct ToolRegistryEntry {
    tool: SupportedTool,
    definition: ToolDefinition,
}

pub struct AcpToolRegistry {
    entries: Vec<ToolRegistryEntry>,
    local_definitions: Vec<ToolDefinition>,
    mapping: HashMap<String, ToolDescriptor>,
}

impl AcpToolRegistry {
    pub fn new(
        workspace_root: &Path,
        read_file_enabled: bool,
        list_files_enabled: bool,
        local_definitions: Vec<ToolDefinition>,
    ) -> Self {
        let mut entries = Vec::with_capacity(5);
        let mut mapping = HashMap::with_capacity(10);

        if read_file_enabled {
            push_registry_entry(
                &mut entries,
                &mut mapping,
                SupportedTool::ReadFile,
                build_read_file_definition(workspace_root),
            );
        }
        if list_files_enabled {
            push_registry_entry(
                &mut entries,
                &mut mapping,
                SupportedTool::ListFiles,
                build_list_files_definition(workspace_root),
            );
        }

        entries.sort_unstable_by_key(|entry| entry.tool.sort_key());

        Self { entries, local_definitions, mapping }
    }

    pub fn registered_tools(&self) -> Vec<SupportedTool> {
        self.entries.iter().map(|entry| entry.tool).collect()
    }

    pub fn definitions_for(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
    ) -> Vec<ToolDefinition> {
        self.definitions_for_filtered(enabled_tools, include_local, |_| true)
    }

    /// Like [`Self::definitions_for`], but filters the local tool definitions
    /// through `local_tool_allowed`. This lets callers apply per-agent
    /// permission gating to local tools while keeping the ACP-advertised tools
    /// (already gated via `enabled_tools`) unchanged.
    pub fn definitions_for_filtered(
        &self,
        enabled_tools: &[SupportedTool],
        include_local: bool,
        local_tool_allowed: impl Fn(&str) -> bool,
    ) -> Vec<ToolDefinition> {
        let mut definitions = Vec::with_capacity(self.entries.len());
        for entry in &self.entries {
            if enabled_tools.contains(&entry.tool) {
                definitions.push(entry.definition.clone());
            }
        }

        if include_local {
            definitions.extend(
                self.local_definitions
                    .iter()
                    .filter(|definition| local_tool_allowed(definition.function_name()))
                    .cloned(),
            );
        }

        definitions
    }

    pub fn render_title(
        &self,
        descriptor: ToolDescriptor,
        function_name: &str,
        args: &Value,
    ) -> String {
        render_title(descriptor, function_name, args)
    }

    pub fn tool_kind(&self, function_name: &str) -> crate::acp::ToolKind {
        self.tool_kind_for_call(function_name, None)
    }

    pub fn tool_kind_for_call(
        &self,
        function_name: &str,
        args: Option<&Value>,
    ) -> crate::acp::ToolKind {
        let _ = args;
        match function_name {
            tools::READ_FILE => crate::acp::ToolKind::Read,
            tools::GREP_FILE | tools::LIST_FILES | tools::CODE_SEARCH => {
                crate::acp::ToolKind::Search
            }
            tools::RUN_PTY_CMD
            | tools::EXEC_PTY_CMD
            | tools::EXEC_COMMAND
            | tools::WRITE_STDIN
            | tools::EXECUTE_CODE
            | tools::SHELL => crate::acp::ToolKind::Execute,
            tools::WRITE_FILE
            | tools::CREATE_FILE
            | tools::EDIT_FILE
            | tools::APPLY_PATCH
            | tools::SEARCH_REPLACE
            | tools::FILE_OP
            | tools::COPY_FILE => crate::acp::ToolKind::Edit,
            tools::DELETE_FILE => crate::acp::ToolKind::Delete,
            tools::MOVE_FILE => crate::acp::ToolKind::Move,
            tools::WEB_FETCH | tools::FETCH_URL | tools::FETCH => crate::acp::ToolKind::Fetch,
            tools::THINK => crate::acp::ToolKind::Think,
            _ => crate::acp::ToolKind::Other,
        }
    }

    pub fn lookup(&self, function_name: &str) -> Option<ToolDescriptor> {
        self.mapping.get(function_name).copied().or_else(|| {
            self.local_definitions
                .iter()
                .any(|definition| definition.function_name() == function_name)
                .then_some(ToolDescriptor::Local)
        })
    }

    pub fn has_local_tools(&self) -> bool {
        !self.local_definitions.is_empty()
    }
}

fn push_registry_entry(
    entries: &mut Vec<ToolRegistryEntry>,
    mapping: &mut HashMap<String, ToolDescriptor>,
    tool: SupportedTool,
    definition: ToolDefinition,
) {
    mapping.insert(definition.function_name().to_string(), ToolDescriptor::Acp(tool));
    entries.push(ToolRegistryEntry { tool, definition });
}
