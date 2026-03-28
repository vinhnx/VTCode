use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::sandboxing::SandboxPermissions as CoreSandboxPermissions;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::app::{
    InlineHandle, ListOverlayRequest, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey,
    TransientRequest, TransientSubmission,
};

use crate::agent::runloop::tool_output::format_unified_diff_lines;
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderGuard;

use super::HitlDecision;
use super::shell_approval::PersistentApprovalTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolPermissionPromptKind {
    Standard,
    Mcp,
}

fn cancelled_prompt_decision(prompt_kind: ToolPermissionPromptKind) -> HitlDecision {
    if prompt_kind == ToolPermissionPromptKind::Mcp {
        HitlDecision::DeniedOnce
    } else {
        HitlDecision::Denied
    }
}

fn shell_run_args<'a>(tool_name: &str, tool_args: Option<&'a Value>) -> Option<&'a Value> {
    let args = tool_args?;
    vtcode_core::tools::tool_intent::is_command_run_tool_call(tool_name, args).then_some(args)
}

fn tool_permission_prompt_kind(tool_name: &str) -> ToolPermissionPromptKind {
    if vtcode_core::tools::mcp::is_legacy_mcp_tool_name(tool_name)
        || vtcode_core::tools::mcp::parse_canonical_mcp_tool_name(tool_name).is_some()
        || tool_name.starts_with(vtcode_core::tools::mcp::MCP_QUALIFIED_TOOL_PREFIX)
    {
        ToolPermissionPromptKind::Mcp
    } else {
        ToolPermissionPromptKind::Standard
    }
}

fn normalized_shell_command_value(args: &Value) -> Option<Value> {
    let normalized = vtcode_core::tools::command_args::normalize_shell_args(args)
        .ok()
        .unwrap_or_else(|| args.clone());
    vtcode_core::tools::command_args::normalized_command_value(&normalized)
        .ok()
        .flatten()
}

fn extract_shell_command_text_from_run_args(args: &Value) -> Option<String> {
    vtcode_core::tools::command_args::command_text(args)
        .ok()
        .flatten()
}

fn extract_shell_command_words_from_run_args(args: &Value) -> Option<Vec<String>> {
    vtcode_core::tools::command_args::command_words(args)
        .ok()
        .flatten()
}

fn render_shell_command_words(parts: &[String]) -> String {
    shell_words::join(parts.iter().map(|part| part.as_str()))
}

fn shell_command_contains_control_operators(command: &str) -> bool {
    command.contains("&&")
        || command.contains("||")
        || command.contains('|')
        || command.contains(';')
        || command.contains("$(")
        || command.contains('`')
        || command.contains("<<")
        || command.contains('\n')
}

fn shell_run_uses_nested_shell(command_words: &[String]) -> bool {
    let Some(program) = command_words.first() else {
        return false;
    };
    let basename = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    let args = &command_words[1..];

    match basename.as_str() {
        "sh" | "bash" | "zsh" | "fish" => args.iter().any(|arg| {
            matches!(
                arg.as_str(),
                "-c" | "-ic" | "-lc" | "--command" | "--login" | "--interactive"
            )
        }),
        "pwsh" | "powershell" | "powershell.exe" => args
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("-command") || arg.eq_ignore_ascii_case("-c")),
        "cmd" | "cmd.exe" => args
            .iter()
            .any(|arg| arg.eq_ignore_ascii_case("/c") || arg.eq_ignore_ascii_case("/k")),
        _ => false,
    }
}

