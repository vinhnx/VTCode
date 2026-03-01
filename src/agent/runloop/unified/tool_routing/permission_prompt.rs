use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use tokio::task;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::sandboxing::SandboxPermissions as CoreSandboxPermissions;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::InlineEvent;
use vtcode_tui::InlineHandle;

use crate::agent::runloop::tool_output::format_unified_diff_lines;
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState};
use crate::agent::runloop::unified::ui_interaction::PlaceholderGuard;

use super::HitlDecision;

fn shell_run_args<'a>(tool_name: &str, tool_args: Option<&'a Value>) -> Option<&'a Value> {
    let args = tool_args?;
    match tool_name {
        "run_pty_cmd" | "shell" => Some(args),
        "unified_exec" | "exec_pty_cmd" | "exec" => {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .or_else(|| args.get("command").map(|_| "run"));
            (action == Some("run")).then_some(args)
        }
        _ => None,
    }
}

fn extract_shell_command_text_from_run_args(args: &Value) -> Option<String> {
    let command_value = args.get("command").or_else(|| args.get("raw_command"))?;

    if let Some(arr) = command_value.as_array() {
        let parts = arr
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            None
        } else {
            Some(shell_words::join(parts))
        }
    } else {
        command_value.as_str().map(|value| value.to_owned())
    }
}

fn parse_shell_sandbox_permissions(args: &Value) -> CoreSandboxPermissions {
    args.get("sandbox_permissions")
        .cloned()
        .map(serde_json::from_value::<CoreSandboxPermissions>)
        .transpose()
        .ok()
        .flatten()
        .unwrap_or(CoreSandboxPermissions::UseDefault)
}

