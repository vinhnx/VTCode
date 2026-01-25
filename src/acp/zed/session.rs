use crate::acp::register_acp_connection;
use crate::acp::workspace::{DefaultWorkspaceTrustSynchronizer, WorkspaceTrustSynchronizer};
use crate::workspace_trust::WorkspaceTrustSyncOutcome;
use agent_client_protocol as acp;
use agent_client_protocol::Client;
use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{error, info, warn};
use vtcode_core::config::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::prompts::generate_system_instruction;

use super::ZedAgent;
use super::constants::{
    WORKSPACE_TRUST_ALREADY_SATISFIED_LOG, WORKSPACE_TRUST_DOWNGRADE_SKIPPED_LOG,
    WORKSPACE_TRUST_UPGRADE_LOG,
};
use super::types::NotificationEnvelope;

pub async fn run_zed_agent(config: &CoreAgentConfig, vt_cfg: &VTCodeConfig) -> Result<()> {
    let zed_config = &vt_cfg.acp.zed;
    let desired_trust_level = zed_config.workspace_trust.to_workspace_trust_level();
    let trust_synchronizer = DefaultWorkspaceTrustSynchronizer::new();
    match trust_synchronizer
        .synchronize(&config.workspace, desired_trust_level)
        .await
        .context("Failed to synchronize workspace trust for ACP bridge")?
    {
        WorkspaceTrustSyncOutcome::Upgraded { previous, new } => {
            info!(previous = ?previous, new = ?new, "{}", WORKSPACE_TRUST_UPGRADE_LOG);
        }
        WorkspaceTrustSyncOutcome::AlreadyMatches(level) => {
            info!(level = ?level, "{}", WORKSPACE_TRUST_ALREADY_SATISFIED_LOG);
        }
        WorkspaceTrustSyncOutcome::SkippedDowngrade(current) => {
            info!(
                current = ?current,
                requested = ?zed_config.workspace_trust,
                "{}",
                WORKSPACE_TRUST_DOWNGRADE_SKIPPED_LOG
            );
        }
    }

    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();
    let content = generate_system_instruction(&Default::default()).await;
    let system_prompt = if let Some(text) = content.parts.first().and_then(|p| p.as_text()) {
        text.to_string()
    } else {
        String::new()
    };
    let tools_config = vt_cfg.tools.clone();
    let commands_config = vt_cfg.commands.clone();

    let local_set = tokio::task::LocalSet::new();
    let config_clone = config.clone();
    let zed_config_clone = zed_config.clone();

    local_set
        .run_until(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
            let tools_config_clone = tools_config.clone();
            let commands_config_clone = commands_config.clone();
            let agent = ZedAgent::new(
                config_clone,
                zed_config_clone,
                tools_config_clone,
                commands_config_clone,
                system_prompt,
                tx,
            )
            .await;
            let (raw_conn, io_task) =
                acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                    tokio::task::spawn_local(fut);
                });
            let conn = Arc::new(raw_conn);
            if let Err(existing) = register_acp_connection(Arc::clone(&conn)) {
                warn!("ACP client already registered; continuing with existing instance");
                drop(existing);
            }

            let notifications_conn = Arc::clone(&conn);
            let notifications = tokio::task::spawn_local(async move {
                while let Some(envelope) = rx.recv().await {
                    let result = notifications_conn
                        .session_notification(envelope.notification)
                        .await;
                    if let Err(error) = result {
                        error!(%error, "Failed to forward ACP session notification");
                    }
                    let _ = envelope.completion.send(());
                }
            });

            let io_result = io_task.await;
            notifications.abort();
            io_result
        })
        .await
        .context("ACP stdio bridge task failed")?;

    Ok(())
}
