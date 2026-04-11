use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use vtcode_config::{DiscoveredSubagents, SubagentSpec};

use crate::config::VTCodeConfig;
use crate::core::agent::task::TaskOutcome;
use crate::core::threads::{ThreadRuntimeHandle, ThreadSnapshot};
use crate::exec::events::ThreadEvent;
use crate::llm::provider::Message;
use crate::utils::session_archive::SessionArchiveMetadata;

// ─── Public Status Types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Queued,
    Running,
    Waiting,
    Completed,
    Failed,
    Closed,
}

impl SubagentStatus {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Closed)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundSubprocessStatus {
    Starting,
    Running,
    Stopped,
    Error,
}

impl BackgroundSubprocessStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped => "stopped",
            Self::Error => "error",
        }
    }

    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

// ─── Public DTOs ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStatusEntry {
    pub id: String,
    pub session_id: String,
    pub parent_thread_id: String,
    pub agent_name: String,
    pub display_label: String,
    pub description: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub status: SubagentStatus,
    pub background: bool,
    pub depth: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundSubprocessEntry {
    pub id: String,
    pub session_id: String,
    pub exec_session_id: String,
    pub agent_name: String,
    pub display_label: String,
    pub description: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub status: BackgroundSubprocessStatus,
    pub desired_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundSubprocessSnapshot {
    pub entry: BackgroundSubprocessEntry,
    #[serde(default)]
    pub preview: String,
}

#[derive(Debug, Clone)]
pub struct SubagentThreadSnapshot {
    pub id: String,
    pub session_id: String,
    pub parent_thread_id: String,
    pub agent_name: String,
    pub display_label: String,
    pub status: SubagentStatus,
    pub background: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archive_path: Option<PathBuf>,
    pub transcript_path: Option<PathBuf>,
    pub effective_config: VTCodeConfig,
    pub snapshot: ThreadSnapshot,
    pub recent_events: Vec<ThreadEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentInputItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SpawnAgentRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub items: Vec<SubagentInputItem>,
    #[serde(default)]
    pub fork_context: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SendInputRequest {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub items: Vec<SubagentInputItem>,
    #[serde(default)]
    pub interrupt: bool,
}

// ─── Internal Records ───────────────────────────────────────────────────────

pub struct ChildRecord {
    pub(crate) id: String,
    pub(crate) session_id: String,
    pub(crate) parent_thread_id: String,
    pub(crate) spec: SubagentSpec,
    pub(crate) display_label: String,
    pub(crate) status: SubagentStatus,
    pub(crate) background: bool,
    pub(crate) depth: usize,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) completed_at: Option<DateTime<Utc>>,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) archive_metadata: Option<SessionArchiveMetadata>,
    pub(crate) archive_path: Option<PathBuf>,
    pub(crate) transcript_path: Option<PathBuf>,
    pub(crate) effective_config: Option<VTCodeConfig>,
    pub(crate) stored_messages: Vec<Message>,
    pub(crate) last_prompt: Option<String>,
    pub(crate) queued_prompts: VecDeque<String>,
    pub(crate) thread_handle: Option<ThreadRuntimeHandle>,
    pub(crate) handle: Option<JoinHandle<()>>,
    pub(crate) notify: Arc<Notify>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedBackgroundRecord {
    pub(crate) id: String,
    pub(crate) agent_name: String,
    pub(crate) display_label: String,
    pub(crate) description: String,
    pub(crate) source: String,
    pub(crate) color: Option<String>,
    pub(crate) session_id: String,
    pub(crate) exec_session_id: String,
    pub(crate) desired_enabled: bool,
    pub(crate) status: BackgroundSubprocessStatus,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) ended_at: Option<DateTime<Utc>>,
    pub(crate) pid: Option<u32>,
    pub(crate) prompt: String,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) archive_path: Option<PathBuf>,
    pub(crate) transcript_path: Option<PathBuf>,
    pub(crate) max_turns: Option<usize>,
    pub(crate) model_override: Option<String>,
    pub(crate) reasoning_override: Option<String>,
    pub(crate) restart_attempts: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedBackgroundState {
    #[serde(default)]
    pub(crate) records: Vec<PersistedBackgroundRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRecord {
    pub(crate) id: String,
    pub(crate) agent_name: String,
    pub(crate) display_label: String,
    pub(crate) description: String,
    pub(crate) source: String,
    pub(crate) color: Option<String>,
    pub(crate) session_id: String,
    pub(crate) exec_session_id: String,
    pub(crate) desired_enabled: bool,
    pub(crate) status: BackgroundSubprocessStatus,
    pub(crate) created_at: DateTime<Utc>,
    pub(crate) updated_at: DateTime<Utc>,
    pub(crate) started_at: Option<DateTime<Utc>>,
    pub(crate) ended_at: Option<DateTime<Utc>>,
    pub(crate) pid: Option<u32>,
    pub(crate) prompt: String,
    pub(crate) summary: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) archive_path: Option<PathBuf>,
    pub(crate) transcript_path: Option<PathBuf>,
    pub(crate) max_turns: Option<usize>,
    pub(crate) model_override: Option<String>,
    pub(crate) reasoning_override: Option<String>,
    pub(crate) restart_attempts: u8,
}

// ─── Status Entry Builders ──────────────────────────────────────────────────

pub trait StatusEntryBuilder {
    type Entry;
    fn build_status_entry(&self) -> Self::Entry;
}

impl StatusEntryBuilder for BackgroundRecord {
    type Entry = BackgroundSubprocessEntry;

