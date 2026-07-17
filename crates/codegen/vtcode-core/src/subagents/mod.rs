#![allow(unused_imports)]
// ─── Module Structure ───────────────────────────────────────────────────────

mod background;
mod config;
mod constants;
mod discovery;
mod model;
mod prompt;
mod types;

// ─── Re-exports ─────────────────────────────────────────────────────────────

pub use background::{
    background_record_id, build_background_subagent_command, extract_tail_lines,
    load_archive_preview, subagent_display_label,
};
pub use config::{
    ResolvedAgentRuntimeView, build_child_config, compose_subagent_instructions,
    filter_child_tools, normalize_background_child_max_turns, normalize_child_max_turns,
    prepare_child_runtime_config,
};
pub use model::{agent_type_for_spec, load_memory_appendix, load_primary_memory_appendix};
pub use prompt::{
    contains_explicit_delegation_request, contains_explicit_model_request,
    delegated_task_requires_clarification, extract_explicit_agent_mentions,
    normalize_requested_model_override, request_prompt, sanitize_subagent_input_items,
};
pub use types::{
    BackgroundRecord, BackgroundSubprocessEntry, BackgroundSubprocessSnapshot,
    BackgroundSubprocessStatus, ChildRecord, ChildRunResult, ControllerState,
    PersistedBackgroundRecord, PersistedBackgroundState, SendInputRequest, SpawnAgentRequest,
    SpawnBackgroundSubprocessRequest, StatusEntryBuilder, SubagentInputItem, SubagentStatus,
    SubagentStatusEntry, SubagentThreadSnapshot, TurnDelegationHints,
};

// VerificationResult is defined in this module (below) and re-exported at the
// crate root via `pub use subagents::VerificationResult`.

// ─── Public Utilities ───────────────────────────────────────────────────────

/// Returns `true` if `name` is one of the reserved subagent-internal tool names.
pub fn is_subagent_tool(name: &str) -> bool {
    SUBAGENT_TOOL_NAMES.contains(&name)
}

#[derive(Clone, Default)]
pub(super) struct BackgroundLaunchOverrides {
    pub(super) prompt: Option<String>,
    pub(super) max_turns: Option<usize>,
    pub(super) model_override: Option<String>,
    pub(super) reasoning_override: Option<String>,
}

#[derive(Clone, Default)]
pub(super) struct PreparedDelegationContext {
    pub(super) requested_agent: Option<String>,
    pub(super) explicit_mentions: Vec<String>,
    pub(super) explicit_request: bool,
}

/// Result of a propose/verify cycle.
///
/// Returned by [`SubagentController::verify_proposed_change`]. The caller
/// inspects `approved` to decide whether to commit or retry the mutation.
#[derive(Debug, Clone)]
pub struct VerificationResult {
    /// Whether the verifier approved the change.
    pub approved: bool,
    /// Concrete issues identified by the verifier (empty if approved).
    pub issues: Vec<String>,
    /// Free-text reasoning from the verifier.
    pub reasoning: String,
}

// ─── Controller ─────────────────────────────────────────────────────────────

use anyhow::{Context, Result, anyhow, bail};
use chrono::Utc;
use futures::future::select_all;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Notify, RwLock};

use crate::config::VTCodeConfig;
use crate::config::types::ReasoningEffortLevel;
use crate::core::agent::runner::{AgentRunner, RunnerSettings};
use crate::core::agent::task::Task;
use crate::core::threads::{ThreadBootstrap, ThreadId, ThreadRuntimeHandle, ThreadSnapshot};
use crate::hooks::{LifecycleHookEngine, SessionStartTrigger};
use crate::llm::provider::Message;
use crate::tools::exec_session::ExecSessionManager;
use crate::tools::pty::{PtyManager, PtySize};
use crate::utils::session_archive::{SessionArchive, find_session_by_identifier};
use vtcode_config::SubagentSpec;
use vtcode_config::auth::OpenAIChatGptAuthHandle;

use self::background::*;
use self::config::*;
use self::constants::*;
use self::discovery::discover_controller_subagents;
use self::model::*;
use vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT;

// ─── Controller Config ─────────────────────────────────────────────────────