pub(super) fn extract_shell_command_text(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    extract_shell_command_text_from_run_args(args)
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

pub(super) fn shell_permission_cache_suffix(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command = extract_shell_command_text_from_run_args(args)?;
    let sandbox_permissions = parse_shell_sandbox_permissions(args);
    let additional_permissions = args.get("additional_permissions");

    if sandbox_permissions == CoreSandboxPermissions::UseDefault && additional_permissions.is_none()
    {
        return Some(command);
    }

    let sandbox_permissions = serde_json::to_string(&sandbox_permissions)
        .unwrap_or_else(|_| "\"use_default\"".to_string());
    let additional_permissions = additional_permissions
        .map(|value| {
            serde_json::to_string(value).unwrap_or_else(|_| "\"<invalid_additional>\"".to_string())
        })
        .unwrap_or_else(|| "null".to_string());

    Some(format!(
        "{command}|sandbox_permissions={sandbox_permissions}|additional_permissions={additional_permissions}"
    ))
}

pub(super) fn shell_requests_elevated_sandbox_permissions(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> bool {
    let args = shell_run_args(tool_name, tool_args);
    args.map(parse_shell_sandbox_permissions)
    .map(|permissions| permissions.requires_approval())
    .unwrap_or(false)
}

fn tool_args_diff_preview(tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
    let args = tool_args?.as_object()?;
    let (before, after) = match tool_name {
        "edit_file" => {
            let old_str = args
                .get("old_str")
                .or_else(|| args.get("old_string"))
                .and_then(Value::as_str)?;
            let new_str = args
                .get("new_str")
                .or_else(|| args.get("new_string"))
                .and_then(Value::as_str)?;
            (Some(old_str), new_str)
        }
        "write_file" | "create_file" => {
            let content = args.get("content").and_then(Value::as_str)?;
            (None, content)
        }
        "unified_file" => {
            let action = args
                .get("action")
                .and_then(Value::as_str)
                .or_else(|| {
                    if args.get("old_str").is_some() || args.get("old_string").is_some() {
                        Some("edit")
                    } else if args.get("content").is_some() {
                        Some("write")
                    } else {
                        None
                    }
                })
                .unwrap_or("read");

            match action {
                "edit" => {
                    let old_str = args
                        .get("old_str")
                        .or_else(|| args.get("old_string"))
                        .and_then(Value::as_str)?;
                    let new_str = args
                        .get("new_str")
                        .or_else(|| args.get("new_string"))
                        .and_then(Value::as_str)?;
                    (Some(old_str), new_str)
                }
                "write" | "create" => {
                    let content = args.get("content").and_then(Value::as_str)?;
                    (None, content)
                }
                _ => return None,
            }
        }
        _ => return None,
    };
    let path = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("target_path"))
        .and_then(Value::as_str)
        .unwrap_or("(unknown file)");

    let diff_preview = vtcode_core::tools::file_ops::build_diff_preview(path, before, after);
    let skipped = diff_preview
        .get("skipped")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if skipped {
        let reason = diff_preview
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("preview unavailable");
        return Some(vec![format!("diff: {}", reason)]);
    }

    let content = diff_preview
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or("");
    if content.is_empty() {
        return Some(vec!["(no changes)".to_string()]);
    }

    let lines = format_unified_diff_lines(content);
    if lines.len() <= 80 {
        return Some(lines);
    }

    let mut preview = Vec::with_capacity(61);
    preview.extend(lines.iter().take(40).cloned());
    preview.push(format!("… +{} lines", lines.len().saturating_sub(60)));
    preview.extend(lines.iter().skip(lines.len().saturating_sub(20)).cloned());
    Some(preview)
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
    use vtcode_tui::{InlineListItem, InlineListSelection};

    let mut description_lines = vec![
        format!("Tool: {}", tool_name),
        format!("Action: {}", display_name),
    ];

    if let Some(args) = tool_args
        && let Some(obj) = args.as_object()
    {
        if let Some(diff_lines) = tool_args_diff_preview(tool_name, tool_args) {
            description_lines.push(String::new());
            description_lines.extend(diff_lines);
        } else {
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
            | InlineEvent::ListModalSelectionChanged(_)
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

#[cfg(test)]
mod tests {
    use super::{
        extract_shell_command_text, shell_permission_cache_suffix,
        shell_requests_elevated_sandbox_permissions,
    };
    use serde_json::json;

    #[test]
    fn unified_exec_extracts_command_when_action_is_run() {
        let args = json!({
            "action": "run",
            "command": "cargo check"
        });
        let command = extract_shell_command_text("unified_exec", Some(&args));
        assert_eq!(command.as_deref(), Some("cargo check"));
    }

    #[test]
    fn unified_exec_ignores_non_run_actions() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123"
        });
        let command = extract_shell_command_text("unified_exec", Some(&args));
        assert_eq!(command, None);
    }

    #[test]
    fn elevated_sandbox_permissions_require_prompt_for_unified_exec_run() {
        let args = json!({
            "action": "run",
            "command": "echo hi",
            "sandbox_permissions": "with_additional_permissions"
        });
        assert!(shell_requests_elevated_sandbox_permissions(
            "unified_exec",
            Some(&args)
        ));
    }

    #[test]
    fn elevated_sandbox_permissions_ignored_for_unified_exec_poll() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123",
            "sandbox_permissions": "with_additional_permissions"
        });
        assert!(!shell_requests_elevated_sandbox_permissions(
            "unified_exec",
            Some(&args)
        ));
    }

    #[test]
    fn require_escalated_permissions_require_prompt_for_unified_exec_run() {
        let args = json!({
            "action": "run",
            "command": "echo hi",
            "sandbox_permissions": "require_escalated"
        });
        assert!(shell_requests_elevated_sandbox_permissions(
            "unified_exec",
            Some(&args)
        ));
    }

    #[test]
    fn cache_suffix_includes_permissions_for_shell_run() {
        let plain = json!({
            "command": "echo hi"
        });
        let with_permissions = json!({
            "command": "echo hi",
            "sandbox_permissions": "with_additional_permissions",
            "additional_permissions": {
                "fs_write": ["/tmp/demo.txt"]
            }
        });

        let plain_key = shell_permission_cache_suffix("shell", Some(&plain));
        let permissioned_key = shell_permission_cache_suffix("shell", Some(&with_permissions));

        assert_ne!(plain_key, permissioned_key);
        assert!(
            permissioned_key
                .as_deref()
                .unwrap_or_default()
                .contains("with_additional_permissions")
        );
    }
}
