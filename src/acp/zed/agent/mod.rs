use crate::acp::permissions::{AcpPermissionPrompter, DefaultPermissionPrompter};
use crate::acp::tooling::AcpToolRegistry;
use agent_client_protocol as acp;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::warn;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::constants::tools;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, CapabilityLevel};
use vtcode_core::config::{AgentClientProtocolZedConfig, CommandsConfig, ToolsConfig};
use vtcode_core::llm::provider::ToolDefinition;
use vtcode_core::tools::file_ops::FileOpsTool;
use vtcode_core::tools::grep_file::GrepSearchManager;
use vtcode_core::tools::registry::{
    ToolRegistry as CoreToolRegistry, build_function_declarations_cached,
    build_function_declarations_for_level,
};

use super::constants::TOOLS_EXCLUDED_FROM_ACP;
use super::types::{NotificationEnvelope, SessionHandle};

mod handlers;
mod prompt;
mod session_state;
mod tool_config;
#[cfg(test)]
mod tool_config_tests;
mod tool_execution;
mod tool_execution_local;
mod updates;

pub(crate) struct ZedAgent {
    config: CoreAgentConfig,
    system_prompt: String,
    sessions: Rc<RefCell<HashMap<acp::SessionId, SessionHandle>>>,
    next_session_id: Cell<u64>,
    session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
    acp_tool_registry: Rc<AcpToolRegistry>,
    permission_prompter: Rc<dyn AcpPermissionPrompter>,
    local_tool_registry: Mutex<CoreToolRegistry>,
    file_ops_tool: Option<FileOpsTool>,
    client_capabilities: Rc<RefCell<Option<acp::ClientCapabilities>>>,
    title: Option<String>,
}

impl ZedAgent {
    pub(crate) async fn new(
        config: CoreAgentConfig,
        zed_config: AgentClientProtocolZedConfig,
        tools_config: ToolsConfig,
        commands_config: CommandsConfig,
        system_prompt: String,
        session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
        title: Option<String>,
    ) -> Self {
        let read_file_enabled = zed_config.tools.read_file;
        let workspace_root = config.workspace.clone();
        let file_ops_tool = if zed_config.tools.list_files {
            let search_root = workspace_root.clone();
            Some(FileOpsTool::new(
                workspace_root.clone(),
                Arc::new(GrepSearchManager::new(search_root)),
            ))
        } else {
            None
        };
        let list_files_enabled = file_ops_tool.is_some();

        let core_tool_registry = CoreToolRegistry::new(config.workspace.clone()).await;
        core_tool_registry.apply_commands_config(&commands_config);
        if let Err(error) = core_tool_registry
            .apply_config_policies(&tools_config)
            .await
        {
            warn!(%error, "Failed to apply tools configuration to ACP tool registry");
        }
        let available_local_tools: HashSet<String> = core_tool_registry
            .available_tools()
            .await
            .into_iter()
            .collect();
        let decls = build_function_declarations_for_level(CapabilityLevel::CodeSearch);
        let mut local_definitions = Vec::with_capacity(decls.len());

        for decl in decls {
            if decl.name != tools::READ_FILE
                && decl.name != tools::LIST_FILES
                && !TOOLS_EXCLUDED_FROM_ACP.contains(&decl.name.as_str())
                && available_local_tools.contains(decl.name.as_str())
            {
                local_definitions.push(ToolDefinition::function(
                    decl.name.clone(),
                    decl.description.clone(),
                    decl.parameters.clone(),
                ));
            }
        }

        if available_local_tools.contains(tools::RUN_PTY_CMD)
            && let Some(run_decl) =
                build_function_declarations_cached(ToolDocumentationMode::default())
                    .iter()
                    .find(|decl| decl.name == tools::RUN_PTY_CMD)
        {
            let already_registered = local_definitions
                .iter()
                .any(|definition| definition.function_name() == tools::RUN_PTY_CMD);
            if !already_registered {
                local_definitions.push(ToolDefinition::function(
                    run_decl.name.clone(),
                    run_decl.description.clone(),
                    run_decl.parameters.clone(),
                ));
            }
        }
        let acp_tool_registry = Rc::new(AcpToolRegistry::new(
            workspace_root.as_path(),
            read_file_enabled,
            list_files_enabled,
            local_definitions,
        ));
        let permission_prompter: Rc<dyn AcpPermissionPrompter> = Rc::new(
            DefaultPermissionPrompter::new(Rc::clone(&acp_tool_registry)),
        );

        Self {
            config,
            system_prompt,
            sessions: Rc::new(RefCell::new(HashMap::with_capacity(10))), // Pre-allocate for typical session count
            next_session_id: Cell::new(0),
            session_update_tx,
            acp_tool_registry,
            permission_prompter,
            local_tool_registry: Mutex::new(core_tool_registry),
            file_ops_tool,
            client_capabilities: Rc::new(RefCell::new(None)),
            title,
        }
    }
}
