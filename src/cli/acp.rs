use anyhow::{Result, bail};
use vtcode_core::cli::args::AgentClientProtocolTarget;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::{AgentClientProtocolTransport, VTCodeConfig};

pub async fn handle_acp_command(
    config: &CoreAgentConfig,
    vt_cfg: &VTCodeConfig,
    target: AgentClientProtocolTarget,
) -> Result<()> {
    if !vt_cfg.acp.enabled {
        bail!(
            "Agent Client Protocol integration is disabled. Enable it via [acp] in vtcode.toml or set VT_ACP_ENABLED=1."
        );
    }

    match target {
        AgentClientProtocolTarget::Zed => {
            if !vt_cfg.acp.zed.enabled {
                bail!(
                    "Zed integration is disabled. Enable it via [acp.zed] in vtcode.toml or set VT_ACP_ZED_ENABLED=1."
                );
            }

            if vt_cfg.acp.zed.transport != AgentClientProtocolTransport::Stdio {
                bail!("Only the stdio transport is currently supported for Zed ACP integration.");
            }

            crate::acp::run_zed_agent(config, &vt_cfg.acp.zed).await?
        }
    }

    Ok(())
}
