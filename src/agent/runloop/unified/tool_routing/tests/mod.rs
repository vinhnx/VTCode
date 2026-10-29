pub(super) use super::{
    AutoPermissionRuntimeContext, HitlDecision, PreToolHookPhaseResult, SessionStats, ToolPermissionFlow,
    ToolPermissionsContext, approval_learning_target,
    approval_persistence::shell_command_has_persisted_approval_prefix, approval_policy_rejects_prompt,
    ensure_tool_permission, exact_shell_approval_target, finalize_permission_decision,
    persist_segment_approval_cache_keys, persist_shell_approval_prefix_rule, persisted_approval_hit_key,
    persisted_segment_approval_hit_key, persistent_approval_target, tool_display_labels,
};
pub(super) use anyhow::Result;
pub(super) use async_trait::async_trait;
pub(super) use serde_json::json;
pub(super) use std::collections::BTreeMap;
pub(super) use std::fs;
pub(super) use std::path::Path;
pub(super) use std::sync::Arc;
pub(super) use tokio::sync::{Notify, RwLock};
pub(super) use vtcode_config::core::PromptCachingConfig;
pub(super) use vtcode_config::core::permissions::{AgentPermissionsConfig, PermissionDefault};
pub(super) use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
pub(super) use vtcode_core::config::PermissionsConfig;
pub(super) use vtcode_core::config::constants::tools;
pub(super) use vtcode_core::config::loader::ConfigManager;
pub(super) use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
pub(super) use vtcode_core::config::types::{ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference};
pub(super) use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
pub(super) use vtcode_core::core::interfaces::ui::UiSession;
pub(super) use vtcode_core::exec_policy::{AskForApproval, RejectConfig};
pub(super) use vtcode_core::hooks::LifecycleHookEngine;
pub(super) use vtcode_core::llm::provider as uni;
pub(super) use vtcode_core::tool_policy::ToolPolicy;
pub(super) use vtcode_core::tools::registry::ToolRegistry;
pub(super) use vtcode_core::utils::ansi::AnsiRenderer;
pub(super) use vtcode_ui::tui::app::{InlineHandle, InlineSession};

pub(super) use crate::agent::runloop::unified::state::CtrlCState;

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
        worker: None,
    }
}

/// Builder for [`ToolPermissionsContext`].
///
/// The production context carries two dozen fields and the tests vary only a
/// few of them, so a builder keeps each call site to the fields it actually
/// sets.
struct PermissionsContextBuilder<'a, S: UiSession + ?Sized> {
    tool_registry: &'a ToolRegistry,
    renderer: &'a mut AnsiRenderer,
    handle: &'a InlineHandle,
    session: &'a mut S,
    ctrl_c_state: &'a Arc<CtrlCState>,
    ctrl_c_notify: &'a Arc<Notify>,
    hooks: Option<&'a LifecycleHookEngine>,
    tool_permission_cache: Option<&'a Arc<RwLock<ToolPermissionCache>>>,
    permissions_config: Option<&'a PermissionsConfig>,
    active_agent_permissions: Option<&'a AgentPermissionsConfig>,
    auto_permission_runtime: Option<AutoPermissionRuntimeContext<'a>>,
    session_stats: Option<&'a mut SessionStats>,
    approval_policy: AskForApproval,
    skip_confirmations: bool,
    safety_approval_justification: Option<String>,
}

impl<'a, S: UiSession + ?Sized> PermissionsContextBuilder<'a, S> {
    fn new(
        tool_registry: &'a ToolRegistry,
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        session: &'a mut S,
        ctrl_c_state: &'a Arc<CtrlCState>,
        ctrl_c_notify: &'a Arc<Notify>,
    ) -> Self {
        Self {
            tool_registry,
            renderer,
            handle,
            session,
            ctrl_c_state,
            ctrl_c_notify,
            hooks: None,
            tool_permission_cache: None,
            permissions_config: None,
            active_agent_permissions: None,
            auto_permission_runtime: None,
            session_stats: None,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            safety_approval_justification: None,
        }
    }

    fn with_hooks(mut self, hooks: &'a LifecycleHookEngine) -> Self {
        self.hooks = Some(hooks);
        self
    }

    fn with_tool_permission_cache(mut self, cache: &'a Arc<RwLock<ToolPermissionCache>>) -> Self {
        self.tool_permission_cache = Some(cache);
        self
    }

    fn with_permissions_config(mut self, permissions: &'a PermissionsConfig) -> Self {
        self.permissions_config = Some(permissions);
        self
    }

    fn with_active_agent_permissions(mut self, permissions: &'a AgentPermissionsConfig) -> Self {
        self.active_agent_permissions = Some(permissions);
        self
    }

