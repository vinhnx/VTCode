#![allow(clippy::too_many_arguments)]
mod approval_cache;
mod approval_persistence;
mod approval_policy;
mod hook_messages;
mod limit_prompts;
mod permission_prompt;
mod shell_approval;

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use serde_json::Value;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::tools::{JustificationExtractor, ToolRiskScorer};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::InlineHandle;

use super::state::CtrlCState;
use approval_cache::{cache_key, spawn_approval_record_task};
use approval_persistence::{persist_shell_approval_prefix_rule, persisted_shell_approval};
use approval_policy::{
    approval_policy_rejects_prompt, build_tool_risk_context,
    trusted_auto_allows_history_based_approval, trusted_auto_allows_immediate_approval,
};
use hook_messages::render_hook_messages;
use permission_prompt::prompt_tool_permission;
use shell_approval::{
    approval_learning_target, exact_shell_approval_target, persistent_approval_target,
    tool_display_labels,
};
use vtcode_core::hooks::{LifecycleHookEngine, PreToolHookDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitlDecision {
    Approved,
    ApprovedSession,
    ApprovedPermanent,
    Denied,
    DeniedOnce,
    Exit,
    Interrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPermissionFlow {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

async fn persisted_approval_hit_key(
    tool_registry: &ToolRegistry,
    primary_target: &shell_approval::ApprovalLearningTarget,
    exact_shell_target: Option<&shell_approval::ApprovalLearningTarget>,
) -> Option<String> {
    if tool_registry
        .has_persisted_approval(&primary_target.approval_key)
        .await
    {
        return Some(primary_target.approval_key.clone());
    }

    let exact_target = exact_shell_target?;
    tool_registry
        .has_persisted_approval(&exact_target.approval_key)
        .await
        .then(|| exact_target.approval_key.clone())
}

async fn persist_approval_cache_key(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    approval_key: &str,
    log_message: &str,
) {
    if let Err(err) = tool_registry.persist_approval_cache_key(approval_key).await {
        tracing::warn!(
            tool = %tool_name,
            approval_key = %approval_key,
            error = %err,
            message = %log_message,
            "Failed to persist approval cache key"
        );
    }
}

/// Context for tool permission checks to reduce argument count
pub(crate) struct ToolPermissionsContext<'a, S: UiSession + ?Sized> {
    pub tool_registry: &'a ToolRegistry,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut S,
    pub default_placeholder: Option<String>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub hooks: Option<&'a LifecycleHookEngine>,
    pub justification: Option<&'a vtcode_core::tools::ToolJustification>,
    pub approval_recorder: Option<&'a vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger:
        Option<&'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>>,
    pub tool_permission_cache: Option<&'a Arc<RwLock<ToolPermissionCache>>>,
    pub hitl_notification_bell: bool,
    pub autonomous_mode: bool,
    pub approval_policy: AskForApproval,
    pub skip_confirmations: bool,
}

pub(crate) async fn ensure_tool_permission<S: UiSession + ?Sized>(
    ctx: ToolPermissionsContext<'_, S>,
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Result<ToolPermissionFlow> {
    let ToolPermissionsContext {
        tool_registry,
        renderer,
        handle,
        session,
        default_placeholder,
        ctrl_c_state,
        ctrl_c_notify,
        hooks,
        justification,
        approval_recorder,
        decision_ledger,
        tool_permission_cache,
        hitl_notification_bell,
        autonomous_mode,
        approval_policy,
        skip_confirmations,
    } = ctx;

    // Generate cache key - use command text for shell tools to enable granular session approval
    let cache_key = cache_key(tool_name, tool_args);

    // Check tool permission cache for persisted denials up front.
    if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;
        if permission_cache.is_denied(&cache_key) || permission_cache.is_denied(tool_name) {
            return Ok(ToolPermissionFlow::Denied);
        }
    }

    let mut hook_requires_prompt = false;

    if let Some(hooks) = hooks {
        match hooks.run_pre_tool_use(tool_name, tool_args).await {
            Ok(outcome) => {
                render_hook_messages(renderer, &outcome.messages)?;
                match outcome.decision {
                    PreToolHookDecision::Allow => {}
                    PreToolHookDecision::Deny => return Ok(ToolPermissionFlow::Denied),
                    PreToolHookDecision::Ask => {
                        hook_requires_prompt = true;
                    }
                    PreToolHookDecision::Continue => {}
                }
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to run pre-tool hooks: {}", err),
                )?;
            }
        }
    }

    let policy_decision = tool_registry.evaluate_tool_policy(tool_name).await?;

    if policy_decision == ToolPermissionDecision::Deny {
        return Ok(ToolPermissionFlow::Denied);
    }

    let normalized_tool_name = tool_args
        .and_then(|args| {
            tool_registry
                .preflight_validate_call(tool_name, args)
                .ok()
                .map(|outcome| outcome.normalized_tool_name)
        })
        .unwrap_or_else(|| tool_name.to_string());

    let persisted_shell_approval =
        persisted_shell_approval(tool_registry, &normalized_tool_name, tool_args).await;

    let mut shell_approval_reason = tool_registry
        .shell_run_approval_reason(&normalized_tool_name, tool_args)
        .await?;
    if persisted_shell_approval.is_some() {
        shell_approval_reason = None;
    }
    if let Some(reason) = shell_approval_reason.as_deref() {
        tracing::debug!(
            tool = %tool_name,
            reason = %reason,
            "Shell execution requires interactive approval"
        );
    }

    let display_labels = tool_display_labels(tool_name, tool_args);
    let approval_learning_target = approval_learning_target(
        &normalized_tool_name,
        tool_args,
        &display_labels.learning_label,
    );
    let exact_shell_approval_target = exact_shell_approval_target(
        &normalized_tool_name,
        tool_args,
        &display_labels.learning_label,
    );
    let persistent_approval_target = persistent_approval_target(
        &normalized_tool_name,
        tool_args,
        &display_labels.learning_label,
    );

    if let Some(approval_key) = persisted_approval_hit_key(
        tool_registry,
        &approval_learning_target,
        exact_shell_approval_target.as_ref(),
    )
    .await
    {
        tool_registry.mark_tool_preapproved(tool_name).await;
        if let Some(cache) = tool_permission_cache {
            let mut permission_cache = cache.write().await;
            permission_cache.cache_grant(cache_key.clone(), PermissionGrant::Permanent);
        }
        tracing::debug!(
            approval_key = %approval_key,
            "Using persisted approval cache entry"
        );
        return Ok(ToolPermissionFlow::Approved);
    }

    // Session approvals are reusable, but only after hook/policy deny checks.
    if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;
        if permission_cache.can_use_cached(&cache_key) || permission_cache.can_use_cached(tool_name)
        {
            tracing::debug!(
                "Using cached ACP permission for tool invocation: {}",
                cache_key
            );
            return Ok(ToolPermissionFlow::Approved);
        }
    }

    if skip_confirmations {
        return Ok(ToolPermissionFlow::Approved);
    }

    let should_prompt = hook_requires_prompt
        || policy_decision == ToolPermissionDecision::Prompt
        || shell_approval_reason.is_some();

    if !should_prompt {
        return Ok(ToolPermissionFlow::Approved);
    }

    let requires_rule_prompt =
        hook_requires_prompt || policy_decision == ToolPermissionDecision::Prompt;
    let requires_sandbox_prompt = shell_approval_reason.is_some();
    if approval_policy_rejects_prompt(
        approval_policy,
        requires_rule_prompt,
        requires_sandbox_prompt,
    ) {
        return Ok(ToolPermissionFlow::Denied);
    }

    let mut risk_context = build_tool_risk_context(&normalized_tool_name, tool_args);

    if let Some(recorder) = approval_recorder {
        risk_context.recent_approvals = recorder
            .get_approval_count(&approval_learning_target.approval_key)
            .await as usize;
    }
    let risk_level = ToolRiskScorer::calculate_risk(&risk_context);

    if autonomous_mode
        && trusted_auto_allows_immediate_approval(
            hook_requires_prompt,
            shell_approval_reason.as_deref(),
            &risk_context,
            risk_level,
        )
    {
        tool_registry.mark_tool_preapproved(tool_name).await;
        tracing::debug!(
            tool = %tool_name,
            risk = %risk_level,
            "Trusted Auto approved low-risk tool"
        );
        return Ok(ToolPermissionFlow::Approved);
    }

    if autonomous_mode
        && trusted_auto_allows_history_based_approval(
            hook_requires_prompt,
            shell_approval_reason.as_deref(),
            &risk_context,
            risk_level,
        )
        && let Some(recorder) = approval_recorder
        && recorder
            .supports_trusted_auto_approval(&approval_learning_target.approval_key)
            .await
    {
        tool_registry.mark_tool_preapproved(tool_name).await;
        tracing::debug!(
            approval_key = %approval_learning_target.approval_key,
            risk = %risk_level,
            "Trusted Auto approved tool from strong approval history"
        );
        return Ok(ToolPermissionFlow::Approved);
    }

    // Extract justification from decision ledger if not provided
    let extracted_justification = if justification.is_none() {
        if let Some(ledger_ref) = decision_ledger {
            let ledger = ledger_ref.read().await;
            if let Some(latest) = ledger.latest_decision() {
                JustificationExtractor::extract_from_decision(latest, tool_name, &risk_level)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    let final_justification = justification.or(extracted_justification.as_ref());
    let decision = prompt_tool_permission(
        &display_labels.prompt_label,
        tool_name,
        tool_args,
        &approval_learning_target.approval_key,
        &approval_learning_target.display_label,
        renderer,
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        shell_approval_reason.as_deref(),
        final_justification,
        Some(&persistent_approval_target),
        approval_recorder,
        hitl_notification_bell,
    )
    .await?;
    match decision {
        HitlDecision::Approved | HitlDecision::ApprovedSession => {
            let grant = if decision == HitlDecision::Approved {
                PermissionGrant::Once
            } else {
                PermissionGrant::Session
            };

            // Mark as preapproved for this execution
            tool_registry.mark_tool_preapproved(tool_name).await;

            // Cache permission grant using the granular cache_key for session/once
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(cache_key.clone(), grant);
            }

            // Record approval decision for pattern learning (fire-and-forget)
            if let Some(recorder) = approval_recorder {
                spawn_approval_record_task(recorder, &approval_learning_target, true);
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::ApprovedPermanent => {
            tool_registry.mark_tool_preapproved(tool_name).await;

            if let shell_approval::PersistentApprovalTarget::PrefixRule { prefix_rule, .. } =
                &persistent_approval_target
            {
                match persist_shell_approval_prefix_rule(
                    tool_registry,
                    tool_name,
                    tool_args,
                    prefix_rule.as_slice(),
                )
                .await
                {
                    Ok(rendered_rule) => {
                        tracing::info!(
                            tool = %tool_name,
                            prefix_rule = %rendered_rule,
                            "✓ Shell approval prefix persisted"
                        );
                    }
                    Err(err) => {
                        tracing::warn!(
                            tool = %tool_name,
                            error = %err,
                            "Failed to persist shell approval prefix"
                        );
                    }
                }
            }

            persist_approval_cache_key(
                tool_registry,
                tool_name,
                &approval_learning_target.approval_key,
                "Failed to persist approval cache entry",
            )
            .await;
            if let Some(exact_target) = exact_shell_approval_target.as_ref()
                && exact_target.approval_key != approval_learning_target.approval_key
            {
                persist_approval_cache_key(
                    tool_registry,
                    tool_name,
                    &exact_target.approval_key,
                    "Failed to persist exact shell approval cache entry",
                )
                .await;
            }

            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(cache_key.clone(), PermissionGrant::Permanent);
            }

            // Record approval decision for pattern learning
            if let Some(recorder) = approval_recorder {
                spawn_approval_record_task(recorder, &approval_learning_target, true);
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::Denied | HitlDecision::DeniedOnce => {
            if decision == HitlDecision::Denied {
                // Persist denial to policy so future runs honor the choice.
                if let Some(cache) = tool_permission_cache {
                    let mut perm_cache = cache.write().await;
                    perm_cache.cache_grant(tool_name.to_string(), PermissionGrant::Denied);
                }

                if let Err(err) = tool_registry
                    .set_tool_policy(tool_name, ToolPolicy::Deny)
                    .await
                {
                    tracing::warn!("Failed to persist denial for tool '{}': {}", tool_name, err);
                }

                if let Err(err) = tool_registry
                    .persist_mcp_tool_policy(tool_name, ToolPolicy::Deny)
                    .await
                {
                    tracing::warn!(
                        "Failed to persist MCP denial for tool '{}': {}",
                        tool_name,
                        err
                    );
                }
            }

            // Record denial decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder
                    .record_approval(
                        &approval_learning_target.approval_key,
                        Some(&approval_learning_target.display_label),
                        false,
                        None,
                    )
                    .await;
            }

            Ok(ToolPermissionFlow::Denied)
        }
        HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
        HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
    }
}

pub(crate) async fn prompt_external_tool_permission<S: UiSession + ?Sized>(
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    tool_name: &str,
    tool_args: Option<&Value>,
    display_name: &str,
    approval_learning_key: &str,
    approval_learning_label: &str,
    approval_reason: Option<&str>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    hitl_notification_bell: bool,
) -> Result<HitlDecision> {
    prompt_tool_permission(
        display_name,
        tool_name,
        tool_args,
        approval_learning_key,
        approval_learning_label,
        renderer,
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        approval_reason,
        None,
        None,
        approval_recorder,
        hitl_notification_bell,
    )
    .await
}

pub(crate) async fn prompt_session_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
) -> Result<Option<usize>> {
    limit_prompts::prompt_session_limit_increase(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        max_limit,
    )
    .await
}

pub(crate) async fn prompt_tool_loop_limit_increase<S: UiSession + ?Sized>(
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    max_limit: usize,
) -> Result<Option<usize>> {
    limit_prompts::prompt_tool_loop_limit_increase(
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        max_limit,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        ToolPermissionFlow, ToolPermissionsContext, approval_learning_target,
        approval_persistence::shell_command_has_persisted_approval_prefix,
        approval_policy_rejects_prompt, build_tool_risk_context, ensure_tool_permission,
        persist_shell_approval_prefix_rule, tool_display_labels,
        trusted_auto_allows_history_based_approval, trusted_auto_allows_immediate_approval,
    };
    use serde_json::json;
    use std::sync::Arc;
    use tokio::sync::{Notify, RwLock};
    use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
    use vtcode_core::config::constants::tools;
    use vtcode_core::config::loader::ConfigManager;
    use vtcode_core::exec_policy::{AskForApproval, RejectConfig};
    use vtcode_core::tools::RiskLevel;
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

    #[test]
    fn trusted_auto_rejects_explicit_shell_escalation() {
        let risk_context = build_tool_risk_context("read_file", None);
        assert!(!trusted_auto_allows_immediate_approval(
            false,
            Some("Command requested execution without sandbox restrictions."),
            &risk_context,
            RiskLevel::Low,
        ));
    }

    #[test]
    fn trusted_auto_rejects_high_risk_commands() {
        let tool_args = json!({
            "action": "run",
            "command": "rm -rf /tmp/demo",
        });
        let risk_context = build_tool_risk_context("unified_exec", Some(&tool_args));
        assert!(!trusted_auto_allows_immediate_approval(
            false,
            None,
            &risk_context,
            RiskLevel::High,
        ));
    }

    #[test]
    fn trusted_auto_allows_low_risk_read_only_tools() {
        let risk_context = build_tool_risk_context("read_file", None);
        assert!(trusted_auto_allows_immediate_approval(
            false,
            None,
            &risk_context,
            RiskLevel::Low,
        ));
    }

    #[test]
    fn trusted_auto_allows_medium_risk_history_reuse_only_for_safe_tools() {
        let tool_args = json!({
            "action": "grep",
            "pattern": "tool_policy",
            "path": ".",
        });
        let risk_context = build_tool_risk_context("unified_search", Some(&tool_args));
        assert!(trusted_auto_allows_history_based_approval(
            false,
            None,
            &risk_context,
            RiskLevel::Medium,
        ));
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
            true,
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
                hitl_notification_bell: false,
                autonomous_mode: false,
                approval_policy: AskForApproval::OnRequest,
                skip_confirmations: true,
            },
            tools::READ_FILE,
            None,
        )
        .await
        .expect("permission flow");

        assert_eq!(flow, ToolPermissionFlow::Denied);
    }
}
