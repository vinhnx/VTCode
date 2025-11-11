use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::{Notify, RwLock};
use tokio::task;

use serde_json::Value;
use vtcode_core::acp::{PermissionGrant, ToolPermissionCache};
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::tools::{
    JustificationExtractor, ToolRiskContext, ToolRiskScorer, ToolSource, WorkspaceTrust,
};
use vtcode_core::ui::tui::{InlineEvent, InlineHandle};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::{CtrlCSignal, CtrlCState};
use super::tool_summary::{describe_tool_action, humanize_tool_name};
use super::ui_interaction::PlaceholderGuard;
use crate::hooks::lifecycle::{
    HookMessage, HookMessageLevel, LifecycleHookEngine, PreToolHookDecision,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitlDecision {
    Approved,
    ApprovedSession,
    ApprovedPermanent,
    Denied,
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

pub(crate) async fn prompt_tool_permission<S: UiSession + ?Sized>(
    display_name: &str,
    tool_name: &str,
    tool_args: Option<&Value>,
    _renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
) -> Result<HitlDecision> {
    use vtcode_core::ui::tui::{InlineListItem, InlineListSelection};

    // Build detailed description lines
    let mut description_lines = vec![
        format!("Tool: {}", tool_name),
        format!("Action: {}", display_name),
    ];

    // Add key arguments if available
    if let Some(args) = tool_args
        && let Some(obj) = args.as_object()
    {
        for (key, value) in obj.iter().take(3) {
            if let Some(str_val) = value.as_str() {
                let truncated = if str_val.len() > 60 {
                    format!("{}...", &str_val[..57])
                } else {
                    str_val.to_string()
                };
                description_lines.push(format!("  {}: {}", key, truncated));
            } else if let Some(bool_val) = value.as_bool() {
                description_lines.push(format!("  {}: {}", key, bool_val));
            } else if let Some(num_val) = value.as_number() {
                description_lines.push(format!("  {}: {}", key, num_val));
            }
        }
        if obj.len() > 3 {
            description_lines.push(format!("  ... and {} more arguments", obj.len() - 3));
        }
    }

    // Add agent justification if available
    if let Some(just) = justification {
        let just_lines = just.format_for_dialog();
        description_lines.extend(just_lines);
    }

    // Add approval suggestion if available
    if let Some(recorder) = approval_recorder {
        if let Some(suggestion) = recorder.get_auto_approval_suggestion(tool_name).await {
            description_lines.push(String::new());
            description_lines.push(format!("💡 {}", suggestion));
        }
    }

    description_lines.push(String::new());
    description_lines.push("Choose how to handle this tool execution:".to_string());
    description_lines.push("Use ↑↓ or Tab to navigate • Enter to select • Esc to deny".to_string());

    let options = vec![
        InlineListItem {
            title: "Approve Once".to_string(),
            subtitle: Some("Allow this tool to execute this time only".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(true)),
            search_value: Some("approve yes allow once y 1".to_string()),
        },
        InlineListItem {
            title: "Allow for Session".to_string(),
            subtitle: Some("Allow this tool for the current session".to_string()),
            badge: Some("Session".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalSession),
            search_value: Some("session temporary temp 2".to_string()),
        },
        InlineListItem {
            title: "Always Allow".to_string(),
            subtitle: Some("Permanently allow this tool (saved to policy)".to_string()),
            badge: Some("Permanent".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalPermanent),
            search_value: Some("always permanent forever save 3".to_string()),
        },
        InlineListItem {
            title: "".to_string(),
            subtitle: None,
            badge: None,
            indent: 0,
            selection: None,
            search_value: None,
        },
        InlineListItem {
            title: "Deny".to_string(),
            subtitle: Some("Reject this tool execution".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no reject cancel n 4".to_string()),
        },
    ];

    let default_selection = InlineListSelection::ToolApproval(true);

    // Show modal list with full context - arrow keys will work here and history navigation is disabled
    handle.show_list_modal(
        "Tool Permission Required".to_string(),
        description_lines,
        options,
        Some(default_selection),
        None,
    );

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_modal();
            handle.force_redraw();
            task::yield_now().await;
            tokio::time::sleep(Duration::from_millis(100)).await;
            if ctrl_c_state.is_cancel_requested() {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        match event {
            InlineEvent::Interrupt => {
                let signal = if ctrl_c_state.is_exit_requested() {
                    CtrlCSignal::Exit
                } else if ctrl_c_state.is_cancel_requested() {
                    CtrlCSignal::Cancel
                } else {
                    ctrl_c_state.register_signal()
                };
                ctrl_c_notify.notify_waiters();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return if matches!(signal, CtrlCSignal::Exit) {
                    Ok(HitlDecision::Exit)
                } else {
                    Ok(HitlDecision::Interrupt)
                };
            }
            InlineEvent::ListModalSubmit(selection) => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                // Force redraw and wait to ensure modal is fully cleared
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;

                match selection {
                    InlineListSelection::ToolApproval(true) => {
                        return Ok(HitlDecision::Approved);
                    }
                    InlineListSelection::ToolApprovalSession => {
                        return Ok(HitlDecision::ApprovedSession);
                    }
                    InlineListSelection::ToolApprovalPermanent => {
                        return Ok(HitlDecision::ApprovedPermanent);
                    }
                    InlineListSelection::ToolApproval(false) => {
                        return Ok(HitlDecision::Denied);
                    }
                    _ => {
                        return Ok(HitlDecision::Denied);
                    }
                }
            }
            InlineEvent::ListModalCancel => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Cancel => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Exit => {
                ctrl_c_state.disarm_exit();
                handle.close_modal();
                handle.force_redraw();
                task::yield_now().await;
                tokio::time::sleep(Duration::from_millis(100)).await;
                return Ok(HitlDecision::Exit);
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                ctrl_c_state.disarm_exit();
                // Ignore text input when modal is shown
                continue;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown
            | InlineEvent::FileSelected(_) => {
                ctrl_c_state.disarm_exit();
            }
        }
    }
}

pub(crate) async fn ensure_tool_permission<S: UiSession + ?Sized>(
    tool_registry: &mut ToolRegistry,
    tool_name: &str,
    tool_args: Option<&Value>,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    default_placeholder: Option<String>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    hooks: Option<&LifecycleHookEngine>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    decision_ledger: Option<
        &Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    >,
    tool_permission_cache: Option<&Arc<RwLock<ToolPermissionCache>>>,
) -> Result<ToolPermissionFlow> {
    // Check tool permission cache for previously granted permissions
    if let Some(cache) = tool_permission_cache {
        let permission_cache = cache.read().await;

        // Check if tool access is denied
        if permission_cache.is_denied(tool_name) {
            return Ok(ToolPermissionFlow::Denied);
        }

        // Check if we have cached permission that can be reused
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

    let should_prompt = hook_requires_prompt || policy_decision == ToolPermissionDecision::Prompt;

    if !should_prompt {
        return Ok(ToolPermissionFlow::Approved);
    }

    // Check approval patterns for auto-approval before prompting
    if !hook_requires_prompt {
        if let Some(recorder) = approval_recorder {
            if recorder.should_auto_approve(tool_name).await {
                tool_registry.mark_tool_preapproved(tool_name);
                tracing::debug!(
                    "Auto-approved tool '{}' based on approval pattern history",
                    tool_name
                );
                return Ok(ToolPermissionFlow::Approved);
            }
        }
    }

    if !hook_requires_prompt && tool_name == tool_names::RUN_COMMAND {
        tool_registry.mark_tool_preapproved(tool_name);
        if let Ok(manager) = tool_registry.policy_manager_mut()
            && let Err(err) = manager.set_policy(tool_name, ToolPolicy::Allow).await
        {
            tracing::warn!(
                "Failed to persist auto-approval for '{}': {}",
                tool_name,
                err
            );
        }
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
    let extracted_justification = if justification.is_none() && decision_ledger.is_some() {
        let ledger = decision_ledger.unwrap().read().await;
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
            JustificationExtractor::extract_from_decision(&latest, tool_name, &risk_level)
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
    )
    .await?;
    match decision {
        HitlDecision::Approved => {
            // One-time approval - mark as preapproved for this execution but don't persist
            tool_registry.mark_tool_preapproved(tool_name);

            // Cache permission grant for this session
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name, PermissionGrant::Once);
            }

            // Record approval decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, true, None).await;
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::ApprovedSession => {
            // Session-only approval - mark as preapproved but don't persist
            tool_registry.mark_tool_preapproved(tool_name);

            // Cache permission grant for this session
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name, PermissionGrant::Session);
            }

            // Record approval decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, true, None).await;
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::ApprovedPermanent => {
            // Permanent approval - mark and persist to policy
            tool_registry.mark_tool_preapproved(tool_name);

            // Cache permission grant permanently
            if let Some(cache) = tool_permission_cache {
                let mut perm_cache = cache.write().await;
                perm_cache.cache_grant(tool_name, PermissionGrant::Permanent);
            }

            // Try to persist to policy manager first
            if let Ok(manager) = tool_registry.policy_manager_mut()
                && let Err(err) = manager.set_policy(tool_name, ToolPolicy::Allow).await
            {
                tracing::warn!(
                    "Failed to persist permanent approval for '{}': {}",
                    tool_name,
                    err
                );
            }

            // Also try MCP tool policy persistence
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

            // Record approval decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, true, None).await;
            }

            Ok(ToolPermissionFlow::Approved)
        }
        HitlDecision::Denied => {
            // Record denial decision for pattern learning
            if let Some(recorder) = approval_recorder {
                let _ = recorder.record_approval(tool_name, false, None).await;
            }

            Ok(ToolPermissionFlow::Denied)
        }
        HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
        HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
    }
}

fn render_hook_messages(renderer: &mut AnsiRenderer, messages: &[HookMessage]) -> Result<()> {
    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }

        let style = match message.level {
            HookMessageLevel::Info => MessageStyle::Info,
            HookMessageLevel::Warning => MessageStyle::Info,
            HookMessageLevel::Error => MessageStyle::Error,
        };

        renderer.line(style, text)?;
    }

    Ok(())
}
