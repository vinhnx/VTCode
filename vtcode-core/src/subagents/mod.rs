use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use futures::future::select_all;
use portable_pty::PtySize;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};
use tokio::task::JoinHandle;

use crate::config::VTCodeConfig;
use crate::config::constants::models;
use crate::config::constants::tools;
use crate::config::models::{ModelId, Provider};
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::{Task, TaskOutcome};
use crate::core::agent::types::AgentType;
use crate::core::threads::{
    ThreadBootstrap, ThreadId, ThreadRuntimeHandle, ThreadSnapshot, build_thread_archive_metadata,
};
use crate::exec::events::ThreadEvent;
use crate::hooks::LifecycleHookEngine;
use crate::llm::auto_lightweight_model;
use crate::llm::factory::{infer_provider, infer_provider_from_model};
use crate::llm::provider::{Message, ToolDefinition};
use crate::plugins::components::AgentsHandler;
use crate::plugins::manifest::PluginManifest;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::pty::PtyManager;
use crate::utils::file_utils::ensure_dir_exists;
use crate::utils::session_archive::{
    SessionArchive, SessionArchiveMetadata, SessionForkMode, SessionListing, SessionMessage,
    SessionSnapshot, find_session_by_identifier,
};
use vtcode_config::auth::OpenAIChatGptAuthHandle;
use vtcode_config::{
    DiscoveredSubagents, HooksConfig, McpProviderConfig, PermissionMode, SubagentDiscoveryInput,
    SubagentMcpServer, SubagentMemoryScope, SubagentSpec, discover_subagents,
};

const SUBAGENT_TRANSCRIPT_LINE_LIMIT: usize = 200;
const SUBAGENT_MEMORY_BYTES_LIMIT: usize = 25 * 1024;
const SUBAGENT_MEMORY_LINE_LIMIT: usize = 200;
const SUBAGENT_HARD_CONCURRENCY_LIMIT: usize = 3;
const SUBAGENT_MIN_MAX_TURNS: usize = 2;
const VAGUE_SUBAGENT_PROMPTS: &[&str] = &[
    "analyze",
    "analyse",
    "check",
    "current state",
    "explore",
    "help",
    "inspect",
    "inspect current state",
    "look",
    "look around",
    "report",
    "report findings",
    "report status",
    "review",
    "status",
    "summarise",
    "summarize",
    "summary",
];

const SUBAGENT_TOOL_NAMES: &[&str] = &[
    tools::SPAWN_AGENT,
    tools::SEND_INPUT,
    tools::WAIT_AGENT,
    tools::RESUME_AGENT,
    tools::CLOSE_AGENT,
];

const NON_MUTATING_TOOL_PREFIXES: &[&str] = &[
    tools::UNIFIED_SEARCH,
    tools::READ_FILE,
    tools::LIST_SKILLS,
    tools::LOAD_SKILL,
    tools::LOAD_SKILL_RESOURCE,
];

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

#[must_use]
pub fn delegated_task_requires_clarification(prompt: &str) -> bool {
    let normalized = prompt
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '.' | ',' | '!' | '?' | ':' | ';'))
        .to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    if VAGUE_SUBAGENT_PROMPTS
        .iter()
        .any(|candidate| normalized == *candidate)
    {
        return true;
    }
    normalized.split_whitespace().count() == 1
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

#[derive(Clone)]
pub struct SubagentControllerConfig {
    pub workspace_root: PathBuf,
    pub parent_session_id: String,
    pub parent_model: String,
    pub parent_provider: String,
    pub parent_reasoning_effort: ReasoningEffortLevel,
    pub api_key: String,
    pub vt_cfg: VTCodeConfig,
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    pub depth: usize,
    pub exec_sessions: ExecSessionManager,
    pub pty_manager: PtyManager,
}

struct ChildRecord {
    id: String,
    session_id: String,
    parent_thread_id: String,
    spec: SubagentSpec,
    display_label: String,
    status: SubagentStatus,
    background: bool,
    depth: usize,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    summary: Option<String>,
    error: Option<String>,
    archive_metadata: Option<SessionArchiveMetadata>,
    archive_path: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    effective_config: Option<VTCodeConfig>,
    stored_messages: Vec<Message>,
    last_prompt: Option<String>,
    queued_prompts: VecDeque<String>,
    thread_handle: Option<ThreadRuntimeHandle>,
    handle: Option<JoinHandle<()>>,
    notify: Arc<Notify>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedBackgroundRecord {
    id: String,
    agent_name: String,
    display_label: String,
    description: String,
    source: String,
    color: Option<String>,
    session_id: String,
    exec_session_id: String,
    desired_enabled: bool,
    status: BackgroundSubprocessStatus,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    pid: Option<u32>,
    prompt: String,
    summary: Option<String>,
    error: Option<String>,
    archive_path: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    max_turns: Option<usize>,
    model_override: Option<String>,
    reasoning_override: Option<String>,
    restart_attempts: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistedBackgroundState {
    #[serde(default)]
    records: Vec<PersistedBackgroundRecord>,
}

struct BackgroundRecord {
    id: String,
    agent_name: String,
    display_label: String,
    description: String,
    source: String,
    color: Option<String>,
    session_id: String,
    exec_session_id: String,
    desired_enabled: bool,
    status: BackgroundSubprocessStatus,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    pid: Option<u32>,
    prompt: String,
    summary: Option<String>,
    error: Option<String>,
    archive_path: Option<PathBuf>,
    transcript_path: Option<PathBuf>,
    max_turns: Option<usize>,
    model_override: Option<String>,
    reasoning_override: Option<String>,
    restart_attempts: u8,
}

impl BackgroundRecord {
    fn status_entry(&self) -> BackgroundSubprocessEntry {
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

    fn persisted(&self) -> PersistedBackgroundRecord {
        PersistedBackgroundRecord {
            id: self.id.clone(),
            agent_name: self.agent_name.clone(),
            display_label: self.display_label.clone(),
            description: self.description.clone(),
            source: self.source.clone(),
            color: self.color.clone(),
            session_id: self.session_id.clone(),
            exec_session_id: self.exec_session_id.clone(),
            desired_enabled: self.desired_enabled,
            status: self.status,
            created_at: self.created_at,
            updated_at: self.updated_at,
            started_at: self.started_at,
            ended_at: self.ended_at,
            pid: self.pid,
            prompt: self.prompt.clone(),
            summary: self.summary.clone(),
            error: self.error.clone(),
            archive_path: self.archive_path.clone(),
            transcript_path: self.transcript_path.clone(),
            max_turns: self.max_turns,
            model_override: self.model_override.clone(),
            reasoning_override: self.reasoning_override.clone(),
            restart_attempts: self.restart_attempts,
        }
    }
}

impl ChildRecord {
    fn status_entry(&self) -> SubagentStatusEntry {
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

#[derive(Debug, Clone, Default)]
struct TurnDelegationHints {
    explicit_mentions: Vec<String>,
    explicit_request: bool,
    current_input: String,
}

struct ControllerState {
    discovered: DiscoveredSubagents,
    parent_messages: Vec<Message>,
    turn_hints: TurnDelegationHints,
    children: BTreeMap<String, ChildRecord>,
    background_children: BTreeMap<String, BackgroundRecord>,
}

#[derive(Clone)]
pub struct SubagentController {
    config: Arc<SubagentControllerConfig>,
    parent_session_id: Arc<RwLock<String>>,
    lifecycle_hooks: Option<LifecycleHookEngine>,
    state: Arc<RwLock<ControllerState>>,
}

impl SubagentController {
    pub async fn new(config: SubagentControllerConfig) -> Result<Self> {
        let discovered = discover_controller_subagents(&config.workspace_root).await?;
        let lifecycle_hooks = LifecycleHookEngine::new(
            config.workspace_root.clone(),
            &config.vt_cfg.hooks,
            crate::hooks::SessionStartTrigger::Startup,
        )?;
        let background_children = load_background_state(&config.workspace_root)?
            .records
            .into_iter()
            .map(|record| {
                (
                    record.id.clone(),
                    BackgroundRecord {
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
                    },
                )
            })
            .collect();
        Ok(Self {
            parent_session_id: Arc::new(RwLock::new(config.parent_session_id.clone())),
            lifecycle_hooks,
            config: Arc::new(config),
            state: Arc::new(RwLock::new(ControllerState {
                discovered,
                parent_messages: Vec::new(),
                turn_hints: TurnDelegationHints::default(),
                children: BTreeMap::new(),
                background_children,
            })),
        })
    }

    pub async fn reload(&self) -> Result<()> {
        let discovered = discover_controller_subagents(&self.config.workspace_root).await?;
        self.state.write().await.discovered = discovered;
        Ok(())
    }

    pub async fn set_parent_messages(&self, messages: &[Message]) {
        self.state.write().await.parent_messages = messages.to_vec();
    }

    pub async fn set_turn_delegation_hints_from_input(&self, input: &str) -> Vec<String> {
        let mut state = self.state.write().await;
        let explicit_mentions =
            extract_explicit_agent_mentions(input, state.discovered.effective.as_slice());
        let explicit_request =
            contains_explicit_delegation_request(input, explicit_mentions.as_slice());
        state.turn_hints = TurnDelegationHints {
            explicit_mentions: explicit_mentions.clone(),
            explicit_request,
            current_input: input.to_string(),
        };
        explicit_mentions
    }

    pub async fn clear_turn_delegation_hints(&self) {
        self.state.write().await.turn_hints = TurnDelegationHints::default();
    }

    pub async fn set_parent_session_id(&self, session_id: impl Into<String>) {
        *self.parent_session_id.write().await = session_id.into();
    }

    pub async fn effective_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.effective.clone()
    }

    pub async fn shadowed_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.shadowed.clone()
    }

    pub async fn status_entries(&self) -> Vec<SubagentStatusEntry> {
        let state = self.state.read().await;
        state
            .children
            .values()
            .map(ChildRecord::status_entry)
            .collect()
    }

    pub async fn background_status_entries(&self) -> Vec<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        state
            .background_children
            .values()
            .map(BackgroundRecord::status_entry)
            .collect()
    }

    pub async fn background_snapshot(&self, target: &str) -> Result<BackgroundSubprocessSnapshot> {
        let _ = self.refresh_background_processes().await?;

        let entry = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?
                .status_entry()
        };

        let preview = if entry.exec_session_id.is_empty() {
            String::new()
        } else {
            match self
                .config
                .exec_sessions
                .read_session_output(&entry.exec_session_id, false)
                .await
            {
                Ok(Some(output)) => summarize_background_preview(&output),
                Ok(None) | Err(_) => {
                    if let Some(path) = entry
                        .transcript_path
                        .as_ref()
                        .or(entry.archive_path.as_ref())
                    {
                        load_archive_preview(path).unwrap_or_default()
                    } else {
                        String::new()
                    }
                }
            }
        };

        Ok(BackgroundSubprocessSnapshot { entry, preview })
    }

    #[must_use]
    pub fn background_subagents_enabled(&self) -> bool {
        self.config.vt_cfg.subagents.background.enabled
    }

    #[must_use]
    pub fn configured_default_background_agent(&self) -> Option<&str> {
        self.config
            .vt_cfg
            .subagents
            .background
            .default_agent
            .as_deref()
            .map(str::trim)
            .filter(|agent| !agent.is_empty())
    }

    pub async fn toggle_default_background_subagent(&self) -> Result<BackgroundSubprocessEntry> {
        if !self.background_subagents_enabled() {
            bail!("Background subagents are disabled by configuration");
        }

        let agent_name = self
            .configured_default_background_agent()
            .ok_or_else(|| anyhow!("No default background subagent is configured"))?
            .to_string();
        let target_id = background_record_id(agent_name.as_str());
        let should_stop = {
            let state = self.state.read().await;
            state
                .background_children
                .get(&target_id)
                .is_some_and(|record| {
                    record.desired_enabled
                        && matches!(
                            record.status,
                            BackgroundSubprocessStatus::Starting
                                | BackgroundSubprocessStatus::Running
                        )
                })
        };

        if should_stop {
            self.graceful_stop_background(&target_id).await
        } else {
            self.ensure_background_record_running(agent_name.as_str(), Some(target_id.as_str()), 0)
                .await
        }
    }

    pub async fn restore_background_subagents(&self) -> Result<Vec<BackgroundSubprocessEntry>> {
        let desired_records = {
            let state = self.state.read().await;
            state
                .background_children
                .values()
                .filter(|record| record.desired_enabled)
                .map(|record| {
                    (
                        record.id.clone(),
                        record.agent_name.clone(),
                        record.exec_session_id.clone(),
                        record.restart_attempts,
                    )
                })
                .collect::<Vec<_>>()
        };

        for (record_id, agent_name, exec_session_id, restart_attempts) in desired_records {
            let is_live = if exec_session_id.is_empty() {
                false
            } else {
                self.config
                    .exec_sessions
                    .snapshot_session(&exec_session_id)
                    .await
                    .ok()
                    .is_some_and(|snapshot| exec_session_is_running(&snapshot))
            };
            if is_live || !self.config.vt_cfg.subagents.background.auto_restore {
                continue;
            }
            tracing::info!(
                agent_name = agent_name.as_str(),
                record_id = record_id.as_str(),
                "Restoring background subagent subprocess"
            );
            self.ensure_background_record_running(
                agent_name.as_str(),
                Some(record_id.as_str()),
                restart_attempts,
            )
            .await?;
        }

        self.refresh_background_processes().await
    }

    pub async fn refresh_background_processes(&self) -> Result<Vec<BackgroundSubprocessEntry>> {
        let record_ids = {
            let state = self.state.read().await;
            state
                .background_children
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        for record_id in record_ids {
            let snapshot_target = {
                let state = self.state.read().await;
                state
                    .background_children
                    .get(&record_id)
                    .map(|record| record.exec_session_id.clone())
            };

            let snapshot = if let Some(exec_session_id) = snapshot_target.as_ref()
                && !exec_session_id.is_empty()
            {
                self.config
                    .exec_sessions
                    .snapshot_session(exec_session_id)
                    .await
                    .ok()
            } else {
                None
            };

            let mut respawn = None;
            {
                let mut state = self.state.write().await;
                let Some(record) = state.background_children.get_mut(&record_id) else {
                    continue;
                };
                record.updated_at = Utc::now();

                if let Some(snapshot) = snapshot {
                    record.pid = snapshot.child_pid;
                    record.started_at = snapshot.started_at.or(record.started_at);

                    match snapshot.lifecycle_state {
                        Some(crate::tools::types::VTCodeSessionLifecycleState::Running) => {
                            record.status = BackgroundSubprocessStatus::Running;
                            record.ended_at = None;
                            record.error = None;
                        }
                        Some(crate::tools::types::VTCodeSessionLifecycleState::Exited) | None => {
                            record.ended_at.get_or_insert(Utc::now());
                            if record.desired_enabled
                                && self.config.vt_cfg.subagents.background.auto_restore
                                && record.restart_attempts < 1
                            {
                                let next_restart_attempt =
                                    record.restart_attempts.saturating_add(1);
                                record.restart_attempts = next_restart_attempt;
                                record.status = BackgroundSubprocessStatus::Starting;
                                tracing::warn!(
                                    agent_name = record.agent_name.as_str(),
                                    record_id = record.id.as_str(),
                                    attempt = next_restart_attempt,
                                    "Background subprocess exited unexpectedly; scheduling restart"
                                );
                                respawn = Some((
                                    record.agent_name.clone(),
                                    record.id.clone(),
                                    next_restart_attempt,
                                ));
                            } else if record.desired_enabled {
                                record.status = BackgroundSubprocessStatus::Error;
                                record.error = Some(match snapshot.exit_code {
                                    Some(exit_code) => {
                                        format!(
                                            "Background subprocess exited with code {exit_code}"
                                        )
                                    }
                                    None => "Background subprocess exited unexpectedly".to_string(),
                                });
                            } else {
                                record.status = BackgroundSubprocessStatus::Stopped;
                            }
                        }
                    }
                } else if record.desired_enabled
                    && self.config.vt_cfg.subagents.background.auto_restore
                {
                    if record.restart_attempts < 1 {
                        let next_restart_attempt = record.restart_attempts.saturating_add(1);
                        record.restart_attempts = next_restart_attempt;
                        record.status = BackgroundSubprocessStatus::Starting;
                        tracing::warn!(
                            agent_name = record.agent_name.as_str(),
                            record_id = record.id.as_str(),
                            attempt = next_restart_attempt,
                            "Background subprocess is missing; scheduling restart"
                        );
                        respawn = Some((
                            record.agent_name.clone(),
                            record.id.clone(),
                            next_restart_attempt,
                        ));
                    } else {
                        record.status = BackgroundSubprocessStatus::Error;
                        record.error = Some("Background subprocess is not running".to_string());
                        record.ended_at.get_or_insert(Utc::now());
                    }
                } else if !record.desired_enabled {
                    record.status = BackgroundSubprocessStatus::Stopped;
                    record.ended_at.get_or_insert(Utc::now());
                }
            }

            if let Some((agent_name, stable_id, restart_attempts)) = respawn {
                self.ensure_background_record_running(
                    agent_name.as_str(),
                    Some(stable_id.as_str()),
                    restart_attempts,
                )
                .await?;
            }

            self.refresh_background_archive_metadata(&record_id).await?;
        }

        self.save_background_state().await?;
        Ok(self.background_status_entries().await)
    }

    pub async fn graceful_stop_background(
        &self,
        target: &str,
    ) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
            record.updated_at = Utc::now();
            record.ended_at = Some(Utc::now());
            (record.agent_name.clone(), record.exec_session_id.clone())
        };

        tracing::info!(
            agent_name = agent_name.as_str(),
            record_id = target,
            exec_session_id = exec_session_id.as_str(),
            "Gracefully stopping background subagent subprocess"
        );

        if !exec_session_id.is_empty() {
            let _ = self
                .config
                .exec_sessions
                .terminate_session(&exec_session_id)
                .await;
            let _ = self
                .config
                .exec_sessions
                .prune_exited_session(&exec_session_id)
                .await;
        }

        self.refresh_background_archive_metadata(target).await?;
        self.save_background_state().await?;
        self.background_status_for(target).await
    }

