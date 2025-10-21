use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Notify;
use tokio::task;

use serde_json::Value;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::core::interfaces::ui::UiSession;
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

pub(crate) async fn prompt_tool_permission<S: UiSession + ?Sized>(
    display_name: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
) -> Result<HitlDecision> {
    use vtcode_core::ui::tui::{InlineListItem, InlineListSelection};

    renderer.line_if_not_empty(MessageStyle::Info)?;

    let prompt_lines = vec![
        format!("The agent wants to use: {}", display_name),
        String::new(),
        "Use ↑/↓ or Tab to navigate • Enter to select • Esc to cancel".to_string(),
    ];
    
    let options = vec![
        InlineListItem {
            title: "Approve".to_string(),
            subtitle: Some("Allow this tool to execute".to_string()),
            badge: Some("✓".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(true)),
            search_value: Some("approve yes allow y".to_string()),
        },
        InlineListItem {
            title: "Deny".to_string(),
            subtitle: Some("Reject this tool execution".to_string()),
            badge: Some("✗".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no reject cancel n".to_string()),
        },
    ];

    let default_selection = InlineListSelection::ToolApproval(true);

    handle.show_list_modal(
        "Tool Permission Required".to_string(),
        prompt_lines,
        options,
        Some(default_selection),
        None,
    );

    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    task::yield_now().await;

    loop {
        if ctrl_c_state.is_cancel_requested() {
            handle.close_modal();
            return Ok(HitlDecision::Interrupt);
        }

        let notify = ctrl_c_notify.clone();
        let maybe_event = tokio::select! {
            _ = notify.notified() => None,
            event = session.next_event() => event,
        };

        let Some(event) = maybe_event else {
            handle.close_modal();
            handle.clear_input();
            if ctrl_c_state.is_cancel_requested() {
                return Ok(HitlDecision::Interrupt);
            }
            return Ok(HitlDecision::Exit);
        };

        ctrl_c_state.disarm_exit();

        match event {
            InlineEvent::ListModalSubmit(selection) => {
                handle.close_modal();
                handle.clear_input();
                
                match selection {
                    InlineListSelection::ToolApproval(true) => return Ok(HitlDecision::Approved),
                    InlineListSelection::ToolApproval(false) => return Ok(HitlDecision::Denied),
                    _ => return Ok(HitlDecision::Denied),
                }
            }
            InlineEvent::ListModalCancel => {
                handle.close_modal();
                handle.clear_input();
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Cancel => {
                handle.close_modal();
                handle.clear_input();
                return Ok(HitlDecision::Denied);
            }
            InlineEvent::Exit => {
                handle.close_modal();
                handle.clear_input();
                return Ok(HitlDecision::Exit);
            }
            InlineEvent::Interrupt => {
                handle.close_modal();
                handle.clear_input();
                return Ok(HitlDecision::Interrupt);
            }
            InlineEvent::Submit(_) | InlineEvent::QueueSubmit(_) => {
                // Ignore text input when modal is shown
                continue;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown => {}
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
                session,
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