/// Configuration required to construct a [`SubagentController`].
#[derive(Clone)]
pub struct SubagentControllerConfig {
    /// Workspace root directory for the session.
    pub workspace_root: PathBuf,
    /// Session identifier of the parent agent.
    pub parent_session_id: String,
    /// Model identifier used by the parent agent.
    pub parent_model: String,
    /// Provider name used by the parent agent.
    pub parent_provider: String,
    /// Reasoning effort level of the parent agent.
    pub parent_reasoning_effort: ReasoningEffortLevel,
    /// API key for LLM provider access.
    pub api_key: String,
    /// Full VT Code configuration.
    pub vt_cfg: VTCodeConfig,
    /// Optional OpenAI ChatGPT authentication handle.
    pub openai_chatgpt_auth: Option<OpenAIChatGptAuthHandle>,
    /// Current nesting depth of the subagent hierarchy.
    pub depth: usize,
    /// Manager for exec sessions (PTY and pipe).
    pub exec_sessions: ExecSessionManager,
    /// PTY session manager.
    pub pty_manager: PtyManager,
    /// Whether this controller manages a background runtime subprocess.
    pub managed_background_runtime: bool,
}

/// Central controller that manages spawning, lifecycle, and state of all subagents.
#[derive(Clone)]
pub struct SubagentController {
    config: Arc<SubagentControllerConfig>,
    parent_session_id: Arc<RwLock<String>>,
    lifecycle_hooks: Option<LifecycleHookEngine>,
    state: Arc<RwLock<ControllerState>>,
    shutdown_requested: Arc<AtomicBool>,
}

impl SubagentController {
    /// Creates a new controller, discovering subagent specs and loading persisted background state.
    pub async fn new(config: SubagentControllerConfig) -> Result<Self> {
        let discovered = discover_controller_subagents(&config.workspace_root).await?;
        let lifecycle_hooks = LifecycleHookEngine::new_with_session(
            config.workspace_root.clone(),
            &config.vt_cfg.hooks,
            SessionStartTrigger::Startup,
            config.parent_session_id.clone(),
        )?;
        let background_children = load_background_state(&config.workspace_root)
            .await?
            .records
            .into_iter()
            .map(|record| (record.id.clone(), BackgroundRecord::from_persisted(record)))
            .collect();
        Ok(Self {
            parent_session_id: Arc::new(RwLock::new(config.parent_session_id.clone())),
            lifecycle_hooks,
            config: Arc::new(config),
            state: Arc::new(RwLock::new(ControllerState {
                discovered,
                parent_messages: Vec::new(),
                turn_hints: TurnDelegationHints::default(),
                children: std::collections::BTreeMap::new(),
                background_children,
            })),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Re-discovers subagent specs from the workspace.
    pub async fn reload(&self) -> Result<()> {
        let discovered = discover_controller_subagents(&self.config.workspace_root).await?;
        self.state.write().await.discovered = discovered;
        Ok(())
    }

    /// Stores the parent conversation messages for context forking into children.
    pub async fn set_parent_messages(&self, messages: &[Message]) {
        let cloned = messages.to_vec();
        self.state.write().await.parent_messages = cloned;
    }

    /// Parses the current user input to extract explicit agent mentions and delegation signals.
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

    /// Resets delegation hints at the end of a turn.
    pub async fn clear_turn_delegation_hints(&self) {
        self.state.write().await.turn_hints = TurnDelegationHints::default();
    }

    /// Updates the parent session identifier at runtime.
    pub async fn set_parent_session_id(&self, session_id: impl Into<String>) {
        *self.parent_session_id.write().await = session_id.into();
    }

    /// Returns the currently effective subagent specifications (merged builtin + workspace).
    pub async fn effective_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.effective.clone()
    }

    /// Returns specs that are shadowed by workspace-level overrides.
    pub async fn shadowed_specs(&self) -> Vec<SubagentSpec> {
        self.state.read().await.discovered.shadowed.clone()
    }

    /// Returns status entries for all tracked child subagents.
    pub async fn status_entries(&self) -> Vec<SubagentStatusEntry> {
        let state = self.state.read().await;
        state
            .children
            .values()
            .map(ChildRecord::build_status_entry)
            .collect()
    }
}

// ─── Controller submodule split ────────────────────────────────────────────

mod controller_background_ops;
mod controller_child_loop;
mod controller_helpers;
mod controller_spawn_run;
mod controller_verify;

#[allow(unused_imports)]
pub(super) use controller_helpers::*;

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