    pub async fn force_cancel_background(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let (agent_name, exec_session_id) = {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
            record.desired_enabled = false;
            record.status = BackgroundSubprocessStatus::Stopped;
            record.updated_at = Utc::now();
            record.ended_at = Some(Utc::now());
            (record.agent_name.clone(), record.exec_session_id.clone())
        };

        tracing::info!(
            agent_name = agent_name.as_str(),
            record_id = target,
            exec_session_id = exec_session_id.as_str(),
            "Force cancelling background subagent subprocess"
        );

        if !exec_session_id.is_empty() {
            let _ = self
                .config
                .exec_sessions
                .close_session(&exec_session_id)
                .await;
        }

        self.refresh_background_archive_metadata(target).await?;
        self.save_background_state().await?;
        self.background_status_for(target).await
    }

    pub async fn snapshot_for_thread(&self, target: &str) -> Result<SubagentThreadSnapshot> {
        let (
            id,
            session_id,
            parent_thread_id,
            agent_name,
            display_label,
            status,
            background,
            created_at,
            updated_at,
            archive_path,
            transcript_path,
            effective_config,
            thread_handle,
            archive_metadata,
            stored_messages,
            recent_events,
        ) = {
            let state = self.state.read().await;
            let record = state
                .children
                .get(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
            (
                record.id.clone(),
                record.session_id.clone(),
                record.parent_thread_id.clone(),
                record.spec.name.clone(),
                record.display_label.clone(),
                record.status,
                record.background,
                record.created_at,
                record.updated_at,
                record.archive_path.clone(),
                record.transcript_path.clone(),
                record.effective_config.clone(),
                record.thread_handle.clone(),
                record.archive_metadata.clone(),
                record.stored_messages.clone(),
                record
                    .thread_handle
                    .as_ref()
                    .map(ThreadRuntimeHandle::recent_events)
                    .unwrap_or_default(),
            )
        };

        let effective_config = effective_config.ok_or_else(|| {
            anyhow!(
                "Subagent {} does not have a captured runtime configuration yet",
                target
            )
        })?;
        let snapshot = match thread_handle {
            Some(handle) => handle.snapshot(),
            None => {
                let archive_listing = match archive_path.as_ref() {
                    Some(path) if path.exists() => load_session_listing(path).ok(),
                    _ => None,
                };
                let metadata = archive_listing
                    .as_ref()
                    .map(|listing| listing.snapshot.metadata.clone())
                    .or(archive_metadata)
                    .or_else(|| {
                        Some(build_thread_archive_metadata(
                            &self.config.workspace_root,
                            effective_config.agent.default_model.as_str(),
                            effective_config.agent.provider.as_str(),
                            effective_config.agent.theme.as_str(),
                            effective_config.agent.reasoning_effort.as_str(),
                        ))
                    });
                ThreadSnapshot {
                    thread_id: ThreadId::new(session_id.clone()),
                    metadata,
                    archive_listing,
                    messages: stored_messages,
                    loaded_skills: Vec::new(),
                    turn_in_flight: false,
                }
            }
        };

        Ok(SubagentThreadSnapshot {
            id,
            session_id,
            parent_thread_id,
            agent_name,
            display_label,
            status,
            background,
            created_at,
            updated_at,
            archive_path,
            transcript_path,
            effective_config,
            snapshot,
            recent_events,
        })
    }

    pub async fn spawn(&self, request: SpawnAgentRequest) -> Result<SubagentStatusEntry> {
        let mut request = request;
        let (requested_agent, hints) = {
            let state = self.state.read().await;
            sanitize_subagent_input_items(&mut request.items);
            request.model = normalize_requested_model_override(
                request.model.take(),
                &state.turn_hints.current_input,
            );
            let requested_agent = if let Some(agent_type) = request.agent_type.as_deref() {
                Some(agent_type.to_string())
            } else {
                match state.turn_hints.explicit_mentions.as_slice() {
                    [] => None,
                    [single] => Some(single.clone()),
                    mentions => {
                        bail!(
                            "spawn_agent omitted agent_type, but the user explicitly selected multiple agents: {}. Specify agent_type explicitly.",
                            mentions.join(", ")
                        );
                    }
                }
            };
            (requested_agent, state.turn_hints.clone())
        };
        let spec = self
            .resolve_requested_spec(requested_agent.as_deref())
            .await?;
        if let Some(explicit) = hints.explicit_mentions.as_slice().first()
            && hints.explicit_mentions.len() == 1
            && !spec.matches_name(explicit)
        {
            bail!(
                "spawn_agent requested agent_type '{}', but the user explicitly selected '{}'. Use the selected agent or ask the user to clarify.",
                spec.name,
                explicit
            );
        }
        if !spec.is_read_only() && !hints.explicit_request {
            bail!(
                "spawn_agent cannot launch write-capable agent '{}' without an explicit delegation signal from the current user turn. Ask the user to mention the agent, say 'delegate'/'spawn', or request parallel work.",
                spec.name
            );
        }
        if spec.is_read_only()
            && !self.config.vt_cfg.subagents.auto_delegate_read_only
            && !hints.explicit_request
        {
            bail!(
                "spawn_agent cannot proactively launch read-only agent '{}' because `subagents.auto_delegate_read_only` is disabled and the current user turn did not explicitly request delegation.",
                spec.name
            );
        }
        if let Some(requested_model) = request.model.as_deref()
            && !contains_explicit_model_request(&hints.current_input, requested_model)
        {
            tracing::warn!(
                agent_name = spec.name.as_str(),
                requested_model = requested_model.trim(),
                "Ignoring subagent model override because the current user turn did not explicitly request that model"
            );
            request.model = None;
        }
        let prompt = request_prompt(&request.message, &request.items)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("spawn_agent requires a task message or items"))?;
        if delegated_task_requires_clarification(&prompt) {
            bail!(
                "spawn_agent task for '{}' is too vague ('{}'). Ask the user for a specific delegated task before spawning the subagent.",
                spec.name,
                prompt.trim()
            );
        }
        self.spawn_with_spec(
            spec,
            prompt,
            request.fork_context,
            request.background,
            request.max_turns,
            request.model,
            request.reasoning_effort,
        )
        .await
    }

