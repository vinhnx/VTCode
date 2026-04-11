use super::{
    AutoModeRuntimeContext, SessionStats, ToolPermissionFlow, ToolPermissionsContext,
    approval_learning_target, approval_persistence::shell_command_has_persisted_approval_prefix,
    approval_policy_rejects_prompt, ensure_tool_permission, persist_segment_approval_cache_keys,
    persist_shell_approval_prefix_rule, tool_display_labels,
};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Notify, RwLock};
use vtcode_config::core::PromptCachingConfig;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::config::types::{ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference};
use vtcode_core::config::{PermissionMode, PermissionsConfig};
use vtcode_core::core::agent::snapshots::{
    DEFAULT_CHECKPOINTS_ENABLED, DEFAULT_MAX_AGE_DAYS, DEFAULT_MAX_SNAPSHOTS,
};
use vtcode_core::exec_policy::{AskForApproval, RejectConfig};
use vtcode_core::llm::provider as uni;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{InlineHandle, InlineSession};

fn create_headless_session() -> InlineSession {
    let (command_tx, _command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    InlineSession {
        handle: InlineHandle::new_for_tests(command_tx),
        events: event_rx,
    }
}

fn create_session_with_receiver() -> (
    InlineSession,
    tokio::sync::mpsc::UnboundedReceiver<vtcode_tui::app::InlineCommand>,
) {
    let (command_tx, command_rx) = tokio::sync::mpsc::unbounded_channel();
    let (_event_tx, event_rx) = tokio::sync::mpsc::unbounded_channel();
    (
        InlineSession {
            handle: InlineHandle::new_for_tests(command_tx),
            events: event_rx,
        },
        command_rx,
    )
}

fn drain_appended_lines(
    receiver: &mut tokio::sync::mpsc::UnboundedReceiver<vtcode_tui::app::InlineCommand>,
) -> Vec<String> {
    let mut lines = Vec::new();
    while let Ok(command) = receiver.try_recv() {
        if let vtcode_tui::app::InlineCommand::AppendLine { segments, .. } = command {
            let line = segments
                .into_iter()
                .map(|segment| segment.text)
                .collect::<String>();
            if !line.trim().is_empty() {
                lines.push(line);
            }
        }
    }
    lines
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
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["test-model".to_string()]
    }

    fn validate_request(&self, _request: &uni::LLMRequest) -> Result<(), uni::LLMError> {
        Ok(())
    }
}

#[test]
fn reject_policy_blocks_sandbox_prompts() {
    assert!(approval_policy_rejects_prompt(
        AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: false,
        }),
        false,
        true,
    ));
}

#[test]
fn reject_policy_blocks_rule_prompts() {
    assert!(approval_policy_rejects_prompt(
        AskForApproval::Reject(RejectConfig {
            sandbox_approval: false,
            rules: true,
            request_permissions: false,
            mcp_elicitations: false,
        }),
        true,
        false,
    ));
}

#[test]
fn on_request_policy_keeps_prompts_enabled() {
    assert!(!approval_policy_rejects_prompt(
        AskForApproval::OnRequest,
        true,
        true
    ));
}

#[test]
fn shell_learning_target_uses_scoped_prefix_rule() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "test"],
        "sandbox_permissions": "require_escalated",
    });

    let target = approval_learning_target("unified_exec", Some(&args), "Run command");
    assert_eq!(
        target.approval_key,
        "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    );
    assert_eq!(target.display_label, "commands starting with `cargo test`");
}

#[test]
fn shell_learning_target_falls_back_to_exact_command_scope() {
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "prefix_rule": ["cargo", "build"],
        "sandbox_permissions": "require_escalated",
    });

    let target = approval_learning_target("unified_exec", Some(&args), "Run command");
    assert_eq!(
        target.approval_key,
        "cargo test -p vtcode|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    );
    assert_eq!(target.display_label, "command `cargo test -p vtcode`");
}

#[test]
fn non_shell_display_labels_keep_learning_label_stable() {
    let args = json!({
        "path": "src/main.rs"
    });

    let labels = tool_display_labels("read_file", Some(&args));
    assert_eq!(labels.learning_label, "Read file");
    assert_eq!(labels.prompt_label, "Read file src/main.rs");
}

