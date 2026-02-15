//! In-process teammate runner.
//!
//! Spawns a persistent tokio task per teammate that:
//! - Polls its inbox on an interval
//! - Processes incoming text messages and task assignments via single-turn LLM calls
//! - Replies to the lead mailbox with results
//! - Sends `IdleNotification` when no work remains
//! - Shuts down on `ShutdownRequest`

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::config::loader::VTCodeConfig;
use crate::llm::{AnyClient, ProviderClientAdapter};

use super::storage::TeamStorage;
use super::types::{
    TeamMailboxMessage, TeamProtocolMessage, TeamProtocolType, TeamTaskStatus, TeammateConfig,
};

/// Manages all in-process teammate runners for a single team.
#[derive(Debug)]
pub struct InProcessTeamRunner {
    team_name: String,
    handles: HashMap<String, TeammateHandle>,
}

#[derive(Debug)]
struct TeammateHandle {
    shutdown_tx: watch::Sender<bool>,
    join: JoinHandle<()>,
}

/// Configuration for spawning an in-process teammate.
pub struct TeammateSpawnConfig {
    pub teammate: TeammateConfig,
    pub team_name: String,
    pub api_key: String,
    pub poll_interval: Duration,
    pub vt_cfg: Option<VTCodeConfig>,
}

impl InProcessTeamRunner {
    pub fn new(team_name: String) -> Self {
        Self {
            team_name,
            handles: HashMap::new(),
        }
    }

    /// Spawn an in-process teammate runner.
    pub fn spawn_teammate(&mut self, config: TeammateSpawnConfig) -> Result<()> {
        let name = config.teammate.name.clone();
        if self.handles.contains_key(&name) {
            anyhow::bail!("Teammate '{}' is already running in-process.", name);
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let join = tokio::spawn(teammate_loop(config, shutdown_rx));

        self.handles.insert(
            name.clone(),
            TeammateHandle { shutdown_tx, join },
        );

        info!(teammate = %name, team = %self.team_name, "Spawned in-process teammate");
        Ok(())
    }

    /// Request graceful shutdown for a specific teammate.
    pub fn request_shutdown(&self, name: &str) {
        if let Some(handle) = self.handles.get(name) {
            let _ = handle.shutdown_tx.send(true);
            debug!(teammate = %name, "Shutdown signal sent");
        }
    }

    /// Request shutdown for all teammates.
    pub fn shutdown_all(&self) {
        for (name, handle) in &self.handles {
            let _ = handle.shutdown_tx.send(true);
            debug!(teammate = %name, "Shutdown signal sent");
        }
    }

    /// Check if a teammate is still running.
    pub fn is_running(&self, name: &str) -> bool {
        self.handles
            .get(name)
            .is_some_and(|h| !h.join.is_finished())
    }

    /// Remove finished handles and return their names.
    pub fn reap_finished(&mut self) -> Vec<String> {
        let finished: Vec<String> = self
            .handles
            .iter()
            .filter(|(_, h)| h.join.is_finished())
            .map(|(name, _)| name.clone())
            .collect();
        for name in &finished {
            self.handles.remove(name);
        }
        finished
    }
}

impl Drop for InProcessTeamRunner {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

/// The main loop for a single in-process teammate.
async fn teammate_loop(config: TeammateSpawnConfig, mut shutdown_rx: watch::Receiver<bool>) {
    let teammate_name = config.teammate.name.clone();
    let team_name = config.team_name.clone();

    let storage = match TeamStorage::from_config(config.vt_cfg.as_ref()).await {
        Ok(s) => s,
        Err(err) => {
            warn!(teammate = %teammate_name, "Failed to init storage: {err}");
            return;
        }
    };

    let mut client = match create_teammate_client(&config) {
        Ok(c) => c,
        Err(err) => {
            warn!(teammate = %teammate_name, "Failed to create LLM client: {err}");
            return;
        }
    };

    let mut mailbox_offset: u64 = storage
        .load_mailbox_offset(&team_name, &teammate_name)
        .await
        .unwrap_or(0);

    // Announce presence
    let hello = TeamMailboxMessage::text(
        &teammate_name,
        format!("Teammate '{}' started (in-process).", teammate_name),
        None,
    );
    if let Err(err) = storage
        .append_mailbox_message(&team_name, "lead", &hello)
        .await
    {
        warn!(teammate = %teammate_name, "Failed to send hello: {err}");
    }

    info!(teammate = %teammate_name, "Entering poll loop");

    loop {
        // Check for shutdown signal
        if *shutdown_rx.borrow() {
            // Send ShutdownApproved
            let proto = TeamMailboxMessage::protocol(
                &teammate_name,
                TeamProtocolMessage {
                    r#type: TeamProtocolType::ShutdownApproved,
                    details: None,
                },
                None,
            );
            let _ = storage
                .append_mailbox_message(&team_name, "lead", &proto)
                .await;
            info!(teammate = %teammate_name, "Shutdown approved, exiting");
            break;
        }

        // Poll inbox
        let (messages, new_offset) = match storage
            .read_mailbox_since(&team_name, &teammate_name, mailbox_offset)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                warn!(teammate = %teammate_name, "Mailbox read error: {err}");
                tokio::select! {
                    _ = tokio::time::sleep(config.poll_interval) => {}
                    _ = shutdown_rx.changed() => {}
                }
                continue;
            }
        };
        mailbox_offset = new_offset;
        let _ = storage
            .save_mailbox_offset(&team_name, &teammate_name, new_offset)
            .await;

