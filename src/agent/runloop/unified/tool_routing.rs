use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task;

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::registry::{ToolPermissionDecision, ToolRegistry};
use vtcode_core::ui::tui::{InlineEvent, InlineHandle};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::CtrlCState;
use super::tool_summary::{describe_tool_action, humanize_tool_name};
use super::ui_interaction::PlaceholderGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HitlDecision {
    Approved,
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

pub(crate) async fn prompt_tool_permission(
    display_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
) -> Result<HitlDecision> {
    renderer.line_if_not_empty(MessageStyle::Info)?;

    renderer.line(
        MessageStyle::Info,
        &format!(
            "Approve '{}' tool? Respond with 'y' to approve or 'n' to deny. (Esc to cancel)",
            display_name
        ),
    )?;

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    handle.set_placeholder(Some("y/n (Esc to cancel)".to_string()));

    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = events.recv() => event,
        };

        let Some(event) = maybe_event else {
            handle.clear_input();
            if ctrl_c_state.is_cancel_requested() {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        ctrl_c_state.disarm_exit();

        match event {
            InlineEvent::Submit(input) => {
                let normalized = input.trim().to_lowercase();
                if normalized.is_empty() {
                    renderer.line(MessageStyle::Info, "Please respond with 'yes' or 'no'.")?;
                    continue;
                }

                if matches!(normalized.as_str(), "y" | "yes" | "approve" | "allow") {
                    handle.clear_input();
                    return Ok(HitlDecision::Approved);
                }

                if matches!(normalized.as_str(), "n" | "no" | "deny" | "cancel" | "stop") {
                    handle.clear_input();
                    return Ok(HitlDecision::Denied);
                }

                renderer.line(
                    MessageStyle::Info,
                    "Respond with 'yes' to approve or 'no' to deny.",
                )?;
            }
            InlineEvent::ListModalSubmit(_) | InlineEvent::ListModalCancel => {
                continue;
            }
            InlineEvent::Cancel => {
                handle.clear_input();
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Exit => {
                handle.clear_input();
                return Ok(HitlDecision::Exit);
            }
            InlineEvent::Interrupt => {
                handle.clear_input();
                return Ok(HitlDecision::Interrupt);
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => {}
        }
    }
}

pub(crate) async fn ensure_tool_permission(
    tool_registry: &mut ToolRegistry,
    tool_name: &str,
    tool_args: Option<&Value>,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    events: &mut UnboundedReceiver<InlineEvent>,
    default_placeholder: Option<String>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
) -> Result<ToolPermissionFlow> {
    match tool_registry.evaluate_tool_policy(tool_name)? {
        ToolPermissionDecision::Allow => Ok(ToolPermissionFlow::Approved),
        ToolPermissionDecision::Deny => Ok(ToolPermissionFlow::Denied),
        ToolPermissionDecision::Prompt => {
            if tool_name == tool_names::RUN_TERMINAL_CMD {
                tool_registry.mark_tool_preapproved(tool_name);
                if let Ok(manager) = tool_registry.policy_manager_mut() {
                    if let Err(err) = manager.set_policy(tool_name, ToolPolicy::Allow) {
                        tracing::warn!(
                            "Failed to persist auto-approval for '{}': {}",
                            tool_name,
                            err
                        );
                    }
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

            let decision = prompt_tool_permission(
                &prompt_label,
                renderer,
                handle,
                events,
                ctrl_c_state,
                ctrl_c_notify,
                default_placeholder,
            )
            .await?;
            match decision {
                HitlDecision::Approved => {
                    tool_registry.mark_tool_preapproved(tool_name);
                    if let Err(err) =
                        tool_registry.persist_mcp_tool_policy(tool_name, ToolPolicy::Allow)
                    {
                        tracing::warn!(
                            "Failed to persist MCP approval for tool '{}': {}",
                            tool_name,
                            err
                        );
                    }
                    Ok(ToolPermissionFlow::Approved)
                }
                HitlDecision::Denied => Ok(ToolPermissionFlow::Denied),
                HitlDecision::Exit => Ok(ToolPermissionFlow::Exit),
                HitlDecision::Interrupt => Ok(ToolPermissionFlow::Interrupted),
            }
        }
    }
}