#[tokio::test]
async fn shell_approval_prefix_rules_persist_to_workspace_config() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let args = json!({
        "action": "run",
        "command": "cargo test -p vtcode",
        "sandbox_permissions": "require_escalated",
    });

    let rendered = persist_shell_approval_prefix_rule(
        &registry,
        "unified_exec",
        Some(&args),
        &["cargo".to_string(), "test".to_string()],
    )
    .await
    .expect("persist approval prefix");
    assert_eq!(
        rendered,
        "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    );

    let saved =
        ConfigManager::load_from_workspace(temp_dir.path()).expect("reload workspace config");
    assert!(
        saved
            .config()
            .commands
            .approval_prefixes
            .iter()
            .any(|entry| entry == &rendered)
    );
    assert!(shell_command_has_persisted_approval_prefix(
        &registry,
        &[
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            "vtcode".to_string()
        ],
        "sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    ));
    assert!(
        registry
            .find_persisted_shell_approval_prefix(
                &[
                    "cargo".to_string(),
                    "test".to_string(),
                    "-p".to_string(),
                    "vtcode".to_string()
                ],
                "sandbox_permissions=\"require_escalated\"|additional_permissions=null",
            )
            .await
            .is_some()
    );
}

#[tokio::test]
async fn shell_approval_prefix_matching_respects_token_boundaries() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager =
        ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config
        .commands
        .approval_prefixes
        .push("echo hi".to_string());
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &["echo".to_string(), "history".to_string()],
        "sandbox_permissions=\"use_default\"|additional_permissions=null"
    ));
}

#[tokio::test]
async fn shell_approval_prefix_matching_respects_permission_scope() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager =
        ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config.commands.approval_prefixes.push(
        "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
            .to_string(),
    );
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &[
            "cargo".to_string(),
            "test".to_string(),
            "-p".to_string(),
            "vtcode".to_string()
        ],
        "sandbox_permissions=\"with_additional_permissions\"|additional_permissions={\"fs_write\":[\"/tmp/demo.txt\"]}"
    ));
}

#[tokio::test]
async fn legacy_unscoped_shell_prefixes_do_not_match_escalated_runs() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let workspace_root = registry.workspace_root().clone();
    let mut manager =
        ConfigManager::load_from_workspace(&workspace_root).expect("load workspace config");
    let mut config = manager.config().clone();
    config
        .commands
        .approval_prefixes
        .push("echo hi".to_string());
    manager.save_config(&config).expect("save config");
    registry.apply_commands_config(&config.commands);

    assert!(!shell_command_has_persisted_approval_prefix(
        &registry,
        &["echo".to_string(), "hi".to_string()],
        "sandbox_permissions=\"require_escalated\"|additional_permissions=null"
    ));
}

#[tokio::test]
async fn skip_confirmations_does_not_bypass_cached_tool_denial() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    permission_cache
        .write()
        .await
        .cache_grant(tools::READ_FILE.to_string(), PermissionGrant::Denied);

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: Some(&permission_cache),
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: true,
            permissions_config: None,
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::READ_FILE,
        None,
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn tool_policy_deny_overrides_cached_session_approval() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry
        .set_tool_policy(tools::READ_FILE, ToolPolicy::Deny)
        .await
        .expect("persist deny policy");

    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permission_cache = Arc::new(RwLock::new(ToolPermissionCache::new()));
    permission_cache
        .write()
        .await
        .cache_grant(tools::READ_FILE.to_string(), PermissionGrant::Session);

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: Some(&permission_cache),
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            permissions_config: None,
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn dont_ask_mode_denies_non_allowed_requests() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::DontAsk,
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn dont_ask_mode_allows_explicitly_allowed_tools() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::DontAsk,
        allow: vec![tools::READ_FILE.to_string()],
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::READ_FILE,
        Some(&json!({"path": "README.md"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}

#[tokio::test]
async fn accept_edits_mode_auto_allows_builtin_file_mutations() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::AcceptEdits,
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "notes.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
}

#[tokio::test]
async fn accept_edits_mode_keeps_protected_write_prompts() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::AcceptEdits,
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": ".vtcode/settings.toml", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn bypass_permissions_keeps_protected_write_prompts() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::BypassPermissions,
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": ".vtcode/settings.toml", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn ask_rules_override_bypass_permissions_mode() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::BypassPermissions,
        ask: vec!["Write(/docs/**)".to_string()],
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::UNIFIED_FILE,
        Some(&json!({"action": "write", "path": "docs/guide.md", "content": "hello"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn disallowed_tools_override_bypass_permissions_mode() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::BypassPermissions,
        deny: vec![tools::UNIFIED_EXEC.to_string()],
        ..PermissionsConfig::default()
    };

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: None,
            active_thread_label: None,
            session_stats: None,
        },
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
}

#[tokio::test]
async fn permanent_shell_approval_persists_segmented_commands() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let args = json!({
        "action": "run",
        "command": ["/bin/zsh", "-lc", "git status && cargo check"],
    });

    persist_segment_approval_cache_keys(&registry, "unified_exec", "unified_exec", Some(&args))
        .await;

    assert!(
        registry
            .has_persisted_approval(
                "git status|sandbox_permissions=\"use_default\"|additional_permissions=null"
            )
            .await
    );
    assert!(
        registry
            .has_persisted_approval(
                "cargo check|sandbox_permissions=\"use_default\"|additional_permissions=null"
            )
            .await
    );
}

