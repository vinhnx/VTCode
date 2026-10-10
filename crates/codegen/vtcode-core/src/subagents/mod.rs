#![allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
// ─── Module Structure ───────────────────────────────────────────────────────

mod background;
mod config;
mod constants;
mod discovery;
pub mod matrix;
mod model;
mod prompt;
mod types;

// ─── Re-exports ─────────────────────────────────────────────────────────────

pub use background::{
    background_record_id, build_background_subagent_command, extract_tail_lines, load_archive_preview,
    subagent_display_label,
};
pub use config::{
    ResolvedAgentRuntimeView, build_child_config, compose_subagent_instructions, filter_child_tools,
    normalize_background_child_max_turns, normalize_child_max_turns, prepare_child_runtime_config,
};
pub use discovery::discover_controller_subagents;
pub use model::{
    agent_type_for_spec, load_memory_appendix, load_memory_appendix_async, load_primary_memory_appendix,
    load_primary_memory_appendix_async,
};
pub use prompt::{
    contains_explicit_delegation_request, contains_explicit_model_request, delegated_task_requires_clarification,
    extract_explicit_agent_mentions, normalize_requested_model_override, request_prompt, sanitize_subagent_input_items,
};
pub use types::{
    BackgroundCompletionEvent, BackgroundRecord, BackgroundSubprocessEntry, BackgroundSubprocessSnapshot,
    BackgroundSubprocessStatus, ChildRecord, ChildRunResult, ControllerState, PersistedBackgroundRecord,
    PersistedBackgroundState, SendInputRequest, SpawnAgentRequest, SpawnBackgroundSubprocessRequest,
    StatusEntryBuilder, SubagentInputItem, SubagentStatus, SubagentStatusEntry, SubagentThreadSnapshot,
    TurnDelegationHints,
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
    prompt: Option<String>,
    max_turns: Option<usize>,
    model_override: Option<String>,
    reasoning_override: Option<String>,
}

#[derive(Clone, Default)]
pub(super) struct PreparedDelegationContext {
    requested_agent: Option<String>,
    explicit_mentions: Vec<String>,
    explicit_request: bool,
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
use parking_lot::Mutex as ParkingMutex;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Mutex, Notify, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

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
use self::model::*;
use vtcode_config::subagents::SUBAGENT_HARD_CONCURRENCY_LIMIT;

const BACKGROUND_COMPLETION_CHANNEL_CAPACITY: usize = 64;

struct BackgroundCompletionChannel {
    sender: broadcast::Sender<BackgroundCompletionEvent>,
    parent_sender: broadcast::Sender<BackgroundCompletionEvent>,
    pending_parent_events: VecDeque<BackgroundCompletionEvent>,
    parent_subscribed: bool,
}

impl BackgroundCompletionChannel {
    fn new() -> Self {
        let (sender, _) = broadcast::channel(BACKGROUND_COMPLETION_CHANNEL_CAPACITY);
        let (parent_sender, _) = broadcast::channel(BACKGROUND_COMPLETION_CHANNEL_CAPACITY);
        Self {
            sender,
            parent_sender,
            pending_parent_events: VecDeque::new(),
            parent_subscribed: false,
        }
    }

    fn subscribe(&self) -> broadcast::Receiver<BackgroundCompletionEvent> {
        self.sender.subscribe()
    }

    fn subscribe_parent(&mut self) -> (broadcast::Receiver<BackgroundCompletionEvent>, bool) {
        let receiver = self.parent_sender.subscribe();
        self.parent_subscribed = true;
        let replayed = !self.pending_parent_events.is_empty();
        while let Some(event) = self.pending_parent_events.pop_front() {
            let _ = self.parent_sender.send(event);
        }
        (receiver, replayed)
    }

