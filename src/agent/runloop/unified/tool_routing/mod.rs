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
use vtcode_core::command_safety::parse_bash_lc_commands;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::{PermissionMode, PermissionsConfig};
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::hooks::{
    LifecycleHookEngine, PermissionDecisionBehavior, PermissionDecisionScope, PermissionUpdateKind,
    PreToolHookDecision,
};
use vtcode_core::permissions::{
    PermissionRequest, PermissionRequestKind, build_permission_request, evaluate_permissions,
};
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::tools::{JustificationExtractor, ToolRiskScorer};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::app::InlineHandle;

use crate::agent::runloop::unified::auto_mode::{AutoModeReviewDecision, review_tool_call};
use crate::agent::runloop::unified::run_loop_context::AutoModeRuntimeContext;
use crate::agent::runloop::unified::state::SessionStats;

use super::state::CtrlCState;
use approval_cache::{cache_key, spawn_approval_record_task};
use approval_persistence::{persist_shell_approval_prefix_rule, persisted_shell_approval};
use approval_policy::{approval_policy_rejects_prompt, build_tool_risk_context};
use hook_messages::render_hook_messages;
use permission_prompt::{
    extract_shell_approval_command_words, extract_shell_permission_scope_signature,
    prompt_tool_permission,
};
use shell_approval::{
    approval_learning_target, exact_shell_approval_target, persistent_approval_target,
    tool_display_labels,
};
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolPermissionFlow {
    Approved { updated_args: Option<Value> },
    Denied,
    Blocked { reason: String },
    Exit,
    Interrupted,
}

enum AutoModePermissionOutcome {
    Allow,
    Block,
    PromptFallback,
    AbortHeadless { reason: String },
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
    pub active_thread_label: Option<&'a str>,
    pub default_placeholder: Option<String>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub hooks: Option<&'a LifecycleHookEngine>,
    pub justification: Option<&'a vtcode_core::tools::ToolJustification>,
    pub approval_recorder: Option<&'a vtcode_core::tools::ApprovalRecorder>,
    pub decision_ledger:
        Option<&'a Arc<RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>>,
    pub tool_permission_cache: Option<&'a Arc<RwLock<ToolPermissionCache>>>,
    pub permissions_state: Option<&'a Arc<RwLock<PermissionsConfig>>>,
    pub hitl_notification_bell: bool,
    pub approval_policy: AskForApproval,
    pub skip_confirmations: bool,
    pub permissions_config: Option<&'a PermissionsConfig>,
    pub auto_mode_runtime: Option<AutoModeRuntimeContext<'a>>,
    pub session_stats: Option<&'a mut SessionStats>,
}

fn current_permission_mode(config: &PermissionsConfig) -> PermissionMode {
    config.default_mode
}

fn effective_permissions_config(
    config: &PermissionsConfig,
    permission_mode: PermissionMode,
) -> Option<PermissionsConfig> {
    let mut effective = config.clone();
    if permission_mode != PermissionMode::Auto || !effective.auto_mode.drop_broad_allow_rules {
        return Some(effective);
    }

    let initial_rule_count = effective.allow.len();
    effective
        .allow
        .retain(|rule| !is_broad_auto_mode_allow_rule(rule));
    let dropped = initial_rule_count.saturating_sub(effective.allow.len());
    if dropped > 0 {
        tracing::trace!(
            dropped_broad_allow_rules = dropped,
            "auto mode filtered broad allow rules"
        );
    }
    Some(effective)
}

fn build_permission_suggestions(
    prompt_kind: permission_prompt::ToolPermissionPromptKind,
    persistent_approval_target: Option<&shell_approval::PersistentApprovalTarget>,
) -> Vec<Value> {
    let mut suggestions = vec![
        serde_json::json!({
            "id": "allow_once",
            "behavior": "allow",
            "scope": "once",
        }),
        serde_json::json!({
            "id": "allow_session",
            "behavior": "allow",
            "scope": "session",
        }),
        serde_json::json!({
            "id": "deny_once",
            "behavior": "deny",
            "scope": "once",
        }),
    ];

    if persistent_approval_target.is_some() {
        suggestions.push(serde_json::json!({
            "id": "allow_permanent",
            "behavior": "allow",
            "scope": "permanent",
        }));
    }

    if matches!(
        persistent_approval_target,
        Some(shell_approval::PersistentApprovalTarget::ToolLevel)
    ) && prompt_kind != permission_prompt::ToolPermissionPromptKind::Mcp
    {
        suggestions.push(serde_json::json!({
            "id": "deny_permanent",
            "behavior": "deny",
            "scope": "permanent",
        }));
    }

    suggestions
}