#[tokio::test]
async fn auto_mode_headless_fallback_returns_blocked_summary() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::stdout();
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec![
            "BLOCK".to_string(),
            r#"{"decision":"block","reason":"force push is destructive","matched_rule":"Destroy or exfiltrate","matched_exception":null}"#.to_string(),
        ]),
    };
    let config = runtime_config();
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::Auto,
        auto_mode: vtcode_core::config::AutoModeConfig {
            max_consecutive_denials: 1,
            max_total_denials: 20,
            ..Default::default()
        },
        ..PermissionsConfig::default()
    };
    let history = vec![uni::Message::user("clean up the PR".to_string())];
    let mut session_stats = SessionStats::default();
    session_stats.set_autonomous_mode(true);

    let flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::OnRequest,
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: Some(AutoModeRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            }),
            active_thread_label: None,
            session_stats: Some(&mut session_stats),
        },
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "git push --force"})),
    )
    .await
    .expect("permission flow");

    match flow {
        ToolPermissionFlow::Blocked { reason } => {
            assert!(reason.contains("non-interactive mode"));
            assert!(reason.contains("force push is destructive"));
            assert!(reason.contains("Destroy or exfiltrate"));
        }
        other => panic!("expected blocked flow, got {other:?}"),
    }
}

#[tokio::test]
async fn auto_mode_interactive_fallback_notice_is_emitted_once() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let (mut session, mut receiver) = create_session_with_receiver();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(crate::agent::runloop::unified::state::CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec![
            "BLOCK".to_string(),
            r#"{"decision":"block","reason":"force push is destructive","matched_rule":"Destroy or exfiltrate","matched_exception":null}"#.to_string(),
        ]),
    };
    let config = runtime_config();
    let permissions = PermissionsConfig {
        default_mode: PermissionMode::Auto,
        auto_mode: vtcode_core::config::AutoModeConfig {
            max_consecutive_denials: 1,
            max_total_denials: 20,
            ..Default::default()
        },
        ..PermissionsConfig::default()
    };
    let history = vec![uni::Message::user("clean up the PR".to_string())];
    let mut session_stats = SessionStats::default();
    session_stats.set_autonomous_mode(true);

    let first_flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: Some(AutoModeRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            }),
            active_thread_label: None,
            session_stats: Some(&mut session_stats),
        },
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "git push --force"})),
    )
    .await
    .expect("first permission flow");

    assert_eq!(first_flow, ToolPermissionFlow::Denied);

    let first_lines = drain_appended_lines(&mut receiver);
    assert!(first_lines.iter().any(|line| {
        line.contains("Auto mode fell back to manual prompts after repeated classifier denials.")
    }));

    let second_flow = ensure_tool_permission(
        ToolPermissionsContext {
            tool_registry: &registry,
            renderer: &mut renderer,
            handle: &handle,
            session: &mut session,
            default_placeholder: None,
            ctrl_c_state: &ctrl_c_state,
            ctrl_c_notify: &ctrl_c_notify,
            hooks: None,
            justification: None,
            approval_recorder: None,
            decision_ledger: None,
            tool_permission_cache: None,
            permissions_state: None,
            hitl_notification_bell: false,
            approval_policy: AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: true,
                request_permissions: true,
                mcp_elicitations: true,
            }),
            skip_confirmations: false,
            permissions_config: Some(&permissions),
            auto_mode_runtime: Some(AutoModeRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            }),
            active_thread_label: None,
            session_stats: Some(&mut session_stats),
        },
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "git push --force"})),
    )
    .await
    .expect("second permission flow");

    assert_eq!(second_flow, ToolPermissionFlow::Denied);
    assert!(drain_appended_lines(&mut receiver).is_empty());
}
