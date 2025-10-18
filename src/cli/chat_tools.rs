use anyhow::Result;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;

use crate::workspace_trust::{WorkspaceTrustGateResult, ensure_workspace_trust};

pub async fn handle_chat_command(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    full_auto: bool,
) -> Result<()> {
    match ensure_workspace_trust(&config.workspace, full_auto)? {
        WorkspaceTrustGateResult::Trusted(level) => {
            if full_auto && level != WorkspaceTrustLevel::FullAuto {
                return Ok(());
            }
        }
        WorkspaceTrustGateResult::Aborted => {
            return Ok(());
        }
    }
    crate::agent::runloop::run_single_agent_loop(config, skip_confirmations, full_auto, None).await
}