async fn apply_permission_hook_updates(
    tool_registry: &ToolRegistry,
    permissions_state: &Arc<RwLock<PermissionsConfig>>,
    behavior: PermissionDecisionBehavior,
    updates: &[vtcode_core::hooks::PermissionUpdateRequest],
) -> Vec<vtcode_core::hooks::HookMessage> {
    use std::collections::BTreeSet;
    use vtcode_core::hooks::{HookMessage, PermissionUpdateDestination, PermissionUpdateKind};

    let mut messages = Vec::new();
    if updates.is_empty() {
        return messages;
    }

    let mut next_permissions = permissions_state.read().await.clone();
    let mut changed = false;
    let mut persist_project = false;

    for update in updates {
        match (&update.destination, &update.kind) {
            (PermissionUpdateDestination::Unsupported(destination), _) => {
                messages.push(HookMessage::warning(format!(
                    "PermissionRequest hook ignored unsupported destination `{destination}`"
                )));
            }
            (_, PermissionUpdateKind::Unsupported(field)) => {
                messages.push(HookMessage::warning(format!(
                    "PermissionRequest hook ignored unsupported permission update `{field}`"
                )));
            }
            (destination, PermissionUpdateKind::SetMode(mode)) => {
                next_permissions.default_mode = *mode;
                changed = true;
                persist_project |=
                    matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
            (destination, PermissionUpdateKind::AddRules(rules)) => {
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut next_permissions.allow,
                    PermissionDecisionBehavior::Deny => &mut next_permissions.deny,
                };
                let mut seen = target.iter().cloned().collect::<BTreeSet<_>>();
                for rule in rules {
                    if seen.insert(rule.clone()) {
                        target.push(rule.clone());
                        changed = true;
                    }
                }
                persist_project |=
                    matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
            (destination, PermissionUpdateKind::ReplaceRules(rules)) => {
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut next_permissions.allow,
                    PermissionDecisionBehavior::Deny => &mut next_permissions.deny,
                };
                if *target != *rules {
                    *target = rules.clone();
                    changed = true;
                }
                persist_project |=
                    matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
            (destination, PermissionUpdateKind::RemoveRules(rules)) => {
                let target = match behavior {
                    PermissionDecisionBehavior::Allow => &mut next_permissions.allow,
                    PermissionDecisionBehavior::Deny => &mut next_permissions.deny,
                };
                let initial_len = target.len();
                target.retain(|rule| !rules.iter().any(|candidate| candidate == rule));
                changed |= target.len() != initial_len;
                persist_project |=
                    matches!(destination, PermissionUpdateDestination::ProjectSettings);
            }
        }
    }

    if !changed {
        return messages;
    }

    {
        let mut state = permissions_state.write().await;
        *state = next_permissions.clone();
    }
    tool_registry.apply_permissions_config(&next_permissions);

    if persist_project {
        let workspace_root = tool_registry.workspace_root().clone();
        match ConfigManager::load_from_workspace(&workspace_root) {
            Ok(mut manager) => {
                let mut config = manager.config().clone();
                config.permissions = next_permissions;
                if let Err(err) = manager.save_config(&config) {
                    messages.push(HookMessage::error(format!(
                        "PermissionRequest hook failed to persist project settings: {err}"
                    )));
                }
            }
            Err(err) => messages.push(HookMessage::error(format!(
                "PermissionRequest hook failed to load project configuration: {err}"
            ))),
        }
    }

    messages
}

#[allow(clippy::too_many_arguments)]
async fn finalize_permission_decision(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
    cache_key: &str,
    tool_permission_cache: Option<&Arc<RwLock<ToolPermissionCache>>>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    approval_learning_target: &shell_approval::ApprovalLearningTarget,
    exact_shell_approval_target: Option<&shell_approval::ApprovalLearningTarget>,
    persistent_approval_target: &shell_approval::PersistentApprovalTarget,
    decision: HitlDecision,
    updated_args: Option<Value>,
) -> Result<ToolPermissionFlow> {
    match decision {
        HitlDecision::Approved | HitlDecision::ApprovedSession => {
            let grant = if decision == HitlDecision::Approved {
                PermissionGrant::Once
            } else {
                PermissionGrant::Session
            };

            tool_registry.mark_tool_preapproved(tool_name).await;

            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(cache_key.to_string(), grant);
            }

            if let Some(recorder) = approval_recorder {
                spawn_approval_record_task(recorder, approval_learning_target, true);
            }

            Ok(ToolPermissionFlow::Approved { updated_args })
        }
        HitlDecision::ApprovedPermanent => {
            tool_registry.mark_tool_preapproved(tool_name).await;

            if let shell_approval::PersistentApprovalTarget::PrefixRule { prefix_rule, .. } =
                persistent_approval_target
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
            if let Some(exact_target) = exact_shell_approval_target
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
            persist_segment_approval_cache_keys(
                tool_registry,
                tool_name,
                normalized_tool_name,
                tool_args,
            )
            .await;

            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(cache_key.to_string(), PermissionGrant::Permanent);
            }

            if let Some(recorder) = approval_recorder {
                spawn_approval_record_task(recorder, approval_learning_target, true);
            }

            Ok(ToolPermissionFlow::Approved { updated_args })
        }
        HitlDecision::Denied | HitlDecision::DeniedOnce => {
            if decision == HitlDecision::Denied {
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

fn is_broad_auto_mode_allow_rule(rule: &str) -> bool {
    let normalized = rule.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }

    if matches!(
        normalized.as_str(),
        "bash" | "bash(*)" | "unified_exec" | "run_pty_cmd" | "agent"
    ) {
        return true;
    }

    let interpreter_prefixes = [
        "bash(python",
        "bash(python3",
        "bash(node",
        "bash(ruby",
        "bash(bash",
        "bash(sh",
        "bash(zsh",
        "bash(fish",
        "bash(pwsh",
        "bash(powershell",
    ];
    if interpreter_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return true;
    }

    [
        "bash(npm run",
        "bash(pnpm run",
        "bash(yarn run",
        "bash(cargo run",
        "bash(uv run",
    ]
    .iter()
    .any(|prefix| normalized.starts_with(prefix))
}

fn auto_mode_safe_builtin_allow(
    workspace_root: &std::path::Path,
    request: &PermissionRequest,
) -> bool {
    match &request.kind {
        PermissionRequestKind::Read { .. } => true,
        PermissionRequestKind::Edit { paths } | PermissionRequestKind::Write { paths } => {
            request.builtin_file_mutation
                && request.protected_write_paths.is_empty()
                && !paths.is_empty()
                && paths
                    .iter()
                    .all(|path| path.strip_prefix(workspace_root).is_ok())
        }
        PermissionRequestKind::Bash { .. }
        | PermissionRequestKind::WebFetch { .. }
        | PermissionRequestKind::Mcp { .. }
        | PermissionRequestKind::Other => false,
    }
}

fn headless_auto_mode_fallback_reason(
    tool_name: &str,
    denial: Option<&crate::agent::runloop::unified::state::AutoModeDenial>,
) -> String {
    let Some(denial) = denial else {
        return format!(
            "Auto mode cannot fall back to manual prompts for `{tool_name}` in non-interactive mode."
        );
    };

    let mut reason = format!(
        "Auto mode blocked `{tool_name}` and reached its denial threshold in non-interactive mode: {}",
        denial.reason
    );
    if let Some(rule) = denial.matched_rule.as_deref() {
        reason.push_str(&format!(" (matched rule: {rule})"));
    }
    if let Some(exception) = denial.matched_exception.as_deref() {
        reason.push_str(&format!(" (matched exception: {exception})"));
    }
    reason
}

async fn resolve_auto_mode_permission(
    renderer: &mut AnsiRenderer,
    tool_registry: &ToolRegistry,
    tool_name: &str,
    tool_args: Option<&Value>,
    permission_request: &PermissionRequest,
    permissions: &PermissionsConfig,
    auto_mode_runtime: Option<AutoModeRuntimeContext<'_>>,
    session_stats: Option<&mut SessionStats>,
) -> Result<AutoModePermissionOutcome> {
    let Some(stats) = session_stats else {
        tracing::warn!(tool = %tool_name, "auto mode reviewer missing session stats");
        return Ok(AutoModePermissionOutcome::PromptFallback);
    };

    if stats.auto_mode_prompt_fallback_active() {
        tracing::trace!(tool = %tool_name, "auto mode prompt fallback active");
        if !renderer.supports_inline_ui() {
            return Ok(AutoModePermissionOutcome::AbortHeadless {
                reason: headless_auto_mode_fallback_reason(
                    tool_name,
                    stats.last_auto_mode_denial(),
                ),
            });
        }
        return Ok(AutoModePermissionOutcome::PromptFallback);
    }

    let Some(auto_mode_runtime) = auto_mode_runtime else {
        tracing::warn!(tool = %tool_name, "auto mode reviewer missing runtime context");
        return Ok(AutoModePermissionOutcome::PromptFallback);
    };

    match review_tool_call(
        auto_mode_runtime.provider_client,
        auto_mode_runtime.config,
        auto_mode_runtime.vt_cfg,
        permissions,
        tool_registry.workspace_root(),
        auto_mode_runtime.working_history,
        tool_name,
        tool_args,
        permission_request,
    )
    .await
    {
        Ok(AutoModeReviewDecision::Allow { stage }) => {
            tool_registry.mark_tool_preapproved(tool_name).await;
            stats.record_auto_mode_allow();
            tracing::trace!(tool = %tool_name, stage, "auto mode approved tool");
            Ok(AutoModePermissionOutcome::Allow)
        }
        Ok(AutoModeReviewDecision::Block(denial)) => {
            let fallback_was_active = stats.auto_mode_prompt_fallback_active();
            let fallback = stats.record_auto_mode_denial(
                denial.clone(),
                permissions.auto_mode.max_consecutive_denials,
                permissions.auto_mode.max_total_denials,
            );
            tracing::trace!(
                tool = %tool_name,
                stage = denial.stage,
                matched_rule = denial.matched_rule.as_deref().unwrap_or(""),
                matched_exception = denial.matched_exception.as_deref().unwrap_or(""),
                fallback,
                "auto mode blocked tool"
            );

            if fallback && !fallback_was_active {
                if !renderer.supports_inline_ui() {
                    return Ok(AutoModePermissionOutcome::AbortHeadless {
                        reason: headless_auto_mode_fallback_reason(
                            tool_name,
                            stats.last_auto_mode_denial(),
                        ),
                    });
                }
                renderer.line(
                    MessageStyle::Info,
                    "Auto mode fell back to manual prompts after repeated classifier denials.",
                )?;
            }

            Ok(AutoModePermissionOutcome::Block)
        }
        Err(err) => {
            tracing::warn!(tool = %tool_name, error = %err, "auto mode reviewer failed");
            Ok(AutoModePermissionOutcome::PromptFallback)
        }
    }
}

fn segmented_shell_approval_keys(
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<Vec<String>> {
    let scope_signature =
        extract_shell_permission_scope_signature(normalized_tool_name, tool_args)?;
    let command_words = extract_shell_approval_command_words(normalized_tool_name, tool_args)?;
    let segments = parse_bash_lc_commands(&command_words).or_else(|| {
        vtcode_core::command_safety::shell_parser::parse_shell_commands(&shell_words::join(
            command_words.iter().map(String::as_str),
        ))
        .ok()
    })?;

    let keys = segments
        .into_iter()
        .filter(|segment| !segment.is_empty())
        .take(5)
        .map(|segment| {
            let rendered = shell_words::join(segment.iter().map(String::as_str));
            format!("{rendered}|{scope_signature}")
        })
        .collect::<Vec<_>>();

    (!keys.is_empty()).then_some(keys)
}

async fn persisted_segment_approval_hit_key(
    tool_registry: &ToolRegistry,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let keys = segmented_shell_approval_keys(normalized_tool_name, tool_args)?;

    for key in &keys {
        if !tool_registry.has_persisted_approval(key).await {
            return None;
        }
    }

    Some(keys.join(", "))
}

async fn persist_segment_approval_cache_keys(
    tool_registry: &ToolRegistry,
    tool_name: &str,
    normalized_tool_name: &str,
    tool_args: Option<&Value>,
) {
    let Some(approval_keys) = segmented_shell_approval_keys(normalized_tool_name, tool_args) else {
        return;
    };

    for approval_key in approval_keys {
        persist_approval_cache_key(
            tool_registry,
            tool_name,
            &approval_key,
            "Failed to persist segmented shell approval cache entry",
        )
        .await;
    }
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
        active_thread_label,
        default_placeholder,
        ctrl_c_state,
        ctrl_c_notify,
        hooks,
        justification,
        approval_recorder,
        decision_ledger,
        tool_permission_cache,
        permissions_state,
        hitl_notification_bell,
        approval_policy,
        skip_confirmations,
        permissions_config,
        auto_mode_runtime,
        session_stats,
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

    let normalized_tool_name = tool_args
        .and_then(|args| {
            tool_registry
                .preflight_validate_call(tool_name, args)
                .ok()
                .map(|outcome| outcome.normalized_tool_name)
        })
        .unwrap_or_else(|| tool_name.to_string());

    let current_dir =
        std::env::current_dir().unwrap_or_else(|_| tool_registry.workspace_root().clone());
    let permissions_snapshot = if let Some(state) = permissions_state {
        state.read().await.clone()
    } else {
        permissions_config.cloned().unwrap_or_default()
    };
    let permission_mode = current_permission_mode(&permissions_snapshot);
    let effective_permissions =
        effective_permissions_config(&permissions_snapshot, permission_mode);
    let permission_request = build_permission_request(
        tool_registry.workspace_root(),
        &current_dir,
        &normalized_tool_name,
        tool_args,
    );
    let permission_matches = effective_permissions
        .as_ref()
        .map(|config| {
            evaluate_permissions(
                config,
                tool_registry.workspace_root(),
                &current_dir,
                &permission_request,
            )
        })
        .unwrap_or_default();

    if permission_matches.deny {
        return Ok(ToolPermissionFlow::Denied);
    }
    let requires_protected_write_prompt = permission_request.requires_protected_write_prompt();
    let requires_rule_prompt =
        hook_requires_prompt || permission_matches.ask || requires_protected_write_prompt;
    let auto_mode_classifier_review = permission_mode == PermissionMode::Auto
        && !requires_rule_prompt
        && !auto_mode_safe_builtin_allow(tool_registry.workspace_root(), &permission_request);
    let policy_decision = tool_registry.evaluate_tool_policy(tool_name).await?;
    if policy_decision == ToolPermissionDecision::Deny {
        return Ok(ToolPermissionFlow::Denied);
    }

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

    let requires_sandbox_prompt = shell_approval_reason.is_some() && !auto_mode_classifier_review;
    let can_reuse_saved_approval = !requires_rule_prompt && !auto_mode_classifier_review;

    if can_reuse_saved_approval
        && let Some(approval_key) =
            persisted_segment_approval_hit_key(tool_registry, &normalized_tool_name, tool_args)
                .await
    {
        tool_registry.mark_tool_preapproved(tool_name).await;
        if let Some(cache) = tool_permission_cache {
            let mut permission_cache = cache.write().await;
            permission_cache.cache_grant(cache_key.clone(), PermissionGrant::Permanent);
        }
        tracing::debug!(
            approval_key = %approval_key,
            "Using persisted segmented shell approval cache entries"
        );
        return Ok(ToolPermissionFlow::Approved { updated_args: None });
    }

    if can_reuse_saved_approval
        && let Some(approval_key) = persisted_approval_hit_key(
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
        return Ok(ToolPermissionFlow::Approved { updated_args: None });
    }

    // Session approvals are reusable, but only after hook/policy deny checks.
    if can_reuse_saved_approval && let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;
        if permission_cache.can_use_cached(&cache_key) || permission_cache.can_use_cached(tool_name)
        {
            tracing::debug!(
                "Using cached ACP permission for tool invocation: {}",
                cache_key
            );
            return Ok(ToolPermissionFlow::Approved { updated_args: None });
        }
    }

    if !requires_rule_prompt && !requires_sandbox_prompt {
        if permission_matches.allow {
            return Ok(ToolPermissionFlow::Approved { updated_args: None });
        }

        if permission_mode == PermissionMode::Auto
            && auto_mode_safe_builtin_allow(tool_registry.workspace_root(), &permission_request)
        {
            return Ok(ToolPermissionFlow::Approved { updated_args: None });
        }

        if permission_mode == PermissionMode::AcceptEdits
            && permission_request.builtin_file_mutation
        {
            return Ok(ToolPermissionFlow::Approved { updated_args: None });
        }

        if permission_mode == PermissionMode::BypassPermissions {
            return Ok(ToolPermissionFlow::Approved { updated_args: None });
        }
    }

    if permission_mode == PermissionMode::DontAsk {
        return Ok(ToolPermissionFlow::Denied);
    }

    if policy_decision == ToolPermissionDecision::Allow
        && !requires_rule_prompt
        && !requires_sandbox_prompt
        && !auto_mode_classifier_review
    {
        return Ok(ToolPermissionFlow::Approved { updated_args: None });
    }

    if skip_confirmations {
        return Ok(ToolPermissionFlow::Approved { updated_args: None });
    }

    let mut requires_auto_fallback_prompt = false;
    if auto_mode_classifier_review {
        match resolve_auto_mode_permission(
            renderer,
            tool_registry,
            &normalized_tool_name,
            tool_args,
            &permission_request,
            &permissions_snapshot,
            auto_mode_runtime,
            session_stats,
        )
        .await?
        {
            AutoModePermissionOutcome::Allow => {
                return Ok(ToolPermissionFlow::Approved { updated_args: None });
            }
            AutoModePermissionOutcome::Block => return Ok(ToolPermissionFlow::Denied),
            AutoModePermissionOutcome::PromptFallback => {
                requires_auto_fallback_prompt = true;
            }
            AutoModePermissionOutcome::AbortHeadless { reason } => {
                return Ok(ToolPermissionFlow::Blocked { reason });
            }
        }
    }

    let should_prompt = requires_rule_prompt
        || requires_sandbox_prompt
        || requires_auto_fallback_prompt
        || (policy_decision == ToolPermissionDecision::Prompt && !auto_mode_classifier_review);
    if !should_prompt {
        return Ok(ToolPermissionFlow::Approved { updated_args: None });
    }

    if approval_policy_rejects_prompt(
        approval_policy,
        requires_rule_prompt
            || requires_auto_fallback_prompt
            || policy_decision == ToolPermissionDecision::Prompt,
        requires_sandbox_prompt,
    ) {
        return Ok(ToolPermissionFlow::Denied);
    }

    let prompt_kind = permission_prompt::tool_permission_prompt_kind(tool_name);
    let permission_suggestions =
        build_permission_suggestions(prompt_kind, Some(&persistent_approval_target));
    if let Some(hooks) = hooks {
        match hooks
            .run_permission_request(
                tool_name,
                tool_args,
                &permission_request,
                &permission_suggestions,
            )
            .await
        {
            Ok(outcome) => {
                render_hook_messages(renderer, &outcome.messages)?;
                if let Some(decision) = outcome.decision {
                    let update_messages = if let Some(permissions_state) = permissions_state {
                        let update_messages = apply_permission_hook_updates(
                            tool_registry,
                            permissions_state,
                            decision.behavior,
                            &decision.permission_updates,
                        )
                        .await;
                        if decision
                            .permission_updates
                            .iter()
                            .any(|update| matches!(update.kind, PermissionUpdateKind::SetMode(_)))
                        {
                            let current_mode = permissions_state.read().await.default_mode;
                            hooks.update_permission_mode(current_mode).await;
                        }
                        update_messages
                    } else if !decision.permission_updates.is_empty() {
                        vec![vtcode_core::hooks::HookMessage::warning(
                            "PermissionRequest hook returned permission updates without runtime permission state; ignoring updates.",
                        )]
                    } else {
                        Vec::new()
                    };
                    render_hook_messages(renderer, &update_messages)?;

                    let mapped = match (decision.behavior, decision.scope, decision.interrupt) {
                        (PermissionDecisionBehavior::Allow, PermissionDecisionScope::Once, _) => {
                            HitlDecision::Approved
                        }
                        (
                            PermissionDecisionBehavior::Allow,
                            PermissionDecisionScope::Session,
                            _,
                        ) => HitlDecision::ApprovedSession,
                        (
                            PermissionDecisionBehavior::Allow,
                            PermissionDecisionScope::Permanent,
                            _,
                        ) => HitlDecision::ApprovedPermanent,
                        (PermissionDecisionBehavior::Deny, _, true) => HitlDecision::Interrupt,
                        (
                            PermissionDecisionBehavior::Deny,
                            PermissionDecisionScope::Permanent,
                            _,
                        ) => HitlDecision::Denied,
                        (PermissionDecisionBehavior::Deny, _, false) => HitlDecision::DeniedOnce,
                    };

                    return finalize_permission_decision(
                        tool_registry,
                        tool_name,
                        &normalized_tool_name,
                        tool_args,
                        &cache_key,
                        tool_permission_cache,
                        approval_recorder,
                        &approval_learning_target,
                        exact_shell_approval_target.as_ref(),
                        &persistent_approval_target,
                        mapped,
                        decision.updated_input,
                    )
                    .await;
                }
            }
            Err(err) => renderer.line(
                MessageStyle::Error,
                &format!("Failed to run permission request hooks: {}", err),
            )?,
        }
    }

    let mut risk_context = build_tool_risk_context(&normalized_tool_name, tool_args);

    if let Some(recorder) = approval_recorder {
        risk_context.recent_approvals = recorder
            .get_approval_count(&approval_learning_target.approval_key)
            .await as usize;
    }
    let risk_level = ToolRiskScorer::calculate_risk(&risk_context);

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
        active_thread_label.filter(|label| *label != "main"),
    )
    .await?;
    finalize_permission_decision(
        tool_registry,
        tool_name,
        &normalized_tool_name,
        tool_args,
        &cache_key,
        tool_permission_cache,
        approval_recorder,
        &approval_learning_target,
        exact_shell_approval_target.as_ref(),
        &persistent_approval_target,
        decision,
        None,
    )
    .await
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
    active_thread_label: Option<&str>,
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
        active_thread_label.filter(|label| *label != "main"),
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
        AutoModeRuntimeContext, SessionStats, ToolPermissionFlow, ToolPermissionsContext,
        approval_learning_target,
        approval_persistence::shell_command_has_persisted_approval_prefix,
        approval_policy_rejects_prompt, ensure_tool_permission,
        persist_segment_approval_cache_keys, persist_shell_approval_prefix_rule,
        tool_display_labels,
    };
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
    use vtcode_core::config::types::{
        ModelSelectionSource, ReasoningEffortLevel, UiSurfacePreference,
    };
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
            model: vtcode_core::config::constants::models::google::GEMINI_3_FLASH_PREVIEW
                .to_string(),
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

        async fn generate(
            &self,
            _request: uni::LLMRequest,
        ) -> Result<uni::LLMResponse, uni::LLMError> {
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
            line.contains(
                "Auto mode fell back to manual prompts after repeated classifier denials.",
            )
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
}