    fn publish(&mut self, event: BackgroundCompletionEvent) {
        if !self.parent_subscribed {
            if self.pending_parent_events.len() >= BACKGROUND_COMPLETION_CHANNEL_CAPACITY {
                self.pending_parent_events.pop_front();
                tracing::warn!(
                    capacity = BACKGROUND_COMPLETION_CHANNEL_CAPACITY,
                    "Dropping oldest undelivered parent background completion"
                );
            }
            self.pending_parent_events.push_back(event.clone());
        } else {
            let _ = self.parent_sender.send(event.clone());
        }
        let _ = self.sender.send(event);
    }
}

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
    /// Whether the subagent lifecycle engine must be workspace-gated. Mirrors
    /// the main-session rule: pass `true` whenever workspace-controlled hook
    /// content is present (workspace vtcode.toml/.vtcode layers OR a primary
    /// agent spec contributing workspace-controlled hooks). Failing to gate
    /// here would let workspace hooks run without user approval in a
    /// subagent context.
    pub workspace_gated: bool,
    /// Manager for exec sessions (PTY and pipe).
    pub exec_sessions: ExecSessionManager,
    /// PTY session manager.
    pub pty_manager: PtyManager,
    /// Whether this controller manages a background runtime subprocess.
    pub managed_background_runtime: bool,
}

/// Central controller that manages spawning, lifecycle, and state of all subagents.
/// The background completion monitor is cancelled when the final controller
/// owner is dropped; its task-held clone is intentionally non-owning.
pub struct SubagentController {
    admission: Arc<tokio::sync::Semaphore>,
    matrix: Arc<matrix::MatrixRuntime>,
    config: Arc<SubagentControllerConfig>,
    parent_session_id: Arc<RwLock<String>>,
    lifecycle_hooks: Option<LifecycleHookEngine>,
    state: Arc<RwLock<ControllerState>>,
    shutdown_requested: Arc<AtomicBool>,
    /// Transient close-in-progress flag. Unlike `shutdown_requested` this is
    /// cleared when a subtree is reopened via `reopen_single`, so a resumed
    /// child can delegate again and its controller keeps saving background
    /// state. Set only while `close_tree`/`signal_shutdown` are tearing a
    /// subtree down.
    closing: Arc<AtomicBool>,
    background_completion_channel: Arc<ParkingMutex<BackgroundCompletionChannel>>,
    background_completion_notify: Arc<Notify>,
    background_completion_shutdown: CancellationToken,
    background_completion_monitor: Arc<Mutex<Option<JoinHandle<()>>>>,
    /// Counts controller clones that participate in monitor ownership.
    background_completion_owners: Arc<AtomicUsize>,
    /// The monitor task retains a clone for processing but must not keep the
    /// monitor alive after all external controller owners are gone.
    background_completion_monitor_owner: bool,
}

impl Clone for SubagentController {
    fn clone(&self) -> Self {
        self.background_completion_owners.fetch_add(1, Ordering::Relaxed);
        Self {
            admission: Arc::clone(&self.admission),
            matrix: Arc::clone(&self.matrix),
            config: Arc::clone(&self.config),
            parent_session_id: Arc::clone(&self.parent_session_id),
            lifecycle_hooks: self.lifecycle_hooks.clone(),
            state: Arc::clone(&self.state),
            shutdown_requested: Arc::clone(&self.shutdown_requested),
            closing: Arc::clone(&self.closing),
            background_completion_channel: Arc::clone(&self.background_completion_channel),
            background_completion_notify: Arc::clone(&self.background_completion_notify),
            background_completion_shutdown: self.background_completion_shutdown.clone(),
            background_completion_monitor: Arc::clone(&self.background_completion_monitor),
            background_completion_owners: Arc::clone(&self.background_completion_owners),
            background_completion_monitor_owner: true,
        }
    }
}

impl Drop for SubagentController {
    fn drop(&mut self) {
        if !self.background_completion_monitor_owner
            || self.background_completion_owners.fetch_sub(1, Ordering::AcqRel) != 1
        {
            return;
        }

        self.background_completion_shutdown.cancel();
        if let Ok(mut monitor_slot) = self.background_completion_monitor.try_lock()
            && let Some(monitor) = monitor_slot.take()
        {
            monitor.abort();
        }
    }
}

impl SubagentController {
    /// Creates a new controller, discovering subagent specs and loading persisted background state.
    pub async fn new(config: SubagentControllerConfig) -> Result<Self> {
        let discovered = discover_controller_subagents(&config.workspace_root).await?;
        // Box the inner constructor: it carries `VTCodeConfig` + spec state
        // across awaits, which would otherwise bloat every `new` caller's
        // async frame past the `large_futures` budget (denied in test builds).
        Box::pin(Self::new_with_discovered(config, discovered)).await
    }

