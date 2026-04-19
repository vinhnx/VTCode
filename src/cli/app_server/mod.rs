use anyhow::Result;
use std::future::Future;
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

pub fn handle_app_server_command<'a>(
    _agent_config: &'a CoreAgentConfig,
    vt_config: &'a VTCodeConfig,
    listen: &'a str,
) -> impl Future<Output = Result<()>> + 'a {
    crate::codex_app_server::launch_app_server_proxy(Some(vt_config), listen)
}
