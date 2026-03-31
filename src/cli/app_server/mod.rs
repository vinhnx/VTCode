use anyhow::Result;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

pub async fn handle_app_server_command(
    _agent_config: &CoreAgentConfig,
    vt_config: &VTCodeConfig,
    listen: &str,
) -> Result<()> {
    crate::codex_app_server::launch_app_server_proxy(Some(vt_config), listen).await
}