    fn with_auto_permission_runtime(mut self, runtime: AutoPermissionRuntimeContext<'a>) -> Self {
        self.auto_permission_runtime = Some(runtime);
        self
    }

    fn with_session_stats(mut self, stats: &'a mut SessionStats) -> Self {
        self.session_stats = Some(stats);
        self
    }

    fn with_approval_policy(mut self, policy: AskForApproval) -> Self {
        self.approval_policy = policy;
        self
    }

    fn with_skip_confirmations(mut self, skip: bool) -> Self {
        self.skip_confirmations = skip;
        self
    }

    fn with_safety_approval_justification(mut self, justification: Option<String>) -> Self {
        self.safety_approval_justification = justification;
        self
    }

    fn build(self) -> ToolPermissionsContext<'a, S> {
        ToolPermissionsContext {
            tool_registry: self.tool_registry,
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            active_thread_label: None,
            default_placeholder: None,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            hooks: self.hooks,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: self.tool_permission_cache,
            permissions_state: None,
            active_agent_permissions: self.active_agent_permissions,
            hitl_notification_bell: false,
            approval_policy: self.approval_policy,
            skip_confirmations: self.skip_confirmations,
            permissions_config: self.permissions_config,
            auto_permission_runtime: self.auto_permission_runtime,
            session_stats: self.session_stats,
            safety_approval_justification: self.safety_approval_justification,
            harness_emitter: None,
        }
    }
}

fn create_session_with_receiver()
-> (InlineSession, tokio::sync::mpsc::UnboundedReceiver<vtcode_ui::tui::app::InlineCommand>) {
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    (
        InlineSession {
            handle: InlineHandle::new_for_tests(command_tx),
            events: event_rx,
            worker: None,
        },
        command_rx,
    )
}

fn isolate_workspace_config(workspace: &Path) {
    fs::write(workspace.join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
        .expect("write isolated workspace config");
}

fn drain_appended_lines(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_ui::tui::app::InlineCommand>,
) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        match command {
            vtcode_ui::tui::app::InlineCommand::AppendLine { segments, .. } => {
                let line = segments.into_iter().map(|segment| segment.text).collect::<String>();
                if !line.trim().is_empty() {
                    lines.push(line);
                }
            }
            vtcode_ui::tui::app::InlineCommand::Diagnostic(diagnostic) => lines.push(diagnostic.text()),
            _ => {}
        }
    }
    lines
}

fn reject_all_approvals() -> AskForApproval {
    AskForApproval::Reject(RejectConfig {
        sandbox_approval: true,
        rules: true,
        request_permissions: true,
        mcp_elicitations: true,
    })
}

fn runtime_config() -> CoreAgentConfig {
    CoreAgentConfig {
        model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW.to_string(),
        api_key: "test-key".to_string(),
        provider: "gemini".to_string(),
        api_key_env: "GEMINI_API_KEY".to_string(),
        workspace: std::env::current_dir().expect("current_dir"),
        verbose: false,
        quiet: false,
        theme: vtcode_core::ui::theme::DEFAULT_THEME_ID.to_string(),
        reasoning_effort: ReasoningEffortLevel::default(),
        ui_surface: UiSurfacePreference::default(),
        prompt_cache: PromptCachingConfig::default(),
        model_source: ModelSelectionSource::WorkspaceConfig,
        custom_api_keys: BTreeMap::new(),
        checkpointing_enabled: DEFAULT_CHECKPOINTS_ENABLED,
        checkpointing_storage_dir: None,
        checkpointing_max_snapshots: DEFAULT_MAX_SNAPSHOTS,
        checkpointing_max_age_days: Some(DEFAULT_MAX_AGE_DAYS),
        max_conversation_turns: 1000,
        model_behavior: None,
        openai_chatgpt_auth: None,
    }
}

struct StaticProvider {
    responses: std::sync::Mutex<Vec<String>>,
}

#[async_trait]
impl uni::LLMProvider for StaticProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        let response = self.responses.lock().expect("responses lock").remove(0);
        Ok(uni::LLMResponse {
            content: Some(response),
            model: "test-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: uni::FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: Vec::new(),
            compaction: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

struct ErrorProvider;

#[async_trait]
impl uni::LLMProvider for ErrorProvider {
    fn name(&self) -> &str {
        "test"
    }

    async fn generate(&self, _request: uni::LLMRequest) -> Result<uni::LLMResponse, uni::LLMError> {
        Err(uni::LLMError::Provider {
            message: "reviewer unavailable".to_string(),
            metadata: None,
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

mod active_agent;
mod deny_overrides;
mod fallback_full_access;
mod full_auto_review;
mod hook_rewrite;
mod policy_prompts;
mod shell_persist;
mod shell_prefix;
mod skip_allow;
