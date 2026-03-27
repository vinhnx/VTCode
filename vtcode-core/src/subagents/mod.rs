use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use futures::future::select_all;
use serde::{Deserialize, Serialize};
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
use crate::core::agent::task::{Task, TaskOutcome, TaskResults};
use crate::core::agent::types::AgentType;
use crate::core::threads::{ThreadBootstrap, build_thread_archive_metadata};
use crate::llm::factory::infer_provider;
use crate::llm::provider::{Message, ToolDefinition};
use crate::plugins::components::AgentsHandler;
use crate::plugins::manifest::PluginManifest;
use crate::utils::session_archive::{SessionArchive, SessionForkMode, SessionMessage};
use vtcode_config::auth::OpenAIChatGptAuthHandle;
use vtcode_config::{
    DiscoveredSubagents, HooksConfig, McpProviderConfig, PermissionMode, SubagentDiscoveryInput,
    SubagentMcpServer, SubagentMemoryScope, SubagentSpec, discover_subagents,
};

const SUBAGENT_TRANSCRIPT_LINE_LIMIT: usize = 200;
const SUBAGENT_MEMORY_BYTES_LIMIT: usize = 25 * 1024;
const SUBAGENT_MEMORY_LINE_LIMIT: usize = 200;

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
    tools::LIST_FILES,
    tools::LIST_SKILLS,
    tools::LOAD_SKILL,
    tools::LOAD_SKILL_RESOURCE,
    tools::REQUEST_USER_INPUT,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentStatusEntry {
    pub id: String,
    pub session_id: String,
    pub agent_name: String,
    pub description: String,
    pub source: String,
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

#[derive(Debug, Clone)]
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
}

#[derive(Debug)]
struct ChildRecord {
    id: String,
    session_id: String,
    spec: SubagentSpec,
    status: SubagentStatus,
    background: bool,
    depth: usize,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
    summary: Option<String>,
    error: Option<String>,
    transcript_path: Option<PathBuf>,
    stored_messages: Vec<Message>,
    last_prompt: Option<String>,
    queued_prompts: VecDeque<String>,
    handle: Option<JoinHandle<()>>,
    notify: Arc<Notify>,
}