    fn build_status_entry(&self) -> BackgroundSubprocessEntry {
        BackgroundSubprocessEntry {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            exec_session_id: self.exec_session_id.clone(),
            agent_name: self.agent_name.clone(),
            display_label: self.display_label.clone(),
            description: self.description.clone(),
            source: self.source.clone(),
            color: self.color.clone(),
            status: self.status,
            desired_enabled: self.desired_enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
            started_at: self.started_at,
            ended_at: self.ended_at,
            pid: self.pid,
            summary: self.summary.clone(),
            error: self.error.clone(),
            archive_path: self.archive_path.clone(),
            transcript_path: self.transcript_path.clone(),
        }
    }
}

impl StatusEntryBuilder for ChildRecord {
    type Entry = SubagentStatusEntry;

    fn build_status_entry(&self) -> SubagentStatusEntry {
        SubagentStatusEntry {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            parent_thread_id: self.parent_thread_id.clone(),
            agent_name: self.spec.name.clone(),
            display_label: self.display_label.clone(),
            description: self.spec.description.clone(),
            source: self.spec.source.label(),
            color: self.spec.color.clone(),
            status: self.status,
            background: self.background,
            depth: self.depth,
            created_at: self.created_at,
            updated_at: self.updated_at,
            completed_at: self.completed_at,
            summary: self.summary.clone(),
            error: self.error.clone(),
            transcript_path: self.transcript_path.clone(),
            nickname: self.spec.nickname_candidates.first().cloned(),
        }
    }
}

impl ChildRecord {
    /// Apply execution result to the record state. Returns the next queued
    /// prompt if the child should continue looping.
    pub(crate) fn apply_result(
        &mut self,
        execute: anyhow::Result<ChildRunResult>,
    ) -> Option<String> {
        match execute {
            Ok(result) => {
                self.status = if result.outcome.is_success() {
                    SubagentStatus::Completed
                } else {
                    SubagentStatus::Failed
                };
                self.summary = Some(result.summary.clone());
                self.error = match result.outcome {
                    TaskOutcome::Failed { reason } => Some(reason),
                    _ => None,
                };
                self.transcript_path = result.transcript_path.clone();
                self.stored_messages = result.messages;
            }
            Err(error) => {
                self.status = SubagentStatus::Failed;
                self.summary = None;
                self.error = Some(error.to_string());
            }
        }
        let next = self.queued_prompts.pop_front();
        if next.is_some() {
            self.status = SubagentStatus::Queued;
            self.completed_at = None;
        } else if self.status.is_terminal() {
            self.completed_at = Some(Utc::now());
        }
        self.notify.notify_waiters();
        next
    }

    /// Build the hook payload for a terminal run (no more queued prompts).
    pub(crate) fn build_hook_payload(
        &self,
    ) -> (
        String,
        String,
        String,
        String,
        bool,
        String,
        Option<PathBuf>,
    ) {
        (
            self.parent_thread_id.clone(),
            self.session_id.clone(),
            self.spec.name.clone(),
            self.display_label.clone(),
            self.background,
            self.status.as_str().to_string(),
            self.transcript_path
                .clone()
                .or_else(|| self.archive_path.clone()),
        )
    }
}

impl BackgroundRecord {
    pub(crate) fn into_persisted(self) -> PersistedBackgroundRecord {
        PersistedBackgroundRecord {
            id: self.id,
            agent_name: self.agent_name,
            display_label: self.display_label,
            description: self.description,
            source: self.source,
            color: self.color,
            session_id: self.session_id,
            exec_session_id: self.exec_session_id,
            desired_enabled: self.desired_enabled,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            started_at: self.started_at,
            ended_at: self.ended_at,
            pid: self.pid,
            prompt: self.prompt,
            summary: self.summary,
            error: self.error,
            archive_path: self.archive_path,
            transcript_path: self.transcript_path,
            max_turns: self.max_turns,
            model_override: self.model_override,
            reasoning_override: self.reasoning_override,
            restart_attempts: self.restart_attempts,
        }
    }

    pub(crate) fn from_persisted(record: PersistedBackgroundRecord) -> Self {
        Self {
            id: record.id,
            agent_name: record.agent_name,
            display_label: record.display_label,
            description: record.description,
            source: record.source,
            color: record.color,
            session_id: record.session_id,
            exec_session_id: record.exec_session_id,
            desired_enabled: record.desired_enabled,
            status: record.status,
            created_at: record.created_at,
            updated_at: record.updated_at,
            started_at: record.started_at,
            ended_at: record.ended_at,
            pid: record.pid,
            prompt: record.prompt,
            summary: record.summary,
            error: record.error,
            archive_path: record.archive_path,
            transcript_path: record.transcript_path,
            max_turns: record.max_turns,
            model_override: record.model_override,
            reasoning_override: record.reasoning_override,
            restart_attempts: record.restart_attempts,
        }
    }
}

// ─── Controller State ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct TurnDelegationHints {
    pub(crate) explicit_mentions: Vec<String>,
    pub(crate) explicit_request: bool,
    pub(crate) current_input: String,
}

pub struct ControllerState {
    pub(crate) discovered: DiscoveredSubagents,
    pub(crate) parent_messages: Vec<Message>,
    pub(crate) turn_hints: TurnDelegationHints,
    pub(crate) children: BTreeMap<String, ChildRecord>,
    pub(crate) background_children: BTreeMap<String, BackgroundRecord>,
}

// ─── Child Run Result ───────────────────────────────────────────────────────

pub struct ChildRunResult {
    pub(crate) messages: Vec<Message>,
    pub(crate) summary: String,
    pub(crate) outcome: TaskOutcome,
    pub(crate) transcript_path: Option<PathBuf>,
}
