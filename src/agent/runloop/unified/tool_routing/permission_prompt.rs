use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::task;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::ui::tui::InlineEvent;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::AnsiRenderer;

use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
use crate::agent::runloop::unified::ui_interaction::PlaceholderGuard;

use super::HitlDecision;

pub(super) fn extract_shell_command_text(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = tool_args?;
    let command_value = match tool_name {
        "run_pty_cmd" | "shell" => args.get("command").or_else(|| args.get("raw_command")),
        "unified_exec" | "exec_pty_cmd" | "exec" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .or_else(|| args.get("command").map(|_| "run"));
            if action == Some("run") {
                args.get("command").or_else(|| args.get("raw_command"))
            } else {
                None
            }
        }
        _ => None,
    }?;

    if let Some(arr) = command_value.as_array() {
        let parts = arr
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    } else {
        command_value.as_str().map(|value| value.to_owned())
    }
}

pub(super) fn shell_command_requires_prompt(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return false;
    }

    let tokens = shell_words::split(trimmed).unwrap_or_else(|_| {
        trimmed
            .split_whitespace()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
    });
    if tokens.is_empty() {
        return false;
    }

    vtcode_core::command_safety::command_might_be_dangerous(&tokens)
}

pub(super) async fn prompt_tool_permission<S: UiSession + ?Sized>(
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
    hitl_notification_bell: bool,
) -> Result<HitlDecision> {
    use vtcode_core::ui::tui::{InlineListItem, InlineListSelection};

    let mut description_lines = vec![
        format!("Tool: {}", tool_name),
        format!("Action: {}", display_name),
    ];

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

    if let Some(just) = justification {
        let just_lines = just.format_for_dialog();
        description_lines.extend(just_lines);
    }

    if let Some(recorder) = approval_recorder
        && let Some(suggestion) = recorder.get_auto_approval_suggestion(tool_name).await
    {
        description_lines.push(String::new());
        description_lines.push(format!("Suggestion: {}", suggestion));
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
            title: "Deny Once".to_string(),
            subtitle: Some("Reject this tool for now (ask again next time)".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalDenyOnce),
            search_value: Some("deny no reject once temporary 4".to_string()),
        },
        InlineListItem {
            title: "Always Deny".to_string(),
            subtitle: Some("Block this tool until policy is changed".to_string()),
            badge: Some("Persistent".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no reject cancel never always 5".to_string()),
        },
    ];

    let default_selection = InlineListSelection::ToolApproval(true);
    if hitl_notification_bell
        && let Err(err) = send_global_notification(NotificationEvent::HumanInTheLoop {
            prompt: "Tool approval required".to_string(),
            context: format!("Tool: {}", tool_name),
        })
        .await
    {
        tracing::debug!(error = %err, "Failed to emit HITL notification");
    }
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
                    InlineListSelection::ToolApprovalDenyOnce => {
                        return Ok(HitlDecision::DeniedOnce);
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
            InlineEvent::WizardModalSubmit(_)
            | InlineEvent::WizardModalStepComplete { .. }
            | InlineEvent::WizardModalBack { .. }
            | InlineEvent::WizardModalCancel => {
                ctrl_c_state.disarm_exit();
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
            InlineEvent::ForceCancelPtySession => {
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
                continue;
            }
            InlineEvent::ScrollLineUp
            | InlineEvent::ScrollLineDown
            | InlineEvent::ScrollPageUp
            | InlineEvent::ScrollPageDown
            | InlineEvent::FileSelected(_)
            | InlineEvent::BackgroundOperation
            | InlineEvent::LaunchEditor
            | InlineEvent::ToggleMode
            | InlineEvent::TeamPrev
            | InlineEvent::TeamNext
            | InlineEvent::PlanConfirmation(_)
            | InlineEvent::DiffPreviewApply
            | InlineEvent::DiffPreviewReject
            | InlineEvent::DiffPreviewTrustChanged { .. }
            | InlineEvent::EditQueue
            | InlineEvent::HistoryPrevious
            | InlineEvent::HistoryNext => {
                ctrl_c_state.disarm_exit();
            }
        }
    }
}
