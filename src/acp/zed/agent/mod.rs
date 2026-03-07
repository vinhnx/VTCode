use crate::acp::permissions::{AcpPermissionPrompter, DefaultPermissionPrompter};
use crate::acp::tooling::AcpToolRegistry;
use agent_client_protocol as acp;
use hashbrown::HashMap;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tracing::warn;
use vtcode_core::config::ToolDocumentationMode;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, CapabilityLevel};
use vtcode_core::config::{AgentClientProtocolZedConfig, CommandsConfig, ToolsConfig};
use vtcode_core::core::threads::ThreadManager;
use vtcode_core::tools::file_ops::FileOpsTool;
use vtcode_core::tools::grep_file::GrepSearchManager;
use vtcode_core::tools::handlers::{SessionSurface, SessionToolsConfig, ToolModelCapabilities};
use vtcode_core::tools::registry::ToolRegistry as CoreToolRegistry;

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
    thread_manager: ThreadManager,
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
        if let Err(error) = core_tool_registry
            .apply_tool_runtime_config(&commands_config, &tools_config)
            .await
        {
            warn!(%error, "Failed to apply tools configuration to ACP tool registry");
        }
        let local_definitions = core_tool_registry
            .model_tools(SessionToolsConfig::full_public(
                SessionSurface::Acp,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::default(),
                ToolModelCapabilities::default(),
            ))
            .await;
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
            thread_manager: ThreadManager::new(),
            client_capabilities: Rc::new(RefCell::new(None)),
            title,
        }
    }
}