    pub async fn send_input(&self, request: SendInputRequest) -> Result<SubagentStatusEntry> {
        let prompt = request_prompt(&request.message, &request.items)
            .ok_or_else(|| anyhow!("send_input requires a message or items"))?;

        let maybe_restart = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(&request.target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", request.target))?;

            if record.status == SubagentStatus::Closed {
                bail!("Subagent {} is closed", request.target);
            }

            record.updated_at = Utc::now();
            record.last_prompt = Some(prompt.clone());

            if request.interrupt {
                if let Some(handle) = record.handle.take() {
                    handle.abort();
                }
                record.status = SubagentStatus::Queued;
                record.queued_prompts.clear();
                record.queued_prompts.push_back(prompt.clone());
                true
            } else if matches!(
                record.status,
                SubagentStatus::Running | SubagentStatus::Queued
            ) {
                record.status = SubagentStatus::Waiting;
                record.queued_prompts.push_back(prompt.clone());
                false
            } else {
                record.status = SubagentStatus::Queued;
                record.queued_prompts.push_back(prompt.clone());
                true
            }
        };

        if maybe_restart {
            self.restart_child(&request.target).await?;
        }

        self.status_for(&request.target).await
    }

    pub async fn resume(&self, target: &str) -> Result<SubagentStatusEntry> {
        {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
            if record.status == SubagentStatus::Closed {
                bail!("Subagent {} is closed", target);
            }
            if matches!(
                record.status,
                SubagentStatus::Running | SubagentStatus::Queued
            ) {
                return Ok(record.status_entry());
            }
            let prompt = record.last_prompt.clone().unwrap_or_else(|| {
                "Continue the delegated task from the existing context.".to_string()
            });
            record.status = SubagentStatus::Queued;
            record.updated_at = Utc::now();
            record.queued_prompts.push_back(prompt);
        }
        self.restart_child(target).await?;
        self.status_for(target).await
    }

    pub async fn close(&self, target: &str) -> Result<SubagentStatusEntry> {
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
        if let Some(handle) = record.handle.take() {
            handle.abort();
        }
        record.status = SubagentStatus::Closed;
        record.updated_at = Utc::now();
        record.completed_at = Some(Utc::now());
        record.notify.notify_waiters();
        Ok(record.status_entry())
    }

    pub async fn wait(
        &self,
        targets: &[String],
        timeout_ms: Option<u64>,
    ) -> Result<Option<SubagentStatusEntry>> {
        for target in targets {
            if let Ok(entry) = self.status_for(target).await
                && entry.status.is_terminal()
            {
                return Ok(Some(entry));
            }
        }

        let timeout = std::time::Duration::from_millis(timeout_ms.unwrap_or_else(|| {
            self.config
                .vt_cfg
                .subagents
                .default_timeout_seconds
                .saturating_mul(1000)
        }));
        let deadline = tokio::time::Instant::now() + timeout;

        loop {
            let notifies = {
                let state = self.state.read().await;
                targets
                    .iter()
                    .filter_map(|target| {
                        state
                            .children
                            .get(target)
                            .map(|record| record.notify.clone())
                    })
                    .collect::<Vec<_>>()
            };
            if notifies.is_empty() {
                return Ok(None);
            }

            for target in targets {
                if let Ok(entry) = self.status_for(target).await
                    && entry.status.is_terminal()
                {
                    return Ok(Some(entry));
                }
            }

            let sleep = tokio::time::sleep_until(deadline);
            tokio::pin!(sleep);
            let wait_any = select_all(
                notifies
                    .into_iter()
                    .map(|notify| Box::pin(async move { notify.notified().await }))
                    .collect::<Vec<_>>(),
            );
            tokio::pin!(wait_any);

            tokio::select! {
                _ = &mut sleep => return Ok(None),
                _ = &mut wait_any => {}
            }
        }
    }

    pub async fn status_for(&self, target: &str) -> Result<SubagentStatusEntry> {
        let state = self.state.read().await;
        let record = state
            .children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
        Ok(record.status_entry())
    }

    async fn background_status_for(&self, target: &str) -> Result<BackgroundSubprocessEntry> {
        let state = self.state.read().await;
        let record = state
            .background_children
            .get(target)
            .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?;
        Ok(record.status_entry())
    }

