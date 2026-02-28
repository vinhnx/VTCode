use super::AgentRunner;
use crate::config::VTCodeConfig;
use crate::core::loop_detector::LoopDetector;
use crate::mcp::McpClient;
use crate::prompts::system::compose_system_instruction_text;
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::warn;

impl AgentRunner {
    /// Apply workspace configuration to the tool registry, including tool policies and MCP setup.
    pub async fn apply_workspace_configuration(&mut self, vt_cfg: &VTCodeConfig) -> Result<()> {
        self.config = Arc::new(vt_cfg.clone());
        *self.loop_detector.lock() =
            LoopDetector::with_max_repeated_calls(self.config.tools.max_repeated_tool_calls.max(1));

        self.system_prompt = compose_system_instruction_text(
            self._workspace.as_path(),
            Some(self.config()),
            None, // No prompt_context
        )
        .await;

        self.tool_registry.apply_timeout_policy(&vt_cfg.timeouts);
        self.tool_registry.initialize_async().await?;

        self.tool_registry.apply_commands_config(&vt_cfg.commands);
        self.tool_registry.apply_sandbox_config(&vt_cfg.sandbox);

        if let Err(err) = self
            .tool_registry
            .apply_config_policies(&vt_cfg.tools)
            .await
        {
            warn!("Failed to apply tool policies from config: {}", err);
        }

        self.max_turns = vt_cfg.automation.full_auto.max_turns.max(1);

        if vt_cfg.mcp.enabled {
            let mut mcp_client = McpClient::new(vt_cfg.mcp.clone());

            // Validate configuration before initializing
            if let Err(e) = crate::mcp::validate_mcp_config(&vt_cfg.mcp) {
                warn!("MCP configuration validation error: {e}");
            }
            match timeout(Duration::from_secs(30), mcp_client.initialize()).await {
                Ok(Ok(())) => {
                    let mcp_client = Arc::new(mcp_client);
                    self.tool_registry
                        .set_mcp_client(Arc::clone(&mcp_client))
                        .await;
                    if let Err(err) = self.tool_registry.refresh_mcp_tools().await {
                        warn!("Failed to refresh MCP tools: {}", err);
                    }

                    // Sync MCP tools to files for dynamic context discovery
                    if vt_cfg.context.dynamic.enabled
                        && vt_cfg.context.dynamic.sync_mcp_tools
                        && let Err(err) = mcp_client.sync_tools_to_files(&self._workspace).await
                    {
                        warn!("Failed to sync MCP tools to files: {}", err);
                    }
                }
                Ok(Err(err)) => {
                    warn!("MCP client initialization failed: {}", err);
                }
                Err(_) => {
                    warn!("MCP client initialization timed out after 30 seconds");
                }
            }
        }

        // Initialize dynamic context discovery directories
        if vt_cfg.context.dynamic.enabled
            && let Err(err) = crate::context::initialize_dynamic_context(
                &self._workspace,
                &vt_cfg.context.dynamic,
            )
            .await
        {
            warn!("Failed to initialize dynamic context directories: {}", err);
        }

        Ok(())
    }
}