fn shell_command_supports_persistent_approval(args: &Value, command_words: &[String]) -> bool {
    let Some(command_value) = normalized_shell_command_value(args) else {
        return false;
    };

    match command_value {
        Value::String(command) => {
            let trimmed = command.trim();
            !trimmed.is_empty()
                && !shell_command_contains_control_operators(trimmed)
                && !shell_run_uses_nested_shell(command_words)
        }
        Value::Array(_) => !shell_run_uses_nested_shell(command_words),
        _ => false,
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

fn shell_permission_scope_suffix(args: &Value) -> String {
    let sandbox_permissions = parse_shell_sandbox_permissions(args);
    let additional_permissions = args.get("additional_permissions");
    let sandbox_permissions = serde_json::to_string(&sandbox_permissions)
        .unwrap_or_else(|_| "\"use_default\"".to_string());
    let additional_permissions = additional_permissions
        .map(|value| {
            serde_json::to_string(value).unwrap_or_else(|_| "\"<invalid_additional>\"".to_string())
        })
        .unwrap_or_else(|| "null".to_string());

    format!(
        "sandbox_permissions={sandbox_permissions}|additional_permissions={additional_permissions}"
    )
}

fn extract_shell_approval_justification(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    args.get("justification")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn extract_shell_command_text(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    extract_shell_command_text_from_run_args(args)
}

pub(super) fn extract_shell_approval_command_words(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<Vec<String>> {
    let args = shell_run_args(tool_name, tool_args)?;
    extract_shell_command_words_from_run_args(args)
}

pub(super) fn extract_shell_approval_command_prefix_words(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<Vec<String>> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command_words = extract_shell_approval_command_words(tool_name, tool_args)?;
    shell_command_supports_persistent_approval(args, &command_words).then_some(command_words)
}

pub(super) fn extract_shell_permission_scope_signature(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    Some(shell_permission_scope_suffix(args))
}

pub(super) fn extract_shell_approval_scope_signature(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command_words = extract_shell_approval_command_words(tool_name, tool_args)?;
    shell_command_supports_persistent_approval(args, &command_words)
        .then(|| shell_permission_scope_suffix(args))
}

pub(super) fn extract_shell_persistent_approval_prefix_rule(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<Vec<String>> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command_words = extract_shell_command_words_from_run_args(args)?;
    if !shell_command_supports_persistent_approval(args, &command_words) {
        return None;
    }

    let prefix_rule = args
        .get("prefix_rule")
        .and_then(Value::as_array)?
        .iter()
        .map(|value| value.as_str().map(ToOwned::to_owned))
        .collect::<Option<Vec<_>>>()?;

    if prefix_rule.is_empty() || prefix_rule.len() > command_words.len() {
        return None;
    }

    prefix_rule
        .iter()
        .zip(command_words.iter())
        .all(|(prefix, command)| prefix == command)
        .then_some(prefix_rule)
}

pub(super) fn render_shell_persistent_approval_prefix_entry(
    tool_name: &str,
    tool_args: Option<&Value>,
    prefix_rule: &[String],
) -> Option<String> {
    let scope_signature = extract_shell_approval_scope_signature(tool_name, tool_args)?;
    Some(format!(
        "{}|{}",
        render_shell_command_words(prefix_rule),
        scope_signature
    ))
}

pub(super) fn render_shell_approval_command_words(parts: &[String]) -> String {
    render_shell_command_words(parts)
}

pub(super) fn shell_permission_cache_suffix(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command = extract_shell_command_text_from_run_args(args)?;
    let scope_suffix = shell_permission_scope_suffix(args);

    if parse_shell_sandbox_permissions(args) == CoreSandboxPermissions::UseDefault
        && args.get("additional_permissions").is_none()
    {
        return Some(command);
    }

    Some(format!("{command}|{scope_suffix}"))
}

#[cfg(test)]
pub(super) fn shell_allows_persistent_decisions(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> bool {
    extract_shell_command_text(tool_name, tool_args).is_none()
}

fn truncate_arg_preview(value: &str) -> String {
    const MAX_CHARS: usize = 60;
    const TRUNCATED_CHARS: usize = 57;
    if value.chars().nth(MAX_CHARS).is_some() {
        let mut truncated: String = value.chars().take(TRUNCATED_CHARS).collect();
        truncated.push_str("...");
        truncated
    } else {
        value.to_string()
    }
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

fn build_tool_permission_options(
    prompt_kind: ToolPermissionPromptKind,
    persistent_approval_target: Option<&PersistentApprovalTarget>,
) -> Vec<vtcode_tui::app::InlineListItem> {
    use vtcode_tui::app::{InlineListItem, InlineListSelection};

    let mut options = vec![
        InlineListItem {
            title: "Approve Once".to_string(),
            subtitle: Some("Allow this tool to execute this time only".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(true)),
            search_value: Some("approve yes allow once y 1".to_string()),
        },
        InlineListItem {
            title: if prompt_kind == ToolPermissionPromptKind::Mcp {
                "Approve this session".to_string()
            } else {
                "Allow for Session".to_string()
            },
            subtitle: if prompt_kind == ToolPermissionPromptKind::Mcp {
                Some("Run the tool and remember this choice for this session".to_string())
            } else {
                Some("Allow this tool for the current session".to_string())
            },
            badge: Some("Session".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalSession),
            search_value: Some("session temporary temp 2".to_string()),
        },
    ];

    if let Some(target) = persistent_approval_target {
        let subtitle = match target {
            PersistentApprovalTarget::ToolLevel => {
                "Remember approval for this tool in this workspace".to_string()
            }
            PersistentApprovalTarget::ExactInvocation { display_label } => {
                format!("Remember approval for {} in this workspace", display_label)
            }
            PersistentApprovalTarget::PrefixRule { display_label, .. } => {
                format!("Remember approval for {} in this workspace", display_label)
            }
        };
        options.push(InlineListItem {
            title: "Always approve and save to policy cache".to_string(),
            subtitle: Some(subtitle),
            badge: Some("Permanent".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalPermanent),
            search_value: Some("always permanent forever save 3".to_string()),
        });
    }

    options.push(InlineListItem {
        title: "".to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    });

    options.push(InlineListItem {
        title: if prompt_kind == ToolPermissionPromptKind::Mcp {
            "Cancel".to_string()
        } else {
            "Deny Once".to_string()
        },
        subtitle: if prompt_kind == ToolPermissionPromptKind::Mcp {
            Some("Cancel this tool call".to_string())
        } else {
            Some("Reject this tool for now (ask again next time)".to_string())
        },
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ToolApprovalDenyOnce),
        search_value: Some(if prompt_kind == ToolPermissionPromptKind::Mcp {
            "cancel stop reject decline 4".to_string()
        } else {
            "deny no reject once temporary 4".to_string()
        }),
    });

    if matches!(
        persistent_approval_target,
        Some(PersistentApprovalTarget::ToolLevel)
    ) && prompt_kind != ToolPermissionPromptKind::Mcp
    {
        options.push(InlineListItem {
            title: "Always Deny".to_string(),
            subtitle: Some("Block this tool until policy is changed".to_string()),
            badge: Some("Persistent".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no reject cancel never always 5".to_string()),
        });
    }

    options
}

pub(super) async fn prompt_tool_permission<S: UiSession + ?Sized>(
    display_name: &str,
    tool_name: &str,
    tool_args: Option<&Value>,
    approval_learning_key: &str,
    approval_learning_label: &str,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    approval_reason: Option<&str>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
    persistent_approval_target: Option<&PersistentApprovalTarget>,
    approval_recorder: Option<&vtcode_core::tools::ApprovalRecorder>,
    hitl_notification_bell: bool,
    source_thread_label: Option<&str>,
) -> Result<HitlDecision> {
    let prompt_kind = tool_permission_prompt_kind(tool_name);
    let mut description_lines = vec![
        format!("Tool: {}", tool_name),
        format!("Action: {}", display_name),
    ];

    if let Some(source_label) = source_thread_label {
        description_lines.push(format!("Source: {}", source_label));
    }

    if let Some(args) = tool_args
        && let Some(obj) = args.as_object()
    {
        if let Some(diff_lines) = tool_args_diff_preview(tool_name, tool_args) {
            description_lines.push(String::new());
            description_lines.extend(diff_lines);
        } else {
            for (key, value) in obj.iter().take(3) {
                if let Some(str_val) = value.as_str() {
                    description_lines.push(format!("  {}: {}", key, truncate_arg_preview(str_val)));
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

    if let Some(reason) = approval_reason {
        description_lines.push(String::new());
        description_lines.push(format!("Reason: {}", reason));
    }

    if let Some(shell_justification) = extract_shell_approval_justification(tool_name, tool_args) {
        description_lines.push(format!("Justification: {}", shell_justification));
    }

    if let Some(just) = justification {
        let just_lines = just.format_for_dialog();
        description_lines.extend(just_lines);
    }

    if let Some(recorder) = approval_recorder
        && let Some(suggestion) = recorder
            .get_auto_approval_suggestion(approval_learning_key, approval_learning_label)
            .await
    {
        description_lines.push(String::new());
        description_lines.push(format!("Suggestion: {}", suggestion));
    }

    description_lines.push(String::new());
    description_lines.push("Choose how to handle this tool execution:".to_string());
    let mut navigation_hint = if prompt_kind == ToolPermissionPromptKind::Mcp {
        "Use ↑↓ or Tab to navigate • Enter to select • Esc to cancel".to_string()
    } else {
        "Use ↑↓ or Tab to navigate • Enter to select • Esc to deny".to_string()
    };
    if source_thread_label.is_some() {
        navigation_hint.push_str(" • o inspect source thread");
    }
    description_lines.push(navigation_hint);

    let options = build_tool_permission_options(prompt_kind, persistent_approval_target);
    let hotkeys = source_thread_label
        .map(|_| {
            vec![TransientHotkey {
                key: TransientHotkeyKey::Char('o'),
                action: TransientHotkeyAction::OpenSourceThread,
            }]
        })
        .unwrap_or_default();

    use vtcode_tui::app::InlineListSelection;
    let default_selection = InlineListSelection::ToolApproval(true);
    if hitl_notification_bell
        && let Err(err) = send_global_notification(NotificationEvent::PermissionPrompt {
            title: "VT Code approval required".to_string(),
            message: format!("Review the permission prompt for tool `{tool_name}`."),
        })
        .await
    {
        tracing::debug!(error = %err, "Failed to emit HITL notification");
    }
    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    let outcome = show_overlay_and_wait(
        handle,
        session,
        TransientRequest::List(ListOverlayRequest {
            title: "Tool Permission Required".to_string(),
            lines: description_lines,
            footer_hint: None,
            items: options,
            selected: Some(default_selection),
            search: None,
            hotkeys,
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            TransientSubmission::Hotkey(TransientHotkeyAction::OpenSourceThread) => {
                handle.set_input("/agent".to_string());
                let _ = renderer.line(
                    vtcode_core::utils::ansi::MessageStyle::Info,
                    "Switched focus to thread selector command. Run /agent to inspect the source thread, then retry the tool action.",
                );
                Some(HitlDecision::DeniedOnce)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApproval(true)) => {
                Some(HitlDecision::Approved)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApprovalSession) => {
                Some(HitlDecision::ApprovedSession)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApprovalPermanent) => {
                Some(HitlDecision::ApprovedPermanent)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApprovalDenyOnce) => {
                Some(HitlDecision::DeniedOnce)
            }
            TransientSubmission::Selection(InlineListSelection::ToolApproval(false)) => {
                Some(HitlDecision::Denied)
            }
            TransientSubmission::Selection(_) => Some(HitlDecision::Denied),
            _ => None,
        },
    )
    .await?;

    match outcome {
        OverlayWaitOutcome::Submitted(decision) => Ok(decision),
        OverlayWaitOutcome::Cancelled => Ok(cancelled_prompt_decision(prompt_kind)),
        OverlayWaitOutcome::Interrupted => Ok(HitlDecision::Interrupt),
        OverlayWaitOutcome::Exit => Ok(HitlDecision::Exit),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ToolPermissionPromptKind, build_tool_permission_options, cancelled_prompt_decision,
        extract_shell_approval_command_prefix_words, extract_shell_approval_justification,
        extract_shell_approval_scope_signature, extract_shell_command_text,
        extract_shell_persistent_approval_prefix_rule,
        render_shell_persistent_approval_prefix_entry, shell_allows_persistent_decisions,
        shell_permission_cache_suffix, tool_permission_prompt_kind, truncate_arg_preview,
    };
    use crate::agent::runloop::unified::tool_routing::shell_approval::PersistentApprovalTarget;
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
    fn unified_exec_extracts_cmd_alias_when_action_is_inferred() {
        let args = json!({
            "cmd": "cargo check"
        });
        let command = extract_shell_command_text("unified_exec", Some(&args));
        assert_eq!(command.as_deref(), Some("cargo check"));
    }

    #[test]
    fn unified_exec_extracts_indexed_command_parts_when_action_is_inferred() {
        let args = json!({
            "command.0": "cargo",
            "command.1": "check"
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
    fn arg_preview_truncates_unicode_safely() {
        let value = "an’t ".repeat(20);
        let truncated = truncate_arg_preview(&value);
        assert!(truncated.ends_with("..."));
        assert!(truncated.chars().count() <= 60);
    }

    #[test]
    fn shell_runs_disable_persistent_decisions() {
        let args = json!({
            "action": "run",
            "command": "echo hi",
            "sandbox_permissions": "with_additional_permissions"
        });
        assert!(!shell_allows_persistent_decisions(
            "unified_exec",
            Some(&args)
        ));
    }

    #[test]
    fn non_shell_unified_exec_actions_keep_persistent_decisions() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123",
            "sandbox_permissions": "with_additional_permissions"
        });
        assert!(shell_allows_persistent_decisions(
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

    #[test]
    fn shell_approval_justification_is_extracted_for_run_actions() {
        let args = json!({
            "action": "run",
            "command": "cargo build",
            "justification": "Do you want to build the project outside the sandbox?"
        });

        let justification = extract_shell_approval_justification("unified_exec", Some(&args));
        assert_eq!(
            justification.as_deref(),
            Some("Do you want to build the project outside the sandbox?")
        );
    }

    #[test]
    fn shell_approval_justification_ignores_non_run_actions() {
        let args = json!({
            "action": "poll",
            "session_id": "run-123",
            "justification": "ignored"
        });

        let justification = extract_shell_approval_justification("unified_exec", Some(&args));
        assert_eq!(justification, None);
    }

    #[test]
    fn shell_persistent_prefix_rule_requires_matching_command_prefix() {
        let args = json!({
            "action": "run",
            "command": "cargo test -p vtcode",
            "prefix_rule": ["cargo", "build"]
        });

        let prefix_rule =
            extract_shell_persistent_approval_prefix_rule("unified_exec", Some(&args));
        assert_eq!(prefix_rule, None);
    }

    #[test]
    fn shell_persistent_prefix_rule_rejects_compound_commands() {
        let args = json!({
            "action": "run",
            "command": "cargo test && cargo fmt",
            "prefix_rule": ["cargo", "test"]
        });

        let prefix_rule =
            extract_shell_persistent_approval_prefix_rule("unified_exec", Some(&args));
        assert_eq!(prefix_rule, None);
    }

    #[test]
    fn shell_approval_prefix_text_normalizes_simple_commands() {
        let args = json!({
            "action": "run",
            "command": ["cargo", "test", "-p", "vtcode"]
        });

        let command = extract_shell_approval_command_prefix_words("unified_exec", Some(&args));
        assert_eq!(
            command,
            Some(vec![
                "cargo".to_string(),
                "test".to_string(),
                "-p".to_string(),
                "vtcode".to_string()
            ])
        );
    }

    #[test]
    fn shell_approval_scope_signature_tracks_requested_permissions() {
        let args = json!({
            "action": "run",
            "command": "cargo test",
            "sandbox_permissions": "require_escalated"
        });

        let scope = extract_shell_approval_scope_signature("unified_exec", Some(&args));
        assert_eq!(
            scope.as_deref(),
            Some("sandbox_permissions=\"require_escalated\"|additional_permissions=null")
        );
    }

    #[test]
    fn shell_persistent_approval_entry_includes_scope() {
        let args = json!({
            "action": "run",
            "command": "cargo test -p vtcode",
            "prefix_rule": ["cargo", "test"],
            "sandbox_permissions": "require_escalated"
        });

        let entry = render_shell_persistent_approval_prefix_entry(
            "unified_exec",
            Some(&args),
            &["cargo".to_string(), "test".to_string()],
        );
        assert_eq!(
            entry.as_deref(),
            Some(
                "cargo test|sandbox_permissions=\"require_escalated\"|additional_permissions=null"
            )
        );
    }

    #[test]
    fn canonical_mcp_tools_use_mcp_prompt_kind() {
        assert_eq!(
            tool_permission_prompt_kind("mcp::calendar::list_events"),
            ToolPermissionPromptKind::Mcp
        );
    }

    #[test]
    fn model_visible_mcp_tools_use_mcp_prompt_kind() {
        assert_eq!(
            tool_permission_prompt_kind("mcp__calendar__list_events"),
            ToolPermissionPromptKind::Mcp
        );
    }

    #[test]
    fn non_mcp_tools_keep_standard_prompt_kind() {
        assert_eq!(
            tool_permission_prompt_kind("unified_exec"),
            ToolPermissionPromptKind::Standard
        );
    }

    #[test]
    fn mcp_prompt_uses_cancel_without_persistent_deny() {
        let titles = build_tool_permission_options(
            ToolPermissionPromptKind::Mcp,
            Some(&PersistentApprovalTarget::ToolLevel),
        )
        .into_iter()
        .map(|item| item.title)
        .collect::<Vec<_>>();
        assert!(titles.iter().any(|title| title == "Cancel"));
        assert!(!titles.iter().any(|title| title == "Always Deny"));
    }

    #[test]
    fn standard_prompt_keeps_persistent_deny() {
        let titles = build_tool_permission_options(
            ToolPermissionPromptKind::Standard,
            Some(&PersistentApprovalTarget::ToolLevel),
        )
        .into_iter()
        .map(|item| item.title)
        .collect::<Vec<_>>();
        assert!(titles.iter().any(|title| title == "Deny Once"));
        assert!(titles.iter().any(|title| title == "Always Deny"));
    }

    #[test]
    fn shell_prompt_offers_policy_cache_option_for_exact_invocation() {
        let titles = build_tool_permission_options(
            ToolPermissionPromptKind::Standard,
            Some(&PersistentApprovalTarget::ExactInvocation {
                display_label: "command `cargo clippy`".to_string(),
            }),
        )
        .into_iter()
        .map(|item| item.title)
        .collect::<Vec<_>>();
        assert!(
            titles
                .iter()
                .any(|title| title == "Always approve and save to policy cache")
        );
        assert!(!titles.iter().any(|title| title == "Always Deny"));
    }

    #[test]
    fn mcp_prompt_cancellation_is_non_persistent() {
        assert_eq!(
            cancelled_prompt_decision(ToolPermissionPromptKind::Mcp),
            super::HitlDecision::DeniedOnce
        );
    }
}