    async fn ensure_background_record_running(
        &self,
        agent_name: &str,
        stable_id: Option<&str>,
        restart_attempts: u8,
    ) -> Result<BackgroundSubprocessEntry> {
        let spec = self
            .resolve_requested_spec(Some(agent_name))
            .await
            .with_context(|| format!("Failed to resolve background subagent '{agent_name}'"))?;
        let record_id = stable_id
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| background_record_id(agent_name));
        let previous_record = {
            let state = self.state.read().await;
            state.background_children.get(&record_id).map(|record| {
                (
                    record.created_at,
                    record.prompt.clone(),
                    record.max_turns,
                    record.model_override.clone(),
                    record.reasoning_override.clone(),
                )
            })
        };
        let parent_session_id = self.parent_session_id.read().await.clone();
        let session_id = format!(
            "{}-{}-{}",
            sanitize_component(parent_session_id.as_str()),
            sanitize_component(record_id.as_str()),
            Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        );
        let exec_session_id = format!("exec-{session_id}");
        let (
            created_at,
            previous_prompt,
            previous_max_turns,
            previous_model_override,
            previous_reasoning_override,
        ) = previous_record.unwrap_or((Utc::now(), String::new(), None, None, None));
        let prompt = (!previous_prompt.trim().is_empty())
            .then_some(previous_prompt)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| {
                format!(
                    "You are the VT Code background subagent `{}`. Summarize readiness briefly, inspect the workspace at a high level, then remain idle until the process is terminated.",
                    spec.name
                )
            });
        let max_turns = normalize_child_max_turns(previous_max_turns.or(spec.max_turns));
        let model_override = previous_model_override.or_else(|| spec.model.clone());
        let reasoning_override =
            previous_reasoning_override.or_else(|| spec.reasoning_effort.clone());

        {
            let mut state = self.state.write().await;
            state.background_children.insert(
                record_id.clone(),
                BackgroundRecord {
                    id: record_id.clone(),
                    agent_name: spec.name.clone(),
                    display_label: subagent_display_label(&spec),
                    description: spec.description.clone(),
                    source: spec.source.label(),
                    color: spec.color.clone(),
                    session_id: session_id.clone(),
                    exec_session_id: exec_session_id.clone(),
                    desired_enabled: true,
                    status: BackgroundSubprocessStatus::Starting,
                    created_at,
                    updated_at: Utc::now(),
                    started_at: None,
                    ended_at: None,
                    pid: None,
                    prompt: prompt.clone(),
                    summary: Some("Starting background subagent".to_string()),
                    error: None,
                    archive_path: None,
                    transcript_path: None,
                    max_turns,
                    model_override: model_override.clone(),
                    reasoning_override: reasoning_override.clone(),
                    restart_attempts,
                },
            );
        }

        let command = build_background_subagent_command(
            &self.config.workspace_root,
            spec.name.as_str(),
            parent_session_id.as_str(),
            session_id.as_str(),
            prompt.as_str(),
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
        )?;
        let metadata = self
            .config
            .exec_sessions
            .create_pty_session(
                exec_session_id.clone(),
                command,
                self.config.workspace_root.clone(),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                hashbrown::HashMap::new(),
                None,
            )
            .await
            .with_context(|| {
                format!(
                    "Failed to spawn background subprocess for subagent '{}'",
                    spec.name
                )
            })?;

        tracing::info!(
            agent_name = spec.name.as_str(),
            record_id = record_id.as_str(),
            exec_session_id = exec_session_id.as_str(),
            pid = metadata.child_pid,
            "Spawned background subagent subprocess"
        );

        {
            let mut state = self.state.write().await;
            let record = state
                .background_children
                .get_mut(&record_id)
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", record_id))?;
            record.exec_session_id = exec_session_id;
            record.pid = metadata.child_pid;
            record.started_at = metadata.started_at;
            record.status = BackgroundSubprocessStatus::Running;
            record.updated_at = Utc::now();
            record.ended_at = None;
            record.error = None;
            record.summary = Some("Background subagent is running".to_string());
        }

        self.save_background_state().await?;
        self.background_status_for(&record_id).await
    }

    async fn refresh_background_archive_metadata(&self, target: &str) -> Result<()> {
        let session_id = {
            let state = self.state.read().await;
            state
                .background_children
                .get(target)
                .map(|record| record.session_id.clone())
                .ok_or_else(|| anyhow!("Unknown background subprocess {}", target))?
        };

        if let Some(listing) = find_session_by_identifier(&session_id).await? {
            let mut state = self.state.write().await;
            if let Some(record) = state.background_children.get_mut(target) {
                record.archive_path = Some(listing.path.clone());
                record.transcript_path = Some(listing.path);
            }
        }

        Ok(())
    }

    async fn save_background_state(&self) -> Result<()> {
        let records = {
            let state = self.state.read().await;
            state
                .background_children
                .values()
                .map(BackgroundRecord::persisted)
                .collect::<Vec<_>>()
        };
        let path = background_state_path(&self.config.workspace_root);
        if let Some(parent) = path.parent() {
            ensure_dir_exists(parent)
                .await
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let payload = serde_json::to_string_pretty(&PersistedBackgroundState { records })?;
        std::fs::write(&path, payload)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    async fn find_spec(&self, candidate: &str) -> Option<SubagentSpec> {
        self.state
            .read()
            .await
            .discovered
            .effective
            .iter()
            .find(|spec| spec.matches_name(candidate))
            .cloned()
    }

    async fn resolve_requested_spec(&self, requested: Option<&str>) -> Result<SubagentSpec> {
        let requested = requested.unwrap_or("default");
        self.find_spec(requested)
            .await
            .ok_or_else(|| anyhow!("Unknown subagent type {}", requested))
    }

    async fn spawn_with_spec(
        &self,
        spec: SubagentSpec,
        prompt: String,
        fork_context: bool,
        background: bool,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<SubagentStatusEntry> {
        if !self.config.vt_cfg.subagents.enabled {
            bail!("Subagents are disabled by configuration");
        }
        if self.config.depth.saturating_add(1) > self.config.vt_cfg.subagents.max_depth {
            bail!(
                "Subagent depth limit reached (max_depth={})",
                self.config.vt_cfg.subagents.max_depth
            );
        }
        if spec.isolation.as_deref() == Some("worktree") {
            bail!("Subagent isolation=worktree is not supported in this VT Code build");
        }

        let active_count = {
            let state = self.state.read().await;
            state
                .children
                .values()
                .filter(|record| {
                    matches!(
                        record.status,
                        SubagentStatus::Queued | SubagentStatus::Running | SubagentStatus::Waiting
                    )
                })
                .count()
        };
        let effective_max_concurrent = self
            .config
            .vt_cfg
            .subagents
            .max_concurrent
            .min(SUBAGENT_HARD_CONCURRENCY_LIMIT);
        if active_count >= effective_max_concurrent {
            bail!(
                "Subagent concurrency limit reached (max_concurrent={})",
                effective_max_concurrent
            );
        }
        let child_max_turns = normalize_child_max_turns(max_turns.or(spec.max_turns));
        let (_, _, effective_config) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            child_max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
        )?;

        let id = format!(
            "agent-{}-{}",
            sanitize_component(spec.name.as_str()),
            Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        );
        let parent_session_id = self.parent_session_id.read().await.clone();
        let session_id = format!(
            "{}-{}",
            sanitize_component(parent_session_id.as_str()),
            sanitize_component(id.as_str())
        );
        let display_label = subagent_display_label(&spec);
        let notify = Arc::new(Notify::new());
        let mut state = self.state.write().await;
        let initial_messages = if fork_context {
            state.parent_messages.clone()
        } else {
            Vec::new()
        };
        let entry = ChildRecord {
            id: id.clone(),
            session_id,
            parent_thread_id: parent_session_id,
            spec: spec.clone(),
            display_label,
            status: SubagentStatus::Queued,
            background: background || spec.background,
            depth: self.config.depth.saturating_add(1),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            summary: None,
            error: None,
            archive_metadata: None,
            archive_path: None,
            transcript_path: None,
            effective_config: Some(effective_config),
            stored_messages: initial_messages,
            last_prompt: Some(prompt.clone()),
            queued_prompts: VecDeque::new(),
            thread_handle: None,
            handle: None,
            notify,
        };
        state.children.insert(id.clone(), entry);
        drop(state);

        self.launch_child(
            id.as_str(),
            prompt,
            child_max_turns,
            model_override,
            reasoning_override,
        )
        .await?;
        self.status_for(&id).await
    }

    async fn restart_child(&self, target: &str) -> Result<()> {
        let (prompt, max_turns) = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(target)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", target))?;
            let prompt = record
                .queued_prompts
                .pop_front()
                .or_else(|| record.last_prompt.clone());
            let prompt =
                prompt.ok_or_else(|| anyhow!("Subagent {} has no queued input", target))?;
            (prompt, record.spec.max_turns)
        };
        self.launch_child(target, prompt, max_turns, None, None)
            .await
    }

    async fn launch_child(
        &self,
        child_id: &str,
        prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<()> {
        let controller = self.clone();
        let target = child_id.to_string();
        let handle = tokio::spawn(async move {
            controller
                .child_loop(
                    &target,
                    prompt,
                    max_turns,
                    model_override,
                    reasoning_override,
                )
                .await;
        });
        let mut state = self.state.write().await;
        let record = state
            .children
            .get_mut(child_id)
            .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
        record.handle = Some(handle);
        record.status = SubagentStatus::Queued;
        record.updated_at = Utc::now();
        Ok(())
    }

    async fn child_loop(
        &self,
        child_id: &str,
        mut prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) {
        loop {
            let execute = self
                .run_child_once(
                    child_id,
                    prompt.clone(),
                    max_turns,
                    model_override.clone(),
                    reasoning_override.clone(),
                )
                .await;

            let next_prompt = {
                let mut state = self.state.write().await;
                let Some(record) = state.children.get_mut(child_id) else {
                    return;
                };
                record.updated_at = Utc::now();

                match execute {
                    Ok(result) => {
                        record.status = if result.outcome.is_success() {
                            SubagentStatus::Completed
                        } else {
                            SubagentStatus::Failed
                        };
                        record.summary = Some(result.summary.clone());
                        record.error = match result.outcome {
                            TaskOutcome::Failed { reason } => Some(reason),
                            _ => None,
                        };
                        record.transcript_path = result.transcript_path.clone();
                        record.stored_messages = result.messages;
                    }
                    Err(error) => {
                        record.status = SubagentStatus::Failed;
                        record.summary = None;
                        record.error = Some(error.to_string());
                    }
                }

                let next_prompt = record.queued_prompts.pop_front();
                let hook_payload = if next_prompt.is_none() {
                    Some((
                        record.parent_thread_id.clone(),
                        record.session_id.clone(),
                        record.spec.name.clone(),
                        record.display_label.clone(),
                        record.background,
                        record.status.as_str().to_string(),
                        record
                            .transcript_path
                            .clone()
                            .or_else(|| record.archive_path.clone()),
                    ))
                } else {
                    None
                };
                if next_prompt.is_some() {
                    record.status = SubagentStatus::Queued;
                    record.completed_at = None;
                } else if record.status.is_terminal() {
                    record.completed_at = Some(Utc::now());
                }
                record.notify.notify_waiters();
                (next_prompt, hook_payload)
            };

            if let Some((
                parent_session_id,
                child_thread_id,
                agent_name,
                display_label,
                background,
                status,
                transcript_path,
            )) = next_prompt.1
                && let Some(hooks) = self.lifecycle_hooks.as_ref()
                && let Err(err) = hooks
                    .run_subagent_stop(
                        &parent_session_id,
                        &child_thread_id,
                        &agent_name,
                        &display_label,
                        background,
                        &status,
                        transcript_path.as_deref(),
                    )
                    .await
            {
                tracing::warn!(
                    child_id,
                    error = %err,
                    "Failed to run subagent stop hooks"
                );
            }

            if let Some(next_prompt) = next_prompt.0 {
                prompt = next_prompt;
                continue;
            } else {
                let mut state = self.state.write().await;
                if let Some(record) = state.children.get_mut(child_id) {
                    record.handle = None;
                    record.updated_at = Utc::now();
                }
                return;
            }
        }
    }

    async fn run_child_once(
        &self,
        child_id: &str,
        prompt: String,
        max_turns: Option<usize>,
        model_override: Option<String>,
        reasoning_override: Option<String>,
    ) -> Result<ChildRunResult> {
        let (spec, session_id, bootstrap_messages, display_label, background) = {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
            record.status = SubagentStatus::Running;
            record.updated_at = Utc::now();
            (
                record.spec.clone(),
                record.session_id.clone(),
                record.stored_messages.clone(),
                record.display_label.clone(),
                record.background,
            )
        };

        let (resolved_model, child_reasoning_effort, child_cfg) = prepare_child_runtime_config(
            &self.config.vt_cfg,
            &spec,
            self.config.parent_model.as_str(),
            self.config.parent_provider.as_str(),
            self.config.parent_reasoning_effort,
            max_turns,
            model_override.as_deref(),
            reasoning_override.as_deref(),
        )?;
        let parent_session_id = self.parent_session_id.read().await.clone();

        let archive_metadata = build_subagent_archive_metadata(
            &self.config.workspace_root,
            child_cfg.agent.default_model.as_str(),
            child_cfg.agent.provider.as_str(),
            child_cfg.agent.theme.as_str(),
            child_reasoning_effort.as_str(),
            parent_session_id.as_str(),
            !bootstrap_messages.is_empty(),
        );
        let bootstrap = ThreadBootstrap::new(Some(archive_metadata.clone()))
            .with_messages(bootstrap_messages.clone());
        let archive = if let Some(listing) = find_session_by_identifier(&session_id).await? {
            SessionArchive::resume_from_listing(&listing, archive_metadata.clone())
        } else {
            SessionArchive::new_with_identifier(archive_metadata.clone(), session_id.clone())
                .await?
        };
        checkpoint_subagent_archive_start(&archive, &bootstrap_messages).await?;
        let mut runner = AgentRunner::new_with_thread_bootstrap_and_config_with_openai_auth(
            agent_type_for_spec(&spec),
            resolved_model,
            self.config.api_key.clone(),
            self.config.workspace_root.clone(),
            session_id.clone(),
            RunnerSettings {
                reasoning_effort: Some(child_reasoning_effort),
                verbosity: None,
            },
            None,
            bootstrap,
            child_cfg.clone(),
            self.config.openai_chatgpt_auth.clone(),
        )
        .await?;
        runner.set_quiet(true);
        let thread_handle = runner.thread_handle();
        let archive_path = archive.path().to_path_buf();

        {
            let mut state = self.state.write().await;
            let record = state
                .children
                .get_mut(child_id)
                .ok_or_else(|| anyhow!("Unknown subagent id {}", child_id))?;
            record.archive_metadata = Some(archive_metadata.clone());
            record.archive_path = Some(archive_path.clone());
            record.effective_config = Some(child_cfg.clone());
            record.thread_handle = Some(thread_handle.clone());
        }
        if let Some(hooks) = self.lifecycle_hooks.as_ref()
            && let Err(err) = hooks
                .run_subagent_start(
                    parent_session_id.as_str(),
                    thread_handle.thread_id().as_str(),
                    spec.name.as_str(),
                    &display_label,
                    background,
                    SubagentStatus::Running.as_str(),
                    Some(archive_path.as_path()),
                )
                .await
        {
            tracing::warn!(
                child_id,
                error = %err,
                "Failed to run subagent start hooks"
            );
        }

        let filtered_tools = filter_child_tools(
            &spec,
            runner.build_universal_tools().await?,
            spec.is_read_only(),
        );
        let allowed_tools = filtered_tools
            .iter()
            .map(|tool| tool.function_name().to_string())
            .collect::<Vec<_>>();
        runner.set_tool_definitions_override(filtered_tools);
        runner.enable_full_auto(&allowed_tools).await;

        let memory_appendix =
            load_memory_appendix(&self.config.workspace_root, spec.name.as_str(), spec.memory)?;
        let mut task = Task::new(
            format!("subagent-{}", spec.name),
            format!("Subagent {}", spec.name),
            prompt,
        );
        task.instructions = Some(compose_subagent_instructions(&spec, memory_appendix));

        let results = runner.execute_task(&task, &[]).await?;
        let messages = runner.session_messages();
        let transcript_path =
            persist_child_archive(&archive, &messages, spec.name.as_str()).await?;

        Ok(ChildRunResult {
            messages,
            summary: if results.summary.trim().is_empty() {
                results.outcome.description()
            } else {
                results.summary.clone()
            },
            outcome: results.outcome,
            transcript_path,
        })
    }
}

async fn discover_controller_subagents(workspace_root: &Path) -> Result<DiscoveredSubagents> {
    let plugin_agent_files = discover_plugin_agent_files(workspace_root).await?;
    discover_subagents(&SubagentDiscoveryInput {
        workspace_root: workspace_root.to_path_buf(),
        cli_agents: None,
        plugin_agent_files,
    })
}

#[derive(Debug)]
struct ChildRunResult {
    messages: Vec<Message>,
    summary: String,
    outcome: TaskOutcome,
    transcript_path: Option<PathBuf>,
}

async fn checkpoint_subagent_archive_start(
    archive: &SessionArchive,
    messages: &[Message],
) -> Result<()> {
    let recent_messages = messages
        .iter()
        .map(SessionMessage::from)
        .collect::<Vec<_>>();
    archive
        .persist_progress_async(crate::utils::session_archive::SessionProgressArgs {
            total_messages: recent_messages.len(),
            distinct_tools: Vec::new(),
            recent_messages,
            turn_number: 1,
            token_usage: None,
            max_context_tokens: None,
            loaded_skills: Some(Vec::new()),
        })
        .await?;
    Ok(())
}

fn build_child_config(
    parent: &VTCodeConfig,
    spec: &SubagentSpec,
    model: &str,
    max_turns: Option<usize>,
) -> VTCodeConfig {
    let mut child = parent.clone();
    child.agent.default_model = model.to_string();
    if let Some(mode) = spec.permission_mode {
        child.permissions.default_mode =
            clamp_permission_mode(parent.permissions.default_mode, mode);
    }
    if let Some(max_turns) = normalize_child_max_turns(max_turns) {
        child.automation.full_auto.max_turns = max_turns;
    }

    let mut allowed_tools = spec.tools.clone().unwrap_or_default();
    if !allowed_tools.is_empty() {
        allowed_tools.retain(|tool| !SUBAGENT_TOOL_NAMES.iter().any(|blocked| blocked == tool));
        child.permissions.allowed_tools =
            intersect_allowed_tools(&parent.permissions.allowed_tools, &allowed_tools);
    }

    let mut disallowed_tools = parent.permissions.disallowed_tools.clone();
    disallowed_tools.extend(spec.disallowed_tools.clone());
    for tool in SUBAGENT_TOOL_NAMES {
        if !disallowed_tools.iter().any(|entry| entry == tool) {
            disallowed_tools.push((*tool).to_string());
        }
    }
    child.permissions.disallowed_tools = disallowed_tools;
    merge_child_hooks(&mut child, spec.hooks.as_ref());
    merge_child_mcp_servers(&mut child, spec.mcp_servers.as_slice());
    child
}

fn normalize_child_max_turns(max_turns: Option<usize>) -> Option<usize> {
    max_turns.map(|value| value.max(SUBAGENT_MIN_MAX_TURNS))
}

fn prepare_child_runtime_config(
    parent: &VTCodeConfig,
    spec: &SubagentSpec,
    parent_model: &str,
    parent_provider: &str,
    parent_reasoning_effort: ReasoningEffortLevel,
    max_turns: Option<usize>,
    model_override: Option<&str>,
    reasoning_override: Option<&str>,
) -> Result<(ModelId, ReasoningEffortLevel, VTCodeConfig)> {
    let resolved_model = resolve_effective_subagent_model(
        parent,
        parent_model,
        parent_provider,
        model_override,
        spec.model.as_deref(),
        spec.name.as_str(),
    )?;
    let mut child_cfg = build_child_config(parent, spec, resolved_model.as_str(), max_turns);
    let child_reasoning_effort = reasoning_override
        .and_then(ReasoningEffortLevel::parse)
        .or_else(|| {
            spec.reasoning_effort
                .as_deref()
                .and_then(ReasoningEffortLevel::parse)
        })
        .unwrap_or(parent_reasoning_effort);
    child_cfg.agent.default_model = resolved_model.to_string();
    child_cfg.agent.reasoning_effort = child_reasoning_effort;
    Ok((resolved_model, child_reasoning_effort, child_cfg))
}

fn clamp_permission_mode(parent: PermissionMode, requested: PermissionMode) -> PermissionMode {
    if matches!(
        parent,
        PermissionMode::Auto | PermissionMode::BypassPermissions
    ) {
        return parent;
    }

    if permission_rank(requested) <= permission_rank(parent) {
        requested
    } else {
        parent
    }
}

fn permission_rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::DontAsk => 0,
        PermissionMode::Plan => 1,
        PermissionMode::Default => 2,
        PermissionMode::AcceptEdits => 3,
        PermissionMode::Auto => 4,
        PermissionMode::BypassPermissions => 5,
    }
}