impl ChildRecord {
    fn status_entry(&self) -> SubagentStatusEntry {
        SubagentStatusEntry {
            id: self.id.clone(),
            session_id: self.session_id.clone(),
            agent_name: self.spec.name.clone(),
            description: self.spec.description.clone(),
            source: self.spec.source.label(),
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

#[derive(Debug)]
struct ControllerState {
    discovered: DiscoveredSubagents,
    parent_messages: Vec<Message>,
    children: BTreeMap<String, ChildRecord>,
}

#[derive(Clone)]
pub struct SubagentController {
    config: Arc<SubagentControllerConfig>,
    parent_session_id: Arc<RwLock<String>>,
    state: Arc<RwLock<ControllerState>>,
}

impl SubagentController {
    pub async fn new(config: SubagentControllerConfig) -> Result<Self> {
        let discovered = discover_controller_subagents(&config.workspace_root).await?;
        Ok(Self {
            parent_session_id: Arc::new(RwLock::new(config.parent_session_id.clone())),
            config: Arc::new(config),
            state: Arc::new(RwLock::new(ControllerState {
                discovered,
                parent_messages: Vec::new(),
                children: BTreeMap::new(),
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

    pub async fn spawn(&self, request: SpawnAgentRequest) -> Result<SubagentStatusEntry> {
        let spec = self
            .resolve_requested_spec(request.agent_type.as_deref())
            .await?;
        let prompt = request_prompt(&request.message, &request.items)
            .or_else(|| spec.initial_prompt.clone())
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| anyhow!("spawn_agent requires a task message or items"))?;
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
        if active_count >= self.config.vt_cfg.subagents.max_concurrent {
            bail!(
                "Subagent concurrency limit reached (max_concurrent={})",
                self.config.vt_cfg.subagents.max_concurrent
            );
        }

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
            spec: spec.clone(),
            status: SubagentStatus::Queued,
            background: background || spec.background,
            depth: self.config.depth.saturating_add(1),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            summary: None,
            error: None,
            transcript_path: None,
            stored_messages: initial_messages,
            last_prompt: Some(prompt.clone()),
            queued_prompts: VecDeque::new(),
            handle: None,
            notify,
        };
        state.children.insert(id.clone(), entry);
        drop(state);

        self.launch_child(
            id.as_str(),
            prompt,
            max_turns.or(spec.max_turns),
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
                if next_prompt.is_some() {
                    record.status = SubagentStatus::Queued;
                    record.completed_at = None;
                } else if record.status.is_terminal() {
                    record.completed_at = Some(Utc::now());
                }
                record.notify.notify_waiters();
                next_prompt
            };

            if let Some(next_prompt) = next_prompt {
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
        let (spec, session_id, bootstrap_messages) = {
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
            )
        };

        let resolved_model = resolve_subagent_model(
            &self.config.vt_cfg,
            &self.config.parent_model,
            &self.config.parent_provider,
            model_override.as_deref().or(spec.model.as_deref()),
            spec.name.as_str(),
        )?;
        let mut child_cfg = build_child_config(
            &self.config.vt_cfg,
            &spec,
            resolved_model.as_str(),
            max_turns,
        );
        let child_reasoning_effort = reasoning_override
            .as_deref()
            .and_then(ReasoningEffortLevel::parse)
            .or_else(|| {
                spec.reasoning_effort
                    .as_deref()
                    .and_then(ReasoningEffortLevel::parse)
            })
            .unwrap_or(self.config.parent_reasoning_effort);
        child_cfg.agent.default_model = resolved_model.to_string();
        child_cfg.agent.reasoning_effort = child_reasoning_effort;
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
        let bootstrap =
            ThreadBootstrap::new(Some(archive_metadata)).with_messages(bootstrap_messages);
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

        if spec.is_read_only() {
            runner.enable_plan_mode();
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
        let transcript_path = persist_child_archive(
            &self.config.workspace_root,
            &session_id,
            &results,
            &messages,
            &child_cfg,
            child_reasoning_effort.as_str(),
            parent_session_id.as_str(),
            spec.name.as_str(),
        )
        .await?;

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
    if let Some(max_turns) = max_turns {
        child.automation.full_auto.max_turns = max_turns.max(1);
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
) -> crate::utils::session_archive::SessionArchiveMetadata {
    build_thread_archive_metadata(workspace_root, model, provider, theme, reasoning_effort)
        .with_parent_session_id(parent_session_id.to_string())
        .with_fork_mode(if forked {
            SessionForkMode::FullCopy
        } else {
            SessionForkMode::Summarized
        })
}

async fn persist_child_archive(
    workspace_root: &Path,
    session_id: &str,
    results: &TaskResults,
    messages: &[Message],
    child_cfg: &VTCodeConfig,
    reasoning_effort: &str,
    parent_session_id: &str,
    agent_name: &str,
) -> Result<Option<PathBuf>> {
    let metadata = build_thread_archive_metadata(
        workspace_root,
        child_cfg.agent.default_model.as_str(),
        child_cfg.agent.provider.as_str(),
        child_cfg.agent.theme.as_str(),
        reasoning_effort,
    )
    .with_parent_session_id(parent_session_id.to_string())
    .with_fork_mode(SessionForkMode::FullCopy);
    let archive = SessionArchive::new_with_identifier(metadata, session_id.to_string()).await?;
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
    let _ = results;
    Ok(Some(path))
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
    let resolved = if requested.eq_ignore_ascii_case("inherit") || requested.is_empty() {
        parent_model.to_string()
    } else if requested.eq_ignore_ascii_case("small") {
        if !vt_cfg.agent.small_model.model.trim().is_empty() {
            vt_cfg.agent.small_model.model.clone()
        } else {
            lightweight_model_for_provider(parent_provider, parent_model)
        }
    } else if requested.eq_ignore_ascii_case("haiku")
        || requested.eq_ignore_ascii_case("sonnet")
        || requested.eq_ignore_ascii_case("opus")
    {
        alias_model_for_provider(parent_provider, requested, parent_model)
    } else {
        requested.to_string()
    };

    resolved.parse::<ModelId>().with_context(|| {
        format!(
            "Failed to resolve model '{}' for subagent {}",
            resolved, agent_name
        )
    })
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

fn lightweight_model_for_provider(parent_provider: &str, parent_model: &str) -> String {
    alias_model_for_provider(parent_provider, "haiku", parent_model)
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

pub fn is_subagent_tool(name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::{
        SpawnAgentRequest, SubagentController, SubagentControllerConfig, SubagentStatus,
        build_child_config, filter_child_tools, request_prompt, resolve_subagent_model,
    };
    use crate::config::PermissionMode;
    use crate::config::VTCodeConfig;
    use crate::config::constants::models;
    use crate::config::constants::tools;
    use crate::config::types::ReasoningEffortLevel;
    use crate::llm::provider::ToolDefinition;
    use chrono::Utc;
    use std::collections::BTreeMap;
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;
    use tokio::sync::Notify;
    use vtcode_config::SubagentMcpServer;

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

    #[tokio::test]
    async fn controller_exposes_builtin_specs() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(SubagentControllerConfig {
            workspace_root: temp.path().to_path_buf(),
            parent_session_id: "parent-session".to_string(),
            parent_model: models::openai::GPT_5_4.to_string(),
            parent_provider: "openai".to_string(),
            parent_reasoning_effort: ReasoningEffortLevel::Medium,
            api_key: "test-key".to_string(),
            vt_cfg: VTCodeConfig::default(),
            openai_chatgpt_auth: None,
            depth: 0,
        })
        .await
        .expect("controller");
        let specs = controller.effective_specs().await;
        assert!(specs.iter().any(|spec| spec.name == "explorer"));
        assert!(specs.iter().any(|spec| spec.name == "worker"));
    }

    #[tokio::test]
    async fn close_marks_child_closed() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(SubagentControllerConfig {
            workspace_root: temp.path().to_path_buf(),
            parent_session_id: "parent-session".to_string(),
            parent_model: models::openai::GPT_5_4.to_string(),
            parent_provider: "openai".to_string(),
            parent_reasoning_effort: ReasoningEffortLevel::Medium,
            api_key: "test-key".to_string(),
            vt_cfg: VTCodeConfig::default(),
            openai_chatgpt_auth: None,
            depth: 0,
        })
        .await
        .expect("controller");
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
    async fn wait_returns_first_terminal_child() {
        let temp = TempDir::new().expect("tempdir");
        let controller = SubagentController::new(SubagentControllerConfig {
            workspace_root: temp.path().to_path_buf(),
            parent_session_id: "parent-session".to_string(),
            parent_model: models::openai::GPT_5_4.to_string(),
            parent_provider: "openai".to_string(),
            parent_reasoning_effort: ReasoningEffortLevel::Medium,
            api_key: "test-key".to_string(),
            vt_cfg: VTCodeConfig::default(),
            openai_chatgpt_auth: None,
            depth: 0,
        })
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
                        spec: spec.clone(),
                        status: SubagentStatus::Running,
                        background: false,
                        depth: 1,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        completed_at: None,
                        summary: None,
                        error: None,
                        transcript_path: None,
                        stored_messages: Vec::new(),
                        last_prompt: None,
                        queued_prompts: VecDeque::new(),
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
