#![allow(clippy::too_many_arguments)]
mod hook_messages;
mod limit_prompts;
mod permission_prompt;

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};

use serde_json::Value;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::tools::{
    JustificationExtractor, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust,
};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::InlineHandle;

use super::state::CtrlCState;
use super::tool_summary::{describe_tool_action, humanize_tool_name};
use crate::hooks::lifecycle::{LifecycleHookEngine, PreToolHookDecision};
use hook_messages::render_hook_messages;
use permission_prompt::{
    extract_shell_command_text, prompt_tool_permission, shell_command_requires_prompt,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPermissionFlow {
    Approved,
    Denied,
    Exit,
    Interrupted,
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
    pub human_in_the_loop: bool,
    pub delegate_mode: bool,
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
        human_in_the_loop,
        delegate_mode,
        skip_confirmations,
    } = ctx;

    if skip_confirmations {
        return Ok(ToolPermissionFlow::Approved);
    }

    if delegate_mode {
        renderer.line(
            MessageStyle::Info,
            "Delegate mode active. Tool execution is disabled.",
        )?;
        return Ok(ToolPermissionFlow::Denied);
    }

    if !human_in_the_loop {
        return Ok(ToolPermissionFlow::Approved);
    }

    // Autonomous mode auto-approval for safe tools
    if autonomous_mode && !tool_registry.is_mutating_tool(tool_name) {
        tracing::debug!(
            "Auto-approving safe tool '{}' in autonomous mode",
            tool_name
        );
        return Ok(ToolPermissionFlow::Approved);
    }

    // Check tool permission cache for previously granted permissions
    if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;

        // Check if tool access is denied by policy (not execution failure)
        // Only reject on explicit policy denials, not temporary execution failures
        if permission_cache.is_denied(tool_name) {
            return Ok(ToolPermissionFlow::Denied);
        }

        // Check if we have cached permission that can be reused
        // Temporary denials are NOT reusable; they should be retried
        if permission_cache.can_use_cached(tool_name) {
            tracing::debug!("Using cached ACP permission for tool: {}", tool_name);
            return Ok(ToolPermissionFlow::Approved);
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

    let command_requires_prompt = extract_shell_command_text(tool_name, tool_args)
        .map(|command| {
            let requires_prompt = shell_command_requires_prompt(&command);
            if requires_prompt {
                tracing::debug!(
                    "Command '{}' requires interactive approval for tool '{}'",
                    command,
                    tool_name
                );
            }
            requires_prompt
        })
        .unwrap_or(false);

    let should_prompt = hook_requires_prompt
        || policy_decision == ToolPermissionDecision::Prompt
        || command_requires_prompt;

    if !should_prompt {
        return Ok(ToolPermissionFlow::Approved);
    }

    // Check approval patterns for auto-approval before prompting
    if !hook_requires_prompt
        && let Some(recorder) = approval_recorder
        && recorder.should_auto_approve(tool_name).await
    {
        tool_registry.mark_tool_preapproved(tool_name).await;
        tracing::debug!(
            "Auto-approved tool '{}' based on approval pattern history",
            tool_name
        );
        return Ok(ToolPermissionFlow::Approved);
    }

    let (headline, _) = tool_args
        .map(|args| describe_tool_action(tool_name, args))
        .unwrap_or_else(|| (humanize_tool_name(tool_name), HashSet::new()));
    let prompt_label = if headline.is_empty() {
        humanize_tool_name(tool_name)
    } else {
        headline
    };

    // Extract justification from decision ledger if not provided
    let extracted_justification = if justification.is_none() {
        if let Some(ledger_ref) = decision_ledger {
            let ledger = ledger_ref.read().await;
            if let Some(latest) = ledger.latest_decision() {
                // Calculate risk level for this tool
                let mut risk_context = ToolRiskContext::new(
                    tool_name.to_string(),
                    ToolSource::Internal,
                    WorkspaceTrust::Untrusted,
                );
                if let Some(args) = tool_args {
                    risk_context.command_args = vec![args.to_string()];
                }
                let risk_level = ToolRiskScorer::calculate_risk(&risk_context);
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
        &prompt_label,
        tool_name,
        tool_args,
        renderer,
        handle,
        session,
        ctrl_c_state,
        ctrl_c_notify,
        default_placeholder,
        final_justification,
        approval_recorder,
        hitl_notification_bell,
    )
    .await?;
    match decision {
        HitlDecision::Approved => {
            // One-time approval - mark as preapproved for this execution but don't persist
            tool_registry.mark_tool_preapproved(tool_name).await;

            // Cache permission grant for this session
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name.to_string(), PermissionGrant::Once);
            }

            // Record approval decision for pattern learning (fire-and-forget to avoid UI stalls)
            if let Some(recorder) = approval_recorder {
                let tool_name_owned = tool_name.to_string();
                let recorder_cloned = recorder.clone();
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(500),
                        recorder_cloned.record_approval(&tool_name_owned, true, None),
                    )
                    .await;
                });
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::ApprovedSession => {
            // Session-only approval - mark as preapproved but don't persist
            tool_registry.mark_tool_preapproved(tool_name).await;

            // Cache permission grant for this session
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name.to_string(), PermissionGrant::Session);
            }

            // Record approval decision for pattern learning (fire-and-forget)
            if let Some(recorder) = approval_recorder {
                let tool_name_owned = tool_name.to_string();
                let recorder_cloned = recorder.clone();
                tokio::spawn(async move {
                    let _ = tokio::time::timeout(
                        Duration::from_millis(500),
                        recorder_cloned.record_approval(&tool_name_owned, true, None),
                    )
                    .await;
                });
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::ApprovedPermanent => {
            // Permanent approval - mark and persist to policy SYNCHRONOUSLY
            tool_registry.mark_tool_preapproved(tool_name).await;
            tracing::info!("✓ Tool '{}' marked as preapproved", tool_name);

            // Cache permission grant permanently (synchronous - immediate effect)
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name.to_string(), PermissionGrant::Permanent);
                tracing::info!("✓ Tool '{}' cached as permanently approved", tool_name);
            }

            // Persist to policy manager IMMEDIATELY (not in background)
            // This ensures the policy is saved before execution continues
            if let Err(err) = tool_registry
                .set_tool_policy(tool_name, ToolPolicy::Allow)
                .await
            {
                tracing::warn!(
                    "Failed to persist permanent approval for '{}': {}",
                    tool_name,
                    err
                );
            } else {
                tracing::info!("✓ Policy persisted for '{}'", tool_name);
            }

            // Also persist MCP tool policy
            if let Err(err) = tool_registry
                .persist_mcp_tool_policy(tool_name, ToolPolicy::Allow)
                .await
            {
                tracing::warn!(
                    "Failed to persist MCP approval for tool '{}': {}",
                    tool_name,
                    err
                );
            }

            // Record approval decision for pattern learning (fire-and-forget)
            if let Some(recorder) = approval_recorder {
                let tool_name_owned = tool_name.to_string();
                let recorder_cloned = recorder.clone();
                tokio::spawn(async move {
                    match tokio::time::timeout(
                        Duration::from_millis(500),
                        recorder_cloned.record_approval(&tool_name_owned, true, None),
                    )
                    .await
                    {
                        Ok(Ok(())) => tracing::info!(
                            "✓ Tool '{}' approval recorded (background)",
                            tool_name_owned
                        ),
                        Ok(Err(err)) => tracing::warn!(
                            "[background] Failed to record approval for '{}': {}",
                            tool_name_owned,
                            err
                        ),
                        Err(_) => tracing::warn!(
                            "[background] Timed out recording approval for '{}'",
                            tool_name_owned
                        ),
                    }
                });
            }

            tracing::info!(
                "✓ Returning ToolPermissionFlow::Approved for '{}'",
                tool_name
            );
            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::Denied => {
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

            // Record denial decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, false, None).await;
            }

            Ok(ToolPermissionFlow::Denied)
        }
        HitlDecision::DeniedOnce => {
            // Record denial decision for pattern learning without persisting policy.
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, false, None).await;
            }

            Ok(ToolPermissionFlow::Denied)
        }
        HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
        HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
    }
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
