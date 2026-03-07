use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::sandboxing::SandboxPermissions as CoreSandboxPermissions;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_tui::{InlineHandle, ListOverlayRequest, OverlayRequest, OverlaySubmission};

use crate::agent::runloop::tool_output::format_unified_diff_lines;
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_overlay_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderGuard;

use super::HitlDecision;

fn shell_run_args<'a>(tool_name: &str, tool_args: Option<&'a Value>) -> Option<&'a Value> {
    let args = tool_args?;
    match tool_name {
        "run_pty_cmd" | "shell" => Some(args),
        "unified_exec" | "exec_pty_cmd" | "exec" => {
            vtcode_core::tools::tool_intent::unified_exec_action(args)
                .is_some_and(|action| action.eq_ignore_ascii_case("run"))
                .then_some(args)
        }
        _ => None,
    }
}

fn normalized_shell_command_value(args: &Value) -> Option<Value> {
    vtcode_core::tools::command_args::normalized_command_value(args)
        .ok()
        .flatten()
}

fn extract_shell_command_text_from_run_args(args: &Value) -> Option<String> {
    let command_value = normalized_shell_command_value(args)?;

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

fn extract_shell_command_words_from_run_args(args: &Value) -> Option<Vec<String>> {
    let command_value = normalized_shell_command_value(args)?;
    let mut parts = match command_value {
        Value::String(command) => shell_words::split(&command).ok()?,
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().map(ToOwned::to_owned))
            .collect::<Option<Vec<_>>>()?,
        _ => return None,
    };

    if let Some(extra_args) = args.get("args").and_then(Value::as_array) {
        for value in extra_args {
            parts.push(value.as_str()?.to_string());
        }
    }

    (!parts.is_empty()).then_some(parts)
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

pub(super) fn shell_allows_persistent_decisions(
    tool_name: &str,
    tool_args: Option<&Value>,
) -> bool {
    extract_shell_command_text(tool_name, tool_args).is_none()
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
    approval_learning_key: &str,
    approval_learning_label: &str,
    _renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    session: &mut S,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    approval_reason: Option<&str>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
    persistent_shell_allow_prefix_rule: Option<&[String]>,
    allow_tool_level_persistent_decisions: bool,
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
    description_lines.push("Use ↑↓ or Tab to navigate • Enter to select • Esc to deny".to_string());

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
            title: "Allow for Session".to_string(),
            subtitle: Some("Allow this tool for the current session".to_string()),
            badge: Some("Session".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApprovalSession),
            search_value: Some("session temporary temp 2".to_string()),
        },
    ];

    if allow_tool_level_persistent_decisions || persistent_shell_allow_prefix_rule.is_some() {
        let subtitle = persistent_shell_allow_prefix_rule
            .map(|prefix_rule| {
                let rendered = shell_words::join(prefix_rule.iter().map(|part| part.as_str()));
                format!("Permanently allow commands that start with `{}`", rendered)
            })
            .unwrap_or_else(|| "Permanently allow this tool (saved to policy)".to_string());
        options.push(InlineListItem {
            title: "Always Allow".to_string(),
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
        title: "Deny Once".to_string(),
        subtitle: Some("Reject this tool for now (ask again next time)".to_string()),
        badge: None,
        indent: 0,
        selection: Some(InlineListSelection::ToolApprovalDenyOnce),
        search_value: Some("deny no reject once temporary 4".to_string()),
    });

    if allow_tool_level_persistent_decisions {
        options.push(InlineListItem {
            title: "Always Deny".to_string(),
            subtitle: Some("Block this tool until policy is changed".to_string()),
            badge: Some("Persistent".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ToolApproval(false)),
            search_value: Some("deny no reject cancel never always 5".to_string()),
        });
    }

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
    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    let outcome = show_overlay_and_wait(
        handle,
        session,
        OverlayRequest::List(ListOverlayRequest {
            title: "Tool Permission Required".to_string(),
            lines: description_lines,
            footer_hint: None,
            items: options,
            selected: Some(default_selection),
            search: None,
            hotkeys: Vec::new(),
        }),
        ctrl_c_state,
        ctrl_c_notify,
        |submission| match submission {
            OverlaySubmission::Selection(InlineListSelection::ToolApproval(true)) => {
                Some(HitlDecision::Approved)
            }
            OverlaySubmission::Selection(InlineListSelection::ToolApprovalSession) => {
                Some(HitlDecision::ApprovedSession)
            }
            OverlaySubmission::Selection(InlineListSelection::ToolApprovalPermanent) => {
                Some(HitlDecision::ApprovedPermanent)
            }
            OverlaySubmission::Selection(InlineListSelection::ToolApprovalDenyOnce) => {
                Some(HitlDecision::DeniedOnce)
            }
            OverlaySubmission::Selection(InlineListSelection::ToolApproval(false)) => {
                Some(HitlDecision::Denied)
            }
            OverlaySubmission::Selection(_) => Some(HitlDecision::Denied),
            _ => None,
        },
    )
    .await?;

    match outcome {
        OverlayWaitOutcome::Submitted(decision) => Ok(decision),
        OverlayWaitOutcome::Cancelled => Ok(HitlDecision::Denied),
        OverlayWaitOutcome::Interrupted => Ok(HitlDecision::Interrupt),
        OverlayWaitOutcome::Exit => Ok(HitlDecision::Exit),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extract_shell_approval_command_prefix_words, extract_shell_approval_justification,
        extract_shell_approval_scope_signature, extract_shell_command_text,
        extract_shell_persistent_approval_prefix_rule,
        render_shell_persistent_approval_prefix_entry, shell_allows_persistent_decisions,
        shell_permission_cache_suffix,
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
}