    /// Creates a new controller reusing an already-discovered spec set.
    ///
    /// Interactive startup discovers specs once on the first-paint path;
    /// hydration passes that result here to skip a second workspace/plugin
    /// filesystem scan. Behavior matches [`Self::new`] otherwise.
    pub async fn new_with_discovered(
        config: SubagentControllerConfig,
        discovered: vtcode_config::DiscoveredSubagents,
    ) -> Result<Self> {
        let workspace_gated = config.workspace_gated;
        let lifecycle_hooks = LifecycleHookEngine::new_with_session_gated(
            config.workspace_root.clone(),
            &config.vt_cfg.hooks,
            SessionStartTrigger::Startup,
            config.parent_session_id.clone(),
            workspace_gated,
        )?;
        if let Some(engine) = lifecycle_hooks.as_ref() {
            crate::hooks::lifecycle::restore_workspace_hook_approval(engine, &config.workspace_root).await;
        }
        let background_children = load_background_state(&config.workspace_root)
            .await?
            .records
            .into_iter()
            .map(|record| (record.id.clone(), BackgroundRecord::from_persisted(record)))
            .collect();
        let controller = Self {
            admission: Arc::new(tokio::sync::Semaphore::new(
                config.vt_cfg.subagents.max_concurrent.min(SUBAGENT_HARD_CONCURRENCY_LIMIT),
            )),
            matrix: Arc::new(matrix::MatrixRuntime::default()),
            parent_session_id: Arc::new(RwLock::new(config.parent_session_id.clone())),
            lifecycle_hooks,
            config: Arc::new(config),
            state: Arc::new(RwLock::new(ControllerState {
                discovered,
                parent_messages: Vec::new(),
                turn_hints: TurnDelegationHints::default(),
                children: std::collections::BTreeMap::new(),
                background_children,
                background_completion_identities: VecDeque::new(),
            })),
            shutdown_requested: Arc::new(AtomicBool::new(false)),
            closing: Arc::new(AtomicBool::new(false)),
            background_completion_channel: Arc::new(ParkingMutex::new(BackgroundCompletionChannel::new())),
            background_completion_notify: Arc::new(Notify::new()),
            background_completion_shutdown: CancellationToken::new(),
            background_completion_monitor: Arc::new(Mutex::new(None)),
            background_completion_owners: Arc::new(AtomicUsize::new(1)),
            background_completion_monitor_owner: true,
        };
        controller.start_background_completion_monitor().await;
        Ok(controller)
    }

    /// Subscribes to terminal notifications for managed background subprocesses.
    pub fn subscribe_background_completions(&self) -> broadcast::Receiver<BackgroundCompletionEvent> {
        self.background_completion_channel.lock().subscribe()
    }

    /// Subscribes the parent run loop and replays completions that arrived
    /// before its receiver was installed.
    pub fn subscribe_parent_background_completions(&self) -> broadcast::Receiver<BackgroundCompletionEvent> {
        let (receiver, replayed) = self.background_completion_channel.lock().subscribe_parent();
        if replayed {
            self.background_completion_notify.notify_one();
        }
        receiver
    }

    /// Returns the wake signal used by the interactive loop while it is idle.
    pub fn background_completion_notify(&self) -> Arc<Notify> {
        Arc::clone(&self.background_completion_notify)
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
        let explicit_mentions = extract_explicit_agent_mentions(input, state.discovered.effective.as_slice());
        let explicit_request = contains_explicit_delegation_request(input, explicit_mentions.as_slice());
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
        let mut entries = state.children.values().map(ChildRecord::build_status_entry).collect::<Vec<_>>();
        drop(state);
        entries.extend(self.matrix_projection_entries().await);
        entries
    }
}

// ─── Controller submodule split ────────────────────────────────────────────

mod controller_background_ops;
mod controller_child_loop;
mod controller_helpers;
mod controller_spawn_run;
mod controller_verify;

#[allow(
    unused_imports,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use controller_helpers::*;

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests;