fn intersect_allowed_tools(parent_allowed: &[String], spec_allowed: &[String]) -> Vec<String> {
    if parent_allowed.is_empty() {
        return spec_allowed.to_vec();
    }

    spec_allowed
        .iter()
        .filter(|candidate| parent_allowed.iter().any(|allowed| allowed == *candidate))
        .cloned()
        .collect()
}

fn merge_child_hooks(child: &mut VTCodeConfig, hooks: Option<&HooksConfig>) {
    let Some(hooks) = hooks else {
        return;
    };

    child.hooks.lifecycle.quiet_success_output |= hooks.lifecycle.quiet_success_output;
    child
        .hooks
        .lifecycle
        .session_start
        .extend(hooks.lifecycle.session_start.clone());
    child
        .hooks
        .lifecycle
        .session_end
        .extend(hooks.lifecycle.session_end.clone());
    child
        .hooks
        .lifecycle
        .user_prompt_submit
        .extend(hooks.lifecycle.user_prompt_submit.clone());
    child
        .hooks
        .lifecycle
        .pre_tool_use
        .extend(hooks.lifecycle.pre_tool_use.clone());
    child
        .hooks
        .lifecycle
        .post_tool_use
        .extend(hooks.lifecycle.post_tool_use.clone());
    child
        .hooks
        .lifecycle
        .pre_compact
        .extend(hooks.lifecycle.pre_compact.clone());
    child
        .hooks
        .lifecycle
        .task_completion
        .extend(hooks.lifecycle.task_completion.clone());
    child
        .hooks
        .lifecycle
        .task_completed
        .extend(hooks.lifecycle.task_completed.clone());
    child
        .hooks
        .lifecycle
        .notification
        .extend(hooks.lifecycle.notification.clone());
}

fn merge_child_mcp_servers(child: &mut VTCodeConfig, servers: &[SubagentMcpServer]) {
    for server in servers {
        match server {
            SubagentMcpServer::Named(name) => {
                if child
                    .mcp
                    .providers
                    .iter()
                    .any(|provider| provider.name == *name)
                {
                    continue;
                }
            }
            SubagentMcpServer::Inline(definition) => {
                for (name, value) in definition {
                    let provider = inline_mcp_provider(name, value);
                    if let Some(provider) = provider {
                        child
                            .mcp
                            .providers
                            .retain(|existing| existing.name != provider.name);
                        child.mcp.providers.push(provider);
                    }
                }
            }
        }
    }
}

fn inline_mcp_provider(name: &str, value: &serde_json::Value) -> Option<McpProviderConfig> {
    let object = value.as_object()?;
    let mut payload = serde_json::Map::with_capacity(object.len().saturating_add(1));
    payload.insert(
        "name".to_string(),
        serde_json::Value::String(name.to_string()),
    );
    for (key, value) in object {
        payload.insert(key.clone(), value.clone());
    }
    serde_json::from_value(serde_json::Value::Object(payload)).ok()
}

fn compose_subagent_instructions(spec: &SubagentSpec, memory_appendix: Option<String>) -> String {
    let mut sections = Vec::new();
    if !spec.prompt.trim().is_empty() {
        sections.push(spec.prompt.trim().to_string());
    }
    if spec.is_read_only() {
        sections.push(
            "Tool reminder: stay inside the exposed read-only tool set for this child. Do not guess hidden or legacy helpers such as `list_files`, `read_file`, `unified_file`, or `unified_exec` when they are not visible. For workspace discovery here, prefer `unified_search`; if that is insufficient, report the blocker instead of retrying denied calls.".to_string(),
        );
        sections.push(
            "This delegated agent already runs with a read-only tool surface. Do not try to enter or exit plan mode, do not call hidden mutating tools, and do not retry the same denied tool call; adjust strategy or report the blocker instead.".to_string(),
        );
    } else {
        sections.push(
            "Tool reminder: `list_files` on the workspace root (`.`) is blocked, and `list_files` already uses search internally. Do not pair `list_files` with `unified_search` in the same batch. Use a specific subdirectory, `unified_search` for workspace-wide discovery, or `unified_exec` with `git diff --name-only` / `git diff --stat` when reviewing current changes.".to_string(),
        );
    }
    if !spec.skills.is_empty() {
        sections.push(format!(
            "Preloaded skill names: {}. Use their established repository conventions.",
            spec.skills.join(", ")
        ));
    }
    if let Some(memory_appendix) = memory_appendix
        && !memory_appendix.trim().is_empty()
    {
        sections.push(memory_appendix);
    }
    sections.join("\n\n")
}

fn build_subagent_archive_metadata(
    workspace_root: &Path,
    model: &str,
    provider: &str,
    theme: &str,
    reasoning_effort: &str,
    parent_session_id: &str,
    forked: bool,
) -> SessionArchiveMetadata {
    build_thread_archive_metadata(workspace_root, model, provider, theme, reasoning_effort)
        .with_parent_session_id(parent_session_id.to_string())
        .with_fork_mode(if forked {
            SessionForkMode::FullCopy
        } else {
            SessionForkMode::Summarized
        })
}

async fn persist_child_archive(
    archive: &SessionArchive,
    messages: &[Message],
    agent_name: &str,
) -> Result<Option<PathBuf>> {
    let transcript = messages
        .iter()
        .filter_map(transcript_line_from_message)
        .take(SUBAGENT_TRANSCRIPT_LINE_LIMIT)
        .collect::<Vec<_>>();
    let stored_messages = messages
        .iter()
        .map(SessionMessage::from)
        .collect::<Vec<_>>();
    let path = archive.finalize(
        transcript,
        stored_messages.len(),
        vec![agent_name.to_string()],
        stored_messages,
    )?;
    Ok(Some(path))
}

fn subagent_display_label(spec: &SubagentSpec) -> String {
    spec.nickname_candidates
        .first()
        .cloned()
        .unwrap_or_else(|| spec.name.clone())
}

fn extract_explicit_agent_mentions(input: &str, specs: &[SubagentSpec]) -> Vec<String> {
    let mut mentions = Vec::new();
    for direct in extract_direct_agent_mentions(input) {
        let canonical = specs
            .iter()
            .find(|spec| spec.matches_name(direct.as_str()))
            .map(|spec| spec.name.clone())
            .unwrap_or(direct);
        push_unique_agent_mention(&mut mentions, canonical);
    }

    let lower = input.to_ascii_lowercase();
    for spec in specs {
        if !matches_explicit_named_agent_selection(lower.as_str(), spec) {
            continue;
        }
        push_unique_agent_mention(&mut mentions, spec.name.clone());
    }

    mentions
}

fn extract_direct_agent_mentions(input: &str) -> Vec<String> {
    input
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|ch: char| {
                matches!(
                    ch,
                    '"' | '\'' | ',' | '.' | ':' | ';' | '!' | '?' | ')' | '('
                )
            });
            trimmed
                .strip_prefix("@agent-")
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect()
}

fn push_unique_agent_mention(mentions: &mut Vec<String>, candidate: String) {
    if mentions
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(candidate.as_str()))
    {
        return;
    }
    mentions.push(candidate);
}

fn matches_explicit_named_agent_selection(input: &str, spec: &SubagentSpec) -> bool {
    std::iter::once(spec.name.as_str())
        .chain(spec.aliases.iter().map(String::as_str))
        .any(|candidate| contains_explicit_named_agent_selection(input, candidate))
}

fn contains_explicit_named_agent_selection(input: &str, candidate: &str) -> bool {
    let candidate = candidate.trim().to_ascii_lowercase();
    if candidate.is_empty() {
        return false;
    }

    let direct_match = [
        format!("use {candidate} agent"),
        format!("use the {candidate} agent"),
        format!("use {candidate} subagent"),
        format!("use the {candidate} subagent"),
        format!("run {candidate} agent"),
        format!("run the {candidate} agent"),
        format!("run {candidate} subagent"),
        format!("run the {candidate} subagent"),
        format!("delegate to {candidate}"),
        format!("delegate this to {candidate}"),
        format!("delegate the task to {candidate}"),
        format!("spawn {candidate}"),
        format!("spawn the {candidate}"),
        format!("ask {candidate} to"),
    ]
    .iter()
    .any(|pattern| input.contains(pattern.as_str()));
    if direct_match {
        return true;
    }

    [
        format!("use {candidate} and"),
        format!("use the {candidate} and"),
    ]
    .iter()
    .any(|pattern| input.contains(pattern.as_str()))
        && (input.contains(" agent") || input.contains(" subagent"))
}

fn contains_explicit_delegation_request(input: &str, explicit_mentions: &[String]) -> bool {
    let lower = input.to_ascii_lowercase();
    !explicit_mentions.is_empty()
        || lower.contains(" run in parallel")
        || lower.contains(" spawn ")
        || lower.starts_with("spawn ")
        || lower.contains(" delegate ")
        || lower.starts_with("delegate ")
        || lower.contains(" background subagent")
        || lower.contains(" background agent")
        || (lower.contains(" use the ")
            && (lower.contains(" agent") || lower.contains(" subagent")))
        || (lower.starts_with("use ") && (lower.contains(" agent") || lower.contains(" subagent")))
}

fn contains_explicit_model_request(input: &str, requested_model: &str) -> bool {
    let requested = requested_model.trim();
    if requested.is_empty() {
        return false;
    }

    let lower_input = input.to_ascii_lowercase();
    let lower_requested = requested.to_ascii_lowercase();

    match lower_requested.as_str() {
        "small" => {
            lower_input.contains("small model")
                || lower_input.contains("smaller model")
                || lower_input.contains("lightweight model")
                || lower_input.contains("cheap model")
        }
        "haiku" | "sonnet" | "opus" | "inherit" => {
            contains_bounded_term(&lower_input, &lower_requested)
                || lower_input.contains(&format!("use {lower_requested}"))
                || lower_input.contains(&format!("using {lower_requested}"))
                || lower_input.contains(&format!("with {lower_requested}"))
                || lower_input.contains(&format!("run on {lower_requested}"))
                || lower_input.contains(&format!("{lower_requested} model"))
                || lower_input.contains(&format!("model {lower_requested}"))
        }
        _ => contains_bounded_term(&lower_input, &lower_requested),
    }
}

fn normalize_requested_model_override(raw: Option<String>, current_input: &str) -> Option<String> {
    let requested = raw?.trim().to_string();
    if requested.is_empty() || requested.eq_ignore_ascii_case("default") {
        return None;
    }
    if requested.eq_ignore_ascii_case("inherit")
        && !contains_explicit_model_request(current_input, requested.as_str())
    {
        return None;
    }
    Some(requested)
}

fn sanitize_subagent_input_items(items: &mut Vec<SubagentInputItem>) {
    let mut sanitized = Vec::with_capacity(items.len());
    for mut item in items.drain(..) {
        item.item_type = trim_optional_field(item.item_type.take());
        item.text = trim_optional_field(item.text.take());
        item.path = trim_optional_field(item.path.take());
        item.name = trim_optional_field(item.name.take());
        item.image_url = trim_optional_field(item.image_url.take());
        if item.text.is_none()
            && item.path.is_none()
            && item.name.is_none()
            && item.image_url.is_none()
        {
            continue;
        }
        sanitized.push(item);
    }
    *items = sanitized;
}

