use hashbrown::HashMap;
use serde_json::Value;
use std::path::Path;
use vtcode_core::config::constants::tools;
use vtcode_core::llm::provider::ToolDefinition;

use super::schemas::{
    build_list_files_definition, build_read_file_definition, build_switch_mode_definition,
};
use super::titles::render_title;

/// Enum of tools available via the Agent Client Protocol (ACP) integration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
            Self::ListFiles => "list_files",
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    local_definitions: Vec<ToolDefinition>,
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

        push_registry_entry(
            &mut entries,
            &mut mapping,
            SupportedTool::SwitchMode,
            build_switch_mode_definition(),
        );

        entries.sort_unstable_by_key(|entry| entry.tool.sort_key());

        Self {
            entries,
            local_definitions,
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
            definitions.extend(self.local_definitions.iter().cloned());
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

    pub fn tool_kind(&self, function_name: &str) -> agent_client_protocol::ToolKind {
        match function_name {
            tools::READ_FILE => agent_client_protocol::ToolKind::Read,
            "grep_file" | "list_files" => agent_client_protocol::ToolKind::Search,
            tools::RUN_PTY_CMD | tools::UNIFIED_EXEC => agent_client_protocol::ToolKind::Execute,
            tools::WRITE_FILE | tools::CREATE_FILE | tools::EDIT_FILE => {
                agent_client_protocol::ToolKind::Edit
            }
            tools::DELETE_FILE => agent_client_protocol::ToolKind::Delete,
            "web_fetch" => agent_client_protocol::ToolKind::Fetch,
            _ => agent_client_protocol::ToolKind::Other,
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
    mapping.insert(
        definition.function_name().to_string(),
        ToolDescriptor::Acp(tool),
    );
    entries.push(ToolRegistryEntry { tool, definition });
}
