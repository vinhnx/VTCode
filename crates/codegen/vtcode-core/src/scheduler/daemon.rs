use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use super::advance_record_runtime;
use super::session::resolve_scheduled_workspace_path;
use super::spec::{ScheduledTaskAction, ScheduledTaskDefinition, ScheduledTaskRecord, TaskRunStatus};
use super::store::{DurableTaskStore, release_claim_async, try_acquire_claim_async};
use crate::notifications::{NotificationEvent, send_global_notification};
use vtcode_commons::VtCodePaths;

#[derive(Debug, Clone)]
pub struct SchedulerDaemon {
    store: DurableTaskStore,
    executable_path: PathBuf,
}

impl SchedulerDaemon {
    #[must_use]
    pub fn new(store: DurableTaskStore, executable_path: PathBuf) -> Self {
        Self { store, executable_path }
    }

    pub async fn serve_forever(&self) -> Result<()> {
        loop {
            self.run_due_tasks_once().await?;
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }

    pub async fn run_due_tasks_once(&self) -> Result<usize> {
        let now = Utc::now();
        let mut records = self.store.load_records_async().await?;
        let mut executed = 0usize;

        records.sort_by_key(|record| record.runtime.next_run_at);
        for mut record in records {
            let Some(next_run_at) = record.runtime.next_run_at else {
                continue;
            };
            if now < next_run_at {
                continue;
            }
            if !try_acquire_claim_async(self.store.paths().clone(), record.definition.id.clone()).await? {
                continue;
            }

            let result = self.execute_record(&record, now).await;
            let release_result = release_claim_async(self.store.paths().clone(), record.definition.id.clone()).await;
            let run_outcome = result?;
            release_result?;

            apply_run_outcome(&mut record, run_outcome)?;
            self.store.update_runtime_async(record).await?;
            executed = executed.saturating_add(1);
        }

        Ok(executed)
    }

    async fn execute_record(&self, record: &ScheduledTaskRecord, run_at: DateTime<Utc>) -> Result<RunOutcome> {
        match &record.definition.action {
            ScheduledTaskAction::Reminder { message } => {
                let notification = send_global_notification(NotificationEvent::IdlePrompt {
                    title: format!("Scheduled reminder: {}", record.definition.name),
                    message: message.clone(),
                })
                .await;
                let status = match notification {
                    Ok(()) => TaskRunStatus::ReminderSent,
                    Err(error) => TaskRunStatus::Failed {
                        message: format!("failed to send reminder notification: {error:#}"),
                    },
                };
                Ok(RunOutcome {
                    ran_at: run_at,
                    status,
                    artifact_dir: None,
                    events_file: None,
                    last_message_file: None,
                })
            }
            ScheduledTaskAction::Prompt { prompt } => {
                let artifact_dir = self.store.paths().artifact_dir(&record.definition.id, run_at);
                let events_file = artifact_dir.join("events.jsonl");
                let last_message_file = artifact_dir.join("last-message.txt");
                let workspace_label = scheduled_workspace_label(&record.definition);
                let workspace = resolve_scheduled_task_workspace(&record.definition);
                let execution = async {
                    VtCodePaths::ensure_user_dir(&artifact_dir)
                        .with_context(|| format!("Failed to create run artifact dir {}", artifact_dir.display()))?;

                    let mut command = Command::new(&self.executable_path);
                    command
                        .arg("exec")
                        .arg("--events")
                        .arg(&events_file)
                        .arg("--last-message-file")
                        .arg(&last_message_file)
                        .arg(prompt)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());

                    if let Some(workspace) = workspace? {
                        command.current_dir(&workspace);
                    }

                    command.status().await.with_context(|| {
                        format!(
                            "Failed to spawn scheduled VT Code exec for task {} using {} in {}",
                            record.definition.id,
                            self.executable_path.display(),
                            workspace_label
                        )
                    })
                }
                .await;
                let run_status = match execution {
                    Ok(status) if status.success() => TaskRunStatus::Success,
                    Ok(status) => TaskRunStatus::Failed {
                        message: format!("vtcode exec exited with {status}"),
                    },
                    Err(error) => TaskRunStatus::Failed { message: format!("{error:#}") },
                };
                Ok(RunOutcome {
                    ran_at: run_at,
                    status: run_status,
                    artifact_dir: artifact_dir.is_dir().then_some(artifact_dir),
                    events_file: events_file.is_file().then_some(events_file),
                    last_message_file: last_message_file.is_file().then_some(last_message_file),
                })
            }
        }
    }
}

fn apply_run_outcome(record: &mut ScheduledTaskRecord, outcome: RunOutcome) -> Result<()> {
    let RunOutcome {
        ran_at,
        status,
        artifact_dir,
        events_file,
        last_message_file,
    } = outcome;

    record.runtime.last_artifact_dir = artifact_dir;
    record.runtime.last_events_file = events_file;
    record.runtime.last_message_file = last_message_file;

    advance_record_runtime(record, ran_at, status)?;

    Ok(())
}

#[derive(Debug, Clone)]
struct RunOutcome {
    ran_at: DateTime<Utc>,
    status: TaskRunStatus,
    artifact_dir: Option<PathBuf>,
    events_file: Option<PathBuf>,
    last_message_file: Option<PathBuf>,
}

fn scheduled_workspace_label(definition: &ScheduledTaskDefinition) -> String {
    definition
        .workspace
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "<none>".to_string())
}

fn resolve_scheduled_task_workspace(definition: &ScheduledTaskDefinition) -> Result<Option<PathBuf>> {
    definition
        .workspace
        .as_deref()
        .map(resolve_scheduled_workspace_path)
        .transpose()
        .with_context(|| {
            format!(
                "Failed to resolve scheduled task workspace {} for task {}",
                scheduled_workspace_label(definition),
                definition.id
            )
        })
}