fn trim_optional_field(value: Option<String>) -> Option<String> {
    let trimmed = value?.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn contains_bounded_term(input: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }

    input.match_indices(needle).any(|(start, matched)| {
        let end = start + matched.len();
        let leading_ok = start == 0
            || !input[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_ascii_alphanumeric());
        let trailing_ok = end == input.len()
            || !input[end..]
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_alphanumeric());
        leading_ok && trailing_ok
    })
}

fn load_session_listing(path: &Path) -> Result<SessionListing> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read session archive {}", path.display()))?;
    let snapshot: SessionSnapshot = serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse session archive {}", path.display()))?;
    Ok(SessionListing {
        path: path.to_path_buf(),
        snapshot,
    })
}

fn transcript_line_from_message(message: &Message) -> Option<String> {
    let role = format!("{:?}", message.role).to_lowercase();
    let content = message.content.trim();
    if content.is_empty() {
        return None;
    }
    Some(format!("{role}: {content}"))
}

fn filter_child_tools(
    spec: &SubagentSpec,
    definitions: Vec<ToolDefinition>,
    read_only: bool,
) -> Vec<ToolDefinition> {
    let allowed = spec.tools.as_ref().map(|tools| {
        tools
            .iter()
            .map(|tool| tool.to_ascii_lowercase())
            .collect::<Vec<_>>()
    });
    let denied = spec
        .disallowed_tools
        .iter()
        .map(|tool| tool.to_ascii_lowercase())
        .collect::<Vec<_>>();

    definitions
        .into_iter()
        .filter(|tool| {
            let name = tool.function_name().to_ascii_lowercase();
            if SUBAGENT_TOOL_NAMES.iter().any(|blocked| *blocked == name) {
                return false;
            }
            if denied.iter().any(|entry| entry == &name) {
                return false;
            }
            if let Some(allowed) = allowed.as_ref()
                && !allowed.iter().any(|entry| entry == &name)
            {
                return false;
            }
            if read_only {
                return NON_MUTATING_TOOL_PREFIXES
                    .iter()
                    .any(|candidate| *candidate == name);
            }
            true
        })
        .collect()
}

fn request_prompt(message: &Option<String>, items: &[SubagentInputItem]) -> Option<String> {
    if let Some(message) = message
        && !message.trim().is_empty()
    {
        return Some(message.trim().to_string());
    }

    let segments = items
        .iter()
        .filter_map(item_prompt_segment)
        .collect::<Vec<_>>();
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n"))
    }
}

fn item_prompt_segment(item: &SubagentInputItem) -> Option<String> {
    if let Some(text) = item.text.as_ref()
        && !text.trim().is_empty()
    {
        return Some(text.trim().to_string());
    }
    if let Some(path) = item.path.as_ref()
        && !path.trim().is_empty()
    {
        return Some(format!("Reference: {}", path.trim()));
    }
    if let Some(name) = item.name.as_ref()
        && !name.trim().is_empty()
    {
        return Some(name.trim().to_string());
    }
    if let Some(image_url) = item.image_url.as_ref()
        && !image_url.trim().is_empty()
    {
        return Some(format!("Image: {}", image_url.trim()));
    }
    None
}

fn resolve_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    requested: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    let requested = requested.unwrap_or("inherit").trim();
    if requested.eq_ignore_ascii_case("inherit") || requested.is_empty() {
        if let Ok(model) = parent_model.parse::<ModelId>() {
            return Ok(model);
        }
        if parent_provider.eq_ignore_ascii_case("copilot") {
            let fallback = ModelId::default_orchestrator_for_provider(Provider::Copilot);
            tracing::warn!(
                agent_name,
                parent_model = parent_model.trim(),
                parent_provider = parent_provider.trim(),
                fallback_model = fallback.as_str(),
                "Falling back to the default Copilot subagent model because the inherited parent model identifier is not supported internally"
            );
            return Ok(fallback);
        }

        let normalized_parent_model = normalize_subagent_model_alias(parent_model);
        return normalized_parent_model.parse::<ModelId>().with_context(|| {
            format!(
                "Failed to resolve model '{}' for subagent {}",
                normalized_parent_model, agent_name
            )
        });
    }

    let resolved = if requested.eq_ignore_ascii_case("small") {
        if !vt_cfg.agent.small_model.model.trim().is_empty() {
            let configured = vt_cfg.agent.small_model.model.trim();
            let active_provider = infer_provider(Some(parent_provider), parent_model);
            let configured_provider =
                infer_provider_from_model(configured).or_else(|| infer_provider(None, configured));
            if configured_provider.is_some() && configured_provider != active_provider {
                tracing::warn!(
                    agent_name,
                    configured_model = configured,
                    active_provider = active_provider
                        .map(|provider| provider.to_string())
                        .unwrap_or_else(|| parent_provider.to_string()),
                    "Ignoring cross-provider lightweight subagent model; using same-provider automatic route"
                );
                auto_lightweight_model(parent_provider, parent_model)
            } else {
                configured.to_string()
            }
        } else {
            auto_lightweight_model(parent_provider, parent_model)
        }
    } else if requested.eq_ignore_ascii_case("haiku")
        || requested.eq_ignore_ascii_case("sonnet")
        || requested.eq_ignore_ascii_case("opus")
    {
        alias_model_for_provider(parent_provider, requested, parent_model)
    } else {
        requested.to_string()
    };

    let normalized_resolved = normalize_subagent_model_alias(resolved.as_str());
    normalized_resolved.parse::<ModelId>().with_context(|| {
        format!(
            "Failed to resolve model '{}' for subagent {}",
            normalized_resolved, agent_name
        )
    })
}

fn normalize_subagent_model_alias(model: &str) -> Cow<'_, str> {
    match model.trim() {
        "claude-haiku-4.5" => Cow::Borrowed(models::anthropic::CLAUDE_HAIKU_4_5),
        "claude-sonnet-4.6" => Cow::Borrowed(models::anthropic::CLAUDE_SONNET_4_6),
        "claude-opus-4.6" => Cow::Borrowed(models::anthropic::CLAUDE_OPUS_4_6),
        other => Cow::Borrowed(other),
    }
}

fn resolve_effective_subagent_model(
    vt_cfg: &VTCodeConfig,
    parent_model: &str,
    parent_provider: &str,
    model_override: Option<&str>,
    spec_model: Option<&str>,
    agent_name: &str,
) -> Result<ModelId> {
    if let Some(requested_model) = model_override {
        match resolve_subagent_model(
            vt_cfg,
            parent_model,
            parent_provider,
            Some(requested_model),
            agent_name,
        ) {
            Ok(model) => return Ok(model),
            Err(err) => {
                if requested_model.trim().eq_ignore_ascii_case("small") {
                    tracing::warn!(
                        agent_name,
                        requested_model = requested_model.trim(),
                        error = %err,
                        "Failed to bootstrap lightweight subagent model; falling back to parent model"
                    );
                    return resolve_subagent_model(
                        vt_cfg,
                        parent_model,
                        parent_provider,
                        Some("inherit"),
                        agent_name,
                    );
                }
                let fallback_model = spec_model
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("inherit");
                tracing::warn!(
                    agent_name,
                    requested_model = requested_model.trim(),
                    fallback_model,
                    error = %err,
                    "Failed to resolve subagent model override; falling back"
                );
            }
        }
    }

    match resolve_subagent_model(
        vt_cfg,
        parent_model,
        parent_provider,
        spec_model,
        agent_name,
    ) {
        Ok(model) => Ok(model),
        Err(err)
            if spec_model
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("small")) =>
        {
            tracing::warn!(
                agent_name,
                error = %err,
                "Failed to resolve lightweight subagent model from spec; falling back to parent model"
            );
            resolve_subagent_model(
                vt_cfg,
                parent_model,
                parent_provider,
                Some("inherit"),
                agent_name,
            )
        }
        Err(err) => Err(err),
    }
}

fn alias_model_for_provider(parent_provider: &str, alias: &str, parent_model: &str) -> String {
    match infer_provider(Some(parent_provider), parent_model) {
        Some(Provider::Anthropic) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::anthropic::CLAUDE_HAIKU_4_5.to_string(),
            "opus" => models::anthropic::CLAUDE_OPUS_4_6.to_string(),
            _ => models::anthropic::CLAUDE_SONNET_4_6.to_string(),
        },
        Some(Provider::OpenAI) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::openai::GPT_5_4_MINI.to_string(),
            "opus" => models::openai::GPT_5_4.to_string(),
            _ => models::openai::GPT_5_4.to_string(),
        },
        Some(Provider::Gemini) => match alias.to_ascii_lowercase().as_str() {
            "haiku" => models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
            _ => models::google::GEMINI_3_1_PRO_PREVIEW.to_string(),
        },
        _ => parent_model.to_string(),
    }
}

fn agent_type_for_spec(spec: &SubagentSpec) -> AgentType {
    match spec.name.as_str() {
        "explorer" | "explore" => AgentType::Explore,
        "plan" => AgentType::Plan,
        "worker" | "general" | "general-purpose" | "default" => AgentType::General,
        _ => AgentType::Custom(spec.name.clone()),
    }
}

fn load_memory_appendix(
    workspace_root: &Path,
    agent_name: &str,
    scope: Option<SubagentMemoryScope>,
) -> Result<Option<String>> {
    let Some(scope) = scope else {
        return Ok(None);
    };

    let memory_dir = match scope {
        SubagentMemoryScope::Project => {
            workspace_root.join(".vtcode/agent-memory").join(agent_name)
        }
        SubagentMemoryScope::Local => workspace_root
            .join(".vtcode/agent-memory-local")
            .join(agent_name),
        SubagentMemoryScope::User => dirs::home_dir()
            .unwrap_or_default()
            .join(".vtcode/agent-memory")
            .join(agent_name),
    };
    std::fs::create_dir_all(&memory_dir).with_context(|| {
        format!(
            "Failed to create subagent memory directory {}",
            memory_dir.display()
        )
    })?;
    let memory_file = memory_dir.join("MEMORY.md");
    if !memory_file.exists() {
        return Ok(Some(format!(
            "Persistent memory directory: {}. Update MEMORY.md with concise reusable notes when you discover stable repository conventions.",
            memory_dir.display()
        )));
    }

    let content = std::fs::read_to_string(&memory_file)
        .with_context(|| format!("Failed to read {}", memory_file.display()))?;
    let mut bytes = 0usize;
    let excerpt = content
        .lines()
        .take(SUBAGENT_MEMORY_LINE_LIMIT)
        .take_while(|line| {
            bytes = bytes.saturating_add(line.len() + 1);
            bytes <= SUBAGENT_MEMORY_BYTES_LIMIT
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(format!(
        "Persistent memory directory: {}.\nRead and maintain MEMORY.md for durable learnings.\n\nCurrent MEMORY.md excerpt:\n{}",
        memory_dir.display(),
        excerpt
    )))
}

async fn discover_plugin_agent_files(workspace_root: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    for plugin_root in trusted_plugin_roots(workspace_root) {
        if !plugin_root.exists() || !plugin_root.is_dir() {
            continue;
        }

        for entry in std::fs::read_dir(&plugin_root)
            .with_context(|| format!("Failed to read plugin directory {}", plugin_root.display()))?
        {
            let path = entry?.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join(".vtcode-plugin/plugin.json");
            if !manifest_path.exists() {
                continue;
            }

            let manifest: PluginManifest =
                serde_json::from_str(&std::fs::read_to_string(&manifest_path).with_context(
                    || format!("Failed to read plugin manifest {}", manifest_path.display()),
                )?)
                .with_context(|| {
                    format!(
                        "Failed to parse plugin manifest {}",
                        manifest_path.display()
                    )
                })?;
            for agent_path in AgentsHandler::process_agents(&path, manifest.agents.clone()).await? {
                files.push((manifest.name.clone(), agent_path));
            }
        }
    }
    Ok(files)
}

fn trusted_plugin_roots(workspace_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(codex_home) = std::env::var_os("CODEX_HOME").map(PathBuf::from) {
        roots.push(codex_home.join("plugins"));
    } else if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".vtcode/plugins"));
    }
    roots.push(workspace_root.join(".vtcode/plugins"));
    roots.push(workspace_root.join(".agents/plugins"));
    roots
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn background_record_id(agent_name: &str) -> String {
    format!("background-{}", sanitize_component(agent_name))
}