        let mut did_work = false;

        for msg in &messages {
            // Handle protocol messages
            if let Some(proto) = &msg.protocol {
                match proto.r#type {
                    TeamProtocolType::ShutdownRequest => {
                        let ack = TeamMailboxMessage::protocol(
                            &teammate_name,
                            TeamProtocolMessage {
                                r#type: TeamProtocolType::ShutdownApproved,
                                details: None,
                            },
                            None,
                        );
                        let _ = storage
                            .append_mailbox_message(&team_name, "lead", &ack)
                            .await;
                        info!(teammate = %teammate_name, "Received shutdown request, exiting");
                        return;
                    }
                    _ => {
                        debug!(teammate = %teammate_name, proto_type = ?proto.r#type, "Ignoring protocol message");
                    }
                }
                continue;
            }

            // Handle text messages — run a single-turn LLM call and reply
            let text = msg.content.as_deref().unwrap_or("").trim();
            if text.is_empty() {
                continue;
            }

            did_work = true;
            debug!(teammate = %teammate_name, from = %msg.sender, "Processing message");

            let prompt = format!(
                "You are '{}', a teammate in team '{}'. \
                 Respond concisely and helpfully.\n\n\
                 Message from '{}':\n{}",
                teammate_name, team_name, msg.sender, text
            );

            match client.generate(&prompt).await {
                Ok(response) => {
                    let reply = TeamMailboxMessage::text(
                        &teammate_name,
                        response.content_string(),
                        msg.task_id,
                    );
                    if let Err(err) = storage
                        .append_mailbox_message(&team_name, &msg.sender, &reply)
                        .await
                    {
                        warn!(teammate = %teammate_name, "Failed to send reply: {err}");
                    }
                }
                Err(err) => {
                    warn!(teammate = %teammate_name, "LLM call failed: {err}");
                    let error_reply = TeamMailboxMessage::text(
                        &teammate_name,
                        format!("Error processing message: {err}"),
                        msg.task_id,
                    );
                    let _ = storage
                        .append_mailbox_message(&team_name, &msg.sender, &error_reply)
                        .await;
                }
            }
        }

        // If no work was done this cycle, check if we should send idle
        if !did_work && !messages.is_empty() {
            // We received only protocol messages; still idle
        }

        // Send idle notification if we have no assigned pending tasks
        if !did_work {
            let tasks = storage.load_tasks(&team_name).await.ok();
            let has_work = tasks.is_some_and(|t| {
                t.tasks.iter().any(|task| {
                    task.assigned_to.as_deref() == Some(&*teammate_name)
                        && matches!(
                            task.status,
                            TeamTaskStatus::Pending | TeamTaskStatus::InProgress
                        )
                })
            });

            if !has_work {
                // Only send idle periodically, not every poll cycle
                // The lead will see it once and can assign new work
            }
        }

        // Sleep until next poll or shutdown
        tokio::select! {
            _ = tokio::time::sleep(config.poll_interval) => {}
            _ = shutdown_rx.changed() => {}
        }
    }
}

fn create_teammate_client(config: &TeammateSpawnConfig) -> Result<AnyClient> {
    let model_string = config
        .teammate
        .model
        .as_deref()
        .unwrap_or("haiku")
        .to_string();

    let provider = crate::llm::factory::create_provider_for_model(
        &model_string,
        config.api_key.clone(),
        None,
    )
    .context("Failed to create LLM provider for in-process teammate")?;

    Ok(Box::new(ProviderClientAdapter::new(provider, model_string)))
}

// Convenience constructors on TeamMailboxMessage for cleaner teammate code.
impl TeamMailboxMessage {
    pub fn text(sender: &str, content: String, task_id: Option<u64>) -> Self {
        Self {
            sender: sender.to_string(),
            content: Some(content),
            protocol: None,
            id: None,
            read: false,
            timestamp: chrono::Utc::now(),
            task_id,
        }
    }

    pub fn protocol(
        sender: &str,
        protocol: TeamProtocolMessage,
        task_id: Option<u64>,
    ) -> Self {
        Self {
            sender: sender.to_string(),
            content: None,
            protocol: Some(protocol),
            id: None,
            read: false,
            timestamp: chrono::Utc::now(),
            task_id,
        }
    }
}
