use anyhow::Result;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

use crate::workspace_trust::{WorkspaceTrustGateResult, ensure_workspace_trust};

pub async fn handle_chat_command(
    config: &CoreAgentConfig,
    skip_confirmations: bool,
    full_auto: bool,
    plan_mode: bool,
    team_context: Option<vtcode_core::agent_teams::TeamContext>,
) -> Result<()> {
    match ensure_workspace_trust(&config.workspace, full_auto).await? {
        WorkspaceTrustGateResult::Trusted(level) => {
            if full_auto && level != WorkspaceTrustLevel::FullAuto {
                return Ok(());
            }
        }
        WorkspaceTrustGateResult::Aborted => {
            return Ok(());
        }
    }
    crate::agent::agents::run_single_agent_loop(
        config,
        skip_confirmations,
        full_auto,
        plan_mode,
        team_context,
        None,
    )
    .await
}