fn background_state_path(workspace_root: &Path) -> PathBuf {
    workspace_root
        .join(".vtcode")
        .join("state")
        .join("background_subagents.json")
}

fn load_background_state(workspace_root: &Path) -> Result<PersistedBackgroundState> {
    let path = background_state_path(workspace_root);
    if !path.exists() {
        return Ok(PersistedBackgroundState::default());
    }

    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
}

fn summarize_background_preview(output: &str) -> String {
    output
        .lines()
        .rev()
        .take(24)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_archive_preview(path: &Path) -> Result<String> {
    let listing = load_session_listing(path)?;
    Ok(listing
        .snapshot
        .transcript
        .into_iter()
        .rev()
        .take(24)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

fn exec_session_is_running(session: &crate::tools::types::VTCodeExecSession) -> bool {
    matches!(
        session.lifecycle_state,
        Some(crate::tools::types::VTCodeSessionLifecycleState::Running)
    )
}

fn build_background_subagent_command(
    workspace_root: &Path,
    agent_name: &str,
    parent_session_id: &str,
    session_id: &str,
    prompt: &str,
    max_turns: Option<usize>,
    model_override: Option<&str>,
    reasoning_override: Option<&str>,
) -> Result<Vec<String>> {
    let executable = std::env::current_exe().context("Failed to resolve current vtcode binary")?;
    let mut command = vec![
        executable.to_string_lossy().into_owned(),
        "background-subagent".to_string(),
        "--workspace".to_string(),
        workspace_root.to_string_lossy().into_owned(),
        "--agent-name".to_string(),
        agent_name.to_string(),
        "--parent-session-id".to_string(),
        parent_session_id.to_string(),
        "--session-id".to_string(),
        session_id.to_string(),
        "--prompt".to_string(),
        prompt.to_string(),
    ];

    if let Some(max_turns) = max_turns {
        command.push("--max-turns".to_string());
        command.push(max_turns.to_string());
    }
    if let Some(model_override) = model_override
        && !model_override.trim().is_empty()
    {
        command.push("--model-override".to_string());
        command.push(model_override.to_string());
    }
    if let Some(reasoning_override) = reasoning_override
        && !reasoning_override.trim().is_empty()
    {
        command.push("--reasoning-override".to_string());
        command.push(reasoning_override.to_string());
    }

    Ok(command)
}

pub fn is_subagent_tool(name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::{
        SUBAGENT_HARD_CONCURRENCY_LIMIT, SUBAGENT_MIN_MAX_TURNS, SpawnAgentRequest,
        SubagentController, SubagentControllerConfig, SubagentInputItem, SubagentStatus,
        background_record_id, build_background_subagent_command, build_child_config,
        contains_explicit_delegation_request, contains_explicit_model_request,
        delegated_task_requires_clarification, extract_explicit_agent_mentions, filter_child_tools,
        normalize_requested_model_override, request_prompt, resolve_effective_subagent_model,
        resolve_subagent_model, sanitize_subagent_input_items,
    };
    use crate::config::PermissionMode;
    use crate::config::VTCodeConfig;
    use crate::config::constants::models;
    use crate::config::constants::tools;
    use crate::config::models::{ModelId, Provider};
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::ToolDefinition;
    use crate::tools::exec_session::ExecSessionManager;
    use crate::tools::registry::PtySessionManager;
    use anyhow::{Result, anyhow};
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use vtcode_config::{SubagentMcpServer, SubagentSource, SubagentSpec};

    fn test_controller_config(
        workspace_root: std::path::PathBuf,
        vt_cfg: VTCodeConfig,
    ) -> SubagentControllerConfig {
        let pty_sessions = PtySessionManager::new(workspace_root.clone(), vt_cfg.pty.clone());
        let exec_sessions = ExecSessionManager::new(workspace_root.clone(), pty_sessions.clone());
        SubagentControllerConfig {
            workspace_root,
            parent_session_id: "parent-session".to_string(),
            parent_model: models::openai::GPT_5_4.to_string(),
            parent_provider: "openai".to_string(),
            parent_reasoning_effort: ReasoningEffortLevel::Medium,
            api_key: "test-key".to_string(),
            vt_cfg,
            openai_chatgpt_auth: None,
            depth: 0,
            exec_sessions,
            pty_manager: pty_sessions.manager().clone(),
        }
    }

    #[test]
    fn request_prompt_prefers_message() {
        let request = SpawnAgentRequest {
            message: Some("hello".to_string()),
            ..SpawnAgentRequest::default()
        };
        assert_eq!(
            request_prompt(&request.message, &request.items).as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn delegated_task_requires_clarification_for_vague_prompt() {
        assert!(delegated_task_requires_clarification("report"));
        assert!(delegated_task_requires_clarification("report findings"));
        assert!(!delegated_task_requires_clarification(
            "review current code changes"
        ));
    }

    #[test]
    fn resolve_subagent_model_maps_aliases() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("haiku"),
            "explorer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn resolve_subagent_model_defaults_to_parent_when_omitted() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(
            &cfg,
            models::ollama::GPT_OSS_120B_CLOUD,
            "ollama",
            None,
            "worker",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
    }

    #[test]
    fn resolve_subagent_model_accepts_dotted_claude_aliases_for_anthropic() {
        let cfg = VTCodeConfig::default();
        let resolved =
            resolve_subagent_model(&cfg, "claude-haiku-4.5", "anthropic", None, "worker")
                .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn resolve_subagent_model_falls_back_to_copilot_default_for_unsupported_inherit_model() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_subagent_model(&cfg, "claude-haiku-4.5", "copilot", None, "worker")
            .expect("resolve model");
        assert_eq!(
            resolved,
            ModelId::default_orchestrator_for_provider(Provider::Copilot)
        );
    }

    #[test]
    fn resolve_effective_subagent_model_uses_explicit_inherit_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("inherit"),
            Some("haiku"),
            "worker",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_SONNET_4_6);
    }

    #[test]
    fn resolve_effective_subagent_model_falls_back_to_parent_on_invalid_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::ollama::GPT_OSS_120B_CLOUD,
            "ollama",
            Some("gpt-5-mini"),
            None,
            "rust-engineer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::ollama::GPT_OSS_120B_CLOUD);
    }

    #[test]
    fn resolve_subagent_small_model_rejects_cross_provider_configured_lightweight_model() {
        let mut cfg = VTCodeConfig::default();
        cfg.agent.small_model.model = models::anthropic::CLAUDE_HAIKU_4_5.to_string();

        let resolved = resolve_subagent_model(
            &cfg,
            models::openai::GPT_5_4,
            "openai",
            Some("small"),
            "worker",
        )
        .expect("resolve model");

        assert_eq!(resolved, ModelId::GPT5Mini);
    }

    #[test]
    fn resolve_effective_subagent_model_falls_back_to_spec_model_on_invalid_override() {
        let cfg = VTCodeConfig::default();
        let resolved = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            Some("gpt-5"),
            Some("haiku"),
            "reviewer",
        )
        .expect("resolve model");
        assert_eq!(resolved.as_str(), models::anthropic::CLAUDE_HAIKU_4_5);
    }

    #[test]
    fn background_record_ids_are_stable_and_sanitized() {
        assert_eq!(
            background_record_id("Rust Engineer"),
            "background-Rust-Engineer"
        );
        assert_eq!(
            background_record_id("plugin:reviewer/default"),
            "background-plugin-reviewer-default"
        );
    }

    #[test]
    fn background_subagent_command_includes_expected_flags() {
        let workspace = std::env::current_dir().expect("workspace");
        let command = build_background_subagent_command(
            &workspace,
            "rust-engineer",
            "session-parent",
            "session-child",
            "Inspect the repo",
            Some(7),
            Some("gpt-5.4-mini"),
            Some("high"),
        )
        .expect("background command");

        assert!(command.len() >= 15);
        assert_eq!(command[1], "background-subagent");
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--agent-name", "rust-engineer"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--parent-session-id", "session-parent"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--session-id", "session-child"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--prompt", "Inspect the repo"])
        );
        assert!(command.windows(2).any(|pair| pair == ["--max-turns", "7"]));
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--model-override", "gpt-5.4-mini"])
        );
        assert!(
            command
                .windows(2)
                .any(|pair| pair == ["--reasoning-override", "high"])
        );
    }

    #[test]
    fn resolve_effective_subagent_model_still_errors_on_invalid_spec_model() {
        let cfg = VTCodeConfig::default();
        let err = resolve_effective_subagent_model(
            &cfg,
            models::anthropic::CLAUDE_SONNET_4_6,
            "anthropic",
            None,
            Some("gpt-5-mini"),
            "reviewer",
        )
        .expect_err("invalid spec model should fail");
        assert!(err.to_string().contains("Failed to resolve model"));
    }

    async fn wait_for_effective_model(
        controller: &SubagentController,
        target: &str,
    ) -> Result<String> {
        for _ in 0..50 {
            if let Ok(snapshot) = controller.snapshot_for_thread(target).await {
                return Ok(snapshot.effective_config.agent.default_model);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        Err(anyhow!(
            "Subagent {target} did not capture an effective runtime configuration in time"
        ))
    }

    fn read_only_test_spec(name: &str) -> SubagentSpec {
        SubagentSpec {
            name: name.to_string(),
            description: "test".to_string(),
            prompt: String::new(),
            tools: Some(vec![tools::READ_FILE.to_string()]),
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permission_mode: Some(PermissionMode::Plan),
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        }
    }

    #[test]
    fn filter_child_tools_removes_subagent_tools_in_children() {
        let defs = vec![
            ToolDefinition::function(
                tools::SPAWN_AGENT.to_string(),
                "Spawn".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::UNIFIED_SEARCH.to_string(),
                "Search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::LIST_FILES.to_string(),
                "List".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::REQUEST_USER_INPUT.to_string(),
                "Ask".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ];
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");
        let filtered = filter_child_tools(&spec, defs, true);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function_name(), tools::UNIFIED_SEARCH);
    }

    #[test]
    fn filter_child_tools_keeps_unified_exec_for_shell_capable_agents() {
        let defs = vec![
            ToolDefinition::function(
                tools::UNIFIED_EXEC.to_string(),
                "Exec".to_string(),
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::function(
                tools::UNIFIED_SEARCH.to_string(),
                "Search".to_string(),
                serde_json::json!({"type": "object"}),
            ),
        ];
        let spec = SubagentSpec {
            name: "shell-demo".to_string(),
            description: "test".to_string(),
            prompt: String::new(),
            tools: Some(vec![
                tools::UNIFIED_EXEC.to_string(),
                tools::UNIFIED_SEARCH.to_string(),
            ]),
            disallowed_tools: Vec::new(),
            model: None,
            color: None,
            reasoning_effort: None,
            permission_mode: None,
            skills: Vec::new(),
            mcp_servers: Vec::new(),
            hooks: None,
            background: false,
            max_turns: None,
            nickname_candidates: Vec::new(),
            initial_prompt: None,
            memory: None,
            isolation: None,
            aliases: Vec::new(),
            source: SubagentSource::Builtin,
            file_path: None,
            warnings: Vec::new(),
        };

        let filtered = filter_child_tools(&spec, defs, spec.is_read_only());
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].function_name(), tools::UNIFIED_EXEC);
        assert_eq!(filtered[1].function_name(), tools::UNIFIED_SEARCH);
    }

    #[test]
    fn build_child_config_clamps_permissions_and_intersects_allowed_tools() {
        let mut parent = VTCodeConfig::default();
        parent.permissions.default_mode = PermissionMode::Default;
        parent.permissions.allowed_tools = vec![
            tools::READ_FILE.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
        ];
        parent.permissions.disallowed_tools = vec![tools::UNIFIED_EXEC.to_string()];

        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");
        spec.permission_mode = Some(PermissionMode::BypassPermissions);
        spec.tools = Some(vec![
            tools::SPAWN_AGENT.to_string(),
            tools::UNIFIED_SEARCH.to_string(),
            tools::READ_FILE.to_string(),
        ]);

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
        assert_eq!(child.permissions.default_mode, PermissionMode::Default);
        assert_eq!(
            child.permissions.allowed_tools,
            vec![
                tools::UNIFIED_SEARCH.to_string(),
                tools::READ_FILE.to_string()
            ]
        );
        assert!(
            child
                .permissions
                .disallowed_tools
                .contains(&tools::UNIFIED_EXEC.to_string())
        );
        assert!(
            child
                .permissions
                .disallowed_tools
                .contains(&tools::SPAWN_AGENT.to_string())
        );
    }

    #[test]
    fn build_child_config_promotes_single_turn_budget_to_recovery_budget() {
        let parent = VTCodeConfig::default();
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "worker")
            .expect("worker");

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, Some(1));

        assert_eq!(child.automation.full_auto.max_turns, SUBAGENT_MIN_MAX_TURNS);
    }

    #[test]
    fn build_child_config_merges_inline_mcp_provider() {
        let parent = VTCodeConfig::default();
        let mut spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "default")
            .expect("default");
        spec.mcp_servers = vec![SubagentMcpServer::Inline(BTreeMap::from([(
            "playwright".to_string(),
            serde_json::json!({
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@playwright/mcp@latest"],
            }),
        )]))];

        let child = build_child_config(&parent, &spec, models::openai::GPT_5_4, None);
        let provider = child
            .mcp
            .providers
            .iter()
            .find(|provider| provider.name == "playwright")
            .expect("playwright provider");
        assert_eq!(provider.name, "playwright");
    }

    #[test]
    fn explicit_delegation_request_detects_mentions_and_keywords() {
        let direct_mentions = extract_explicit_agent_mentions("@agent-worker fix the issue", &[]);
        assert!(contains_explicit_delegation_request(
            "@agent-worker fix the issue",
            direct_mentions.as_slice()
        ));
        let no_mentions = extract_explicit_agent_mentions("delegate this in parallel", &[]);
        assert!(contains_explicit_delegation_request(
            "delegate this in parallel",
            no_mentions.as_slice()
        ));
        let empty_mentions = extract_explicit_agent_mentions("review the repository", &[]);
        assert!(!contains_explicit_delegation_request(
            "review the repository",
            empty_mentions.as_slice()
        ));
    }

    #[test]
    fn explicit_agent_mentions_detect_natural_language_selection() {
        let rust_engineer = read_only_test_spec("rust-engineer");
        assert_eq!(
            extract_explicit_agent_mentions(
                "use rust-engineer agent to review current code",
                &[rust_engineer]
            ),
            vec!["rust-engineer".to_string()]
        );
    }

    #[test]
    fn explicit_agent_mentions_detect_looser_subagent_selection() {
        let background_demo = read_only_test_spec("background-demo");
        assert_eq!(
            extract_explicit_agent_mentions(
                "use background-demo and run the subagent",
                &[background_demo]
            ),
            vec!["background-demo".to_string()]
        );
    }

    #[test]
    fn explicit_agent_mentions_detect_run_subagent_selection() {
        let rust_engineer = read_only_test_spec("rust-engineer");
        assert_eq!(
            extract_explicit_agent_mentions(
                "run rust-engineer subagent and review changes",
                &[rust_engineer]
            ),
            vec!["rust-engineer".to_string()]
        );
    }

    #[test]
    fn explicit_model_request_detects_aliases_and_full_ids() {
        assert!(contains_explicit_model_request(
            "delegate this using gpt-5.4-mini",
            "gpt-5.4-mini"
        ));
        assert!(contains_explicit_model_request(
            "use the worker subagent with haiku",
            "haiku"
        ));
        assert!(contains_explicit_model_request(
            "run this with the small model",
            "small"
        ));
        assert!(!contains_explicit_model_request(
            "delegate this small cleanup task",
            "small"
        ));
        assert!(!contains_explicit_model_request(
            "delegate this task",
            "gpt-5.4-mini"
        ));
    }

    #[test]
    fn normalize_requested_model_override_drops_default_like_values() {
        assert_eq!(
            normalize_requested_model_override(Some("default".to_string()), "delegate this task"),
            None
        );
        assert_eq!(
            normalize_requested_model_override(Some(" inherit ".to_string()), "delegate this task"),
            None
        );
        assert_eq!(
            normalize_requested_model_override(
                Some(" inherit ".to_string()),
                "delegate this task using inherit"
            ),
            Some("inherit".to_string())
        );
    }

    #[test]
    fn sanitize_subagent_input_items_drops_empty_fields() {
        let mut items = vec![
            SubagentInputItem {
                item_type: Some("text".to_string()),
                text: Some("  Workspace: /tmp/repo  ".to_string()),
                path: Some(String::new()),
                name: Some(" ".to_string()),
                image_url: None,
            },
            SubagentInputItem {
                item_type: Some("text".to_string()),
                text: Some("   ".to_string()),
                path: Some(String::new()),
                name: None,
                image_url: None,
            },
        ];

        sanitize_subagent_input_items(&mut items);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].text.as_deref(), Some("Workspace: /tmp/repo"));
        assert!(items[0].path.is_none());
        assert!(items[0].name.is_none());
    }

    #[tokio::test]
    async fn controller_exposes_builtin_specs() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        let specs = controller.effective_specs().await;
        assert!(specs.iter().any(|spec| spec.name == "explorer"));
        assert!(specs.iter().any(|spec| spec.name == "worker"));
    }

    #[tokio::test]
    async fn spawn_defaults_to_single_explicit_mention() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Inspect the codebase.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "explorer");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_defaults_to_single_natural_language_selection() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let mentions = controller
            .set_turn_delegation_hints_from_input("use explorer agent to inspect the codebase")
            .await;
        assert_eq!(mentions, vec!["explorer".to_string()]);

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Inspect the codebase.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "explorer");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_rejects_mismatched_explicit_mention() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("@agent-explorer inspect the codebase")
            .await;

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("mismatched mention should fail");

        assert!(
            err.to_string()
                .contains("user explicitly selected 'explorer'")
        );
    }

    #[tokio::test]
    async fn spawn_rejects_write_capable_agent_without_explicit_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement a change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("write-capable agent should require explicit request");

        assert!(
            err.to_string()
                .contains("cannot launch write-capable agent 'worker'")
        );
    }

    #[tokio::test]
    async fn spawn_rejects_vague_task_even_with_explicit_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("run worker subagent and report")
            .await;

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("report".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("vague task should require clarification");

        assert!(err.to_string().contains("too vague ('report')"));
    }

    #[tokio::test]
    async fn spawn_defaults_to_write_capable_run_subagent_selection() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        let mentions = controller
            .set_turn_delegation_hints_from_input("run worker subagent and implement the change")
            .await;
        assert_eq!(mentions, vec!["worker".to_string()]);

        let spawned = controller
            .spawn(SpawnAgentRequest {
                message: Some("Implement the change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        assert_eq!(spawned.agent_name, "worker");
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_rejects_read_only_agent_when_auto_delegate_is_disabled() {
        let temp = TempDir::new().expect("tempdir");
        let mut cfg = VTCodeConfig::default();
        cfg.subagents.auto_delegate_read_only = false;
        let controller =
            SubagentController::new(test_controller_config(temp.path().to_path_buf(), cfg))
                .await
                .expect("controller");

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("explorer".to_string()),
                message: Some("Inspect the repository.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("read-only agent should require explicit delegation");

        assert!(
            err.to_string()
                .contains("cannot proactively launch read-only agent 'explorer'")
        );
    }

    #[tokio::test]
    async fn spawn_ignores_model_override_without_explicit_user_model_request() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                model: Some(models::openai::GPT_5_4_MINI.to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let effective_model = wait_for_effective_model(&controller, &spawned.id)
            .await
            .expect("effective model");
        assert_eq!(effective_model, models::openai::GPT_5_4);
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_honors_model_override_when_user_explicitly_requests_it() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task using gpt-5.4-mini")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                model: Some(models::openai::GPT_5_4_MINI.to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let effective_model = wait_for_effective_model(&controller, &spawned.id)
            .await
            .expect("effective model");
        assert_eq!(effective_model, models::openai::GPT_5_4_MINI);
        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn spawn_captures_runtime_config_before_first_child_turn() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");

        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("worker".to_string()),
                message: Some("Implement the change.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");

        let snapshot = controller
            .snapshot_for_thread(&spawned.id)
            .await
            .expect("snapshot");

        assert_eq!(snapshot.id, spawned.id);
        assert!(
            !snapshot
                .effective_config
                .agent
                .default_model
                .trim()
                .is_empty()
        );

        controller.close(&spawned.id).await.expect("close");
    }

    #[tokio::test]
    async fn close_marks_child_closed() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;
        let spawned = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("default".to_string()),
                message: Some("Summarize the repository.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect("spawn");
        let closed = controller.close(&spawned.id).await.expect("close");
        assert_eq!(closed.status, SubagentStatus::Closed);
    }

    #[tokio::test]
    async fn spawn_rejects_fourth_active_subagent() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        controller
            .set_turn_delegation_hints_from_input("delegate this task")
            .await;

        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "explorer")
            .expect("explorer");

        {
            let mut state = controller.state.write().await;
            for idx in 0..SUBAGENT_HARD_CONCURRENCY_LIMIT {
                let id = format!("active-{idx}");
                state.children.insert(
                    id.clone(),
                    super::ChildRecord {
                        id: id.clone(),
                        session_id: format!("session-{id}"),
                        parent_thread_id: "parent-session".to_string(),
                        spec: spec.clone(),
                        display_label: super::subagent_display_label(&spec),
                        status: SubagentStatus::Running,
                        background: false,
                        depth: 1,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        completed_at: None,
                        summary: None,
                        error: None,
                        archive_metadata: None,
                        archive_path: None,
                        transcript_path: None,
                        effective_config: None,
                        stored_messages: Vec::new(),
                        last_prompt: Some("Inspect the codebase.".to_string()),
                        queued_prompts: VecDeque::new(),
                        thread_handle: None,
                        handle: None,
                        notify: Arc::new(Notify::new()),
                    },
                );
            }
        }

        let err = controller
            .spawn(SpawnAgentRequest {
                agent_type: Some("explorer".to_string()),
                message: Some("Inspect another codepath.".to_string()),
                ..SpawnAgentRequest::default()
            })
            .await
            .expect_err("fourth active subagent should be rejected");

        assert!(err.to_string().contains(&format!(
            "Subagent concurrency limit reached (max_concurrent={})",
            SUBAGENT_HARD_CONCURRENCY_LIMIT
        )));
    }

    #[tokio::test]
    async fn wait_returns_first_terminal_child() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(test_controller_config(
            temp.path().to_path_buf(),
            VTCodeConfig::default(),
        ))
        .await
        .expect("controller");
        let spec = vtcode_config::builtin_subagents()
            .into_iter()
            .find(|spec| spec.name == "default")
            .expect("default");

        {
            let mut state = controller.state.write().await;
            for id in ["first", "second"] {
                state.children.insert(
                    id.to_string(),
                    super::ChildRecord {
                        id: id.to_string(),
                        session_id: format!("session-{id}"),
                        parent_thread_id: "parent-session".to_string(),
                        spec: spec.clone(),
                        display_label: super::subagent_display_label(&spec),
                        status: SubagentStatus::Running,
                        background: false,
                        depth: 1,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        completed_at: None,
                        summary: None,
                        error: None,
                        archive_metadata: None,
                        archive_path: None,
                        transcript_path: None,
                        effective_config: None,
                        stored_messages: Vec::new(),
                        last_prompt: None,
                        queued_prompts: VecDeque::new(),
                        thread_handle: None,
                        handle: None,
                        notify: Arc::new(Notify::new()),
                    },
                );
            }
        }

        let controller_clone = controller.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let mut state = controller_clone.state.write().await;
            let record = state.children.get_mut("second").expect("second child");
            record.status = SubagentStatus::Completed;
            record.summary = Some("done".to_string());
            record.completed_at = Some(Utc::now());
            record.updated_at = Utc::now();
            record.notify.notify_waiters();
        });

        let result = controller
            .wait(&["first".to_string(), "second".to_string()], Some(500))
            .await
            .expect("wait result")
            .expect("terminal child");
        assert_eq!(result.id, "second");
        assert_eq!(result.status, SubagentStatus::Completed);
    }
}
