use crate::agent::runloop::ui_list;
use crate::agent::runloop::ui_list::Tone;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tokio::sync::Notify;
use vtcode_commons::modal_hints::{APPROVAL_NAVIGATE_CANCEL, APPROVAL_NAVIGATE_DENY, choose_handling_line};
use vtcode_core::core::interfaces::ui::UiSession;
use vtcode_core::notifications::{NotificationEvent, send_global_notification};
use vtcode_core::sandboxing::{AdditionalPermissions, SandboxPermissions as CoreSandboxPermissions};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::{
    InlineHandle, ListOverlayRequest, TransientHotkey, TransientHotkeyAction, TransientHotkeyKey, TransientRequest,
    TransientSubmission,
};

use super::HitlDecision;
use super::shell_approval::{ApprovalLearningTarget, PersistentApprovalTarget};
use crate::agent::runloop::tool_output::format_unified_diff_lines;
use crate::agent::runloop::unified::overlay_prompt::{OverlayWaitOutcome, show_permission_and_wait};
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::ui_interaction::PlaceholderGuard;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ToolPermissionPromptKind {
    Standard,
    Mcp,
}

#[cold]
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

pub(super) fn tool_permission_prompt_kind(tool_name: &str) -> ToolPermissionPromptKind {
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
    match vtcode_core::tools::command_args::normalize_shell_args(args) {
        Ok(normalized) => vtcode_core::tools::command_args::normalized_command_value(&normalized)
            .ok()
            .flatten(),
        Err(_) => vtcode_core::tools::command_args::normalized_command_value(args).ok().flatten(),
    }
}

fn extract_shell_command_text_from_run_args(args: &Value) -> Option<String> {
    vtcode_core::tools::command_args::command_text(args).ok().flatten()
}

fn extract_shell_command_words_from_run_args(args: &Value) -> Option<Vec<String>> {
    vtcode_core::tools::command_args::command_words(args).ok().flatten()
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
        "sh" | "bash" | "zsh" | "fish" => args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-c" | "-ic" | "-lc" | "--command" | "--login" | "--interactive")),
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
    let requested_permissions = args
        .get("sandbox_permissions")
        .cloned()
        .map(serde_json::from_value::<CoreSandboxPermissions>)
        .transpose()
        .ok()
        .flatten()
        .unwrap_or(CoreSandboxPermissions::UseDefault);

    requested_permissions.normalized_for(parse_shell_additional_permissions(args).as_ref())
}

fn parse_shell_additional_permissions(args: &Value) -> Option<AdditionalPermissions> {
    args.get("additional_permissions")
        .cloned()
        .and_then(|value| serde_json::from_value::<AdditionalPermissions>(value).ok())
        .filter(|permissions| !permissions.is_empty())
}

fn shell_additional_permissions_present(args: &Value) -> bool {
    parse_shell_additional_permissions(args).is_some()
}

fn shell_permission_scope_suffix(args: &Value) -> String {
    let sandbox_permissions = parse_shell_sandbox_permissions(args);
    let sandbox_permissions =
        serde_json::to_string(&sandbox_permissions).unwrap_or_else(|_| "\"use_default\"".to_string());
    let additional_permissions = args
        .get("additional_permissions")
        .filter(|_| shell_additional_permissions_present(args))
        .map(|value| serde_json::to_string(value).unwrap_or_else(|_| "\"<invalid_additional>\"".to_string()))
        .unwrap_or_else(|| "null".to_string());

    format!("sandbox_permissions={sandbox_permissions}|additional_permissions={additional_permissions}")
}

fn extract_shell_approval_justification(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    args.get("justification")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn extract_shell_command_text(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    extract_shell_command_text_from_run_args(args)
}

pub(super) fn extract_shell_raw_command_text(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    vtcode_core::tools::command_args::raw_command_text(args)
}

pub(super) fn split_command_words_on_operators(parts: &[String]) -> Option<Vec<Vec<String>>> {
    if parts.is_empty() {
        return None;
    }

    let mut segments: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();

    for word in parts {
        match word.as_str() {
            "|" | "||" | "&&" | ";" => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(word.clone()),
        }
    }

    if !current.is_empty() {
        segments.push(current);
    }

    if segments.is_empty() || segments.iter().all(|s| s.is_empty()) {
        return None;
    }

    Some(segments)
}

pub(super) fn extract_shell_approval_command_words(tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
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

pub(super) fn extract_shell_permission_scope_signature(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    Some(shell_permission_scope_suffix(args))
}

pub(super) fn extract_shell_approval_scope_signature(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command_words = extract_shell_approval_command_words(tool_name, tool_args)?;
    shell_command_supports_persistent_approval(args, &command_words).then(|| shell_permission_scope_suffix(args))
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
    Some(format!("{}|{}", render_shell_command_words(prefix_rule), scope_signature))
}

pub(super) fn render_shell_approval_command_words(parts: &[String]) -> String {
    render_shell_command_words(parts)
}

pub(super) fn shell_permission_cache_suffix(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let command = extract_shell_command_text_from_run_args(args)?;
    let scope_suffix = shell_permission_scope_suffix(args);

    if parse_shell_sandbox_permissions(args) == CoreSandboxPermissions::UseDefault
        && !shell_additional_permissions_present(args)
    {
        return Some(command);
    }

    Some(format!("{command}|{scope_suffix}"))
}

#[cfg(test)]
pub(super) fn shell_allows_persistent_decisions(tool_name: &str, tool_args: Option<&Value>) -> bool {
    extract_shell_command_text(tool_name, tool_args).is_none()
}

#[cfg(test)]
fn truncate_arg_preview(value: &str) -> String {
    // Canonical helper: hard 60-char budget with "..." reserving its own width.
    vtcode_commons::formatting::truncate_within(value, 60, "...")
}

fn shell_command_preview_lines(tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
    let command = extract_shell_command_text(tool_name, tool_args)?;
    let command = command.trim();
    (!command.is_empty()).then(|| command.lines().map(str::to_string).collect())
}

/// Sandbox posture row for shell approvals (`Environment: ...`), mirroring
/// the reference approval UX. Labels describe the *requested* posture only —
/// the execution boundary enforces the policy — so `UseDefault` never
/// claims a sandbox this layer cannot verify. `None` for non-shell tools,
/// which have no shell execution environment to review.
fn shell_environment_row(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let args = shell_run_args(tool_name, tool_args)?;
    let posture = match parse_shell_sandbox_permissions(args) {
        CoreSandboxPermissions::UseDefault => "default policy",
        CoreSandboxPermissions::WithAdditionalPermissions => "default policy + extra grants",
        CoreSandboxPermissions::RequireEscalated => "escalated privileges",
        CoreSandboxPermissions::BypassSandbox => "no sandbox",
    };
    Some(format!("Environment: {posture}"))
}

/// Full, untruncated URL for web-fetch approval modals. The modal description
/// must show the exact fetch target (not just the domain) so approval is
/// informed. No truncation is applied here; modal wrapping handles width.
fn web_fetch_approval_url(tool_name: &str, tool_args: Option<&Value>) -> Option<String> {
    let canonical = vtcode_core::tools::names::canonical_tool_name(tool_name);
    let is_web_fetch = canonical == vtcode_core::config::constants::tools::WEB_FETCH
        || canonical == vtcode_core::config::constants::tools::FETCH_URL
        || canonical == vtcode_core::config::constants::tools::FETCH
        || canonical == vtcode_core::config::constants::tools::DEFUDDLE_FETCH
        || tool_name == vtcode_core::config::constants::tools::FETCH;
    if !is_web_fetch {
        return None;
    }
    tool_args?
        .as_object()?
        .get("url")?
        .as_str()
        .map(str::trim)
        .filter(|url| !url.is_empty())
        .map(ToOwned::to_owned)
}

/// Max logical command lines shown in the approval modal; longer scripts
/// collapse their middle lines behind an explicit omission count so the
/// popup fits its 8-row viewport while head and tail stay reviewable.
const MAX_COMMAND_PREVIEW_LINES: usize = 8;
/// Max chars for persistent-approval display labels embedded in option
/// subtitles; the dedicated command block carries the reviewable invocation.
const MAX_APPROVAL_LABEL_CHARS: usize = 60;
/// Max chars per context row (modal wrapping handles visual width).
const MAX_CONTEXT_LINE_CHARS: usize = 160;
const MAX_RISK_LINE_CHARS: usize = 32;

fn format_command_preview_line(line: &str) -> String {
    // Same control-char mapping as `truncate_middle`: a lone `\r` survives
    // `str::lines` and could otherwise hide a suffix at render time.
    // No length cap: per-line truncation splits shell tokens and quoted
    // script bodies (screenshot 2026-09-27 `awk 'NR==FNR ...'`), defeating
    // review; modal wrapping owns viewport width.
    let sanitized: String = line.chars().map(|c| if matches!(c, '\r' | '\t') { ' ' } else { c }).collect();
    format!("`{sanitized}`")
}

/// Full command lines for the approval modal. Shown lines are never
/// truncated, so the quote-aware `shell_syntax_segments` highlighter (fed
/// by the tree-sitter-bash safety pipeline) sees intact tokens. The first
/// row carries a `$ ` shell marker (rendered dimmed by the modal) so the
/// block reads as a command to execute. Only the row count is bounded:
/// beyond `MAX_COMMAND_PREVIEW_LINES` the middle collapses behind an
/// explicit omission count, keeping head and tail reviewable inside the
/// modal viewport. Mirrors `preview_full_command` for transcript `• Ran`
/// headers (821a9c35a) and the untruncated web-fetch URL (99cc2fdfe).
fn format_command_preview_lines(command_lines: Vec<String>) -> Vec<String> {
    let visible: Vec<String> = command_lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| format_command_preview_line(line))
        .collect();
    if visible.len() <= MAX_COMMAND_PREVIEW_LINES {
        let mut preview = visible;
        prefix_shell_marker(preview.first_mut());
        return preview;
    }
    const HEAD_LINES: usize = 5;
    const TAIL_LINES: usize = 2;
    let hidden = visible.len().saturating_sub(HEAD_LINES + TAIL_LINES);
    let mut preview = Vec::with_capacity(MAX_COMMAND_PREVIEW_LINES);
    preview.extend(visible.iter().take(HEAD_LINES).cloned());
    preview.push(format!("… +{hidden} more lines (full command runs on approval)"));
    preview.extend(visible.iter().skip(visible.len().saturating_sub(TAIL_LINES)).cloned());
    prefix_shell_marker(preview.first_mut());
    preview
}

/// Prefix the first preview row with the `$ ` shell marker, inside the
/// backtick code fence so the modal renderer keeps highlighting the body.
/// Index-free: rows that do not open with a backtick are left untouched.
fn prefix_shell_marker(first_row: Option<&mut String>) {
    if let Some(row) = first_row
        && let Some(body) = row.strip_prefix('`')
    {
        *row = format!("`$ {body}");
    }
}

/// Friendly label for the agent's goal in the permission popup. The modal
/// renderer styles this as a subordinate context row (no bullet), matching
/// the previous `Reason:` treatment without the technical `WHY` header.
const AGENT_GOAL_LABEL: &str = "What the agent is trying to do";

fn push_context_line(description_lines: &mut Vec<String>, label: &str, value: &str) -> bool {
    push_capped_context_line(description_lines, label, value, MAX_CONTEXT_LINE_CHARS)
}

fn push_capped_context_line(description_lines: &mut Vec<String>, label: &str, value: &str, max_chars: usize) -> bool {
    let first = value.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    if !first.is_empty() {
        description_lines.push(format!("{label}: {}", vtcode_commons::formatting::truncate_middle(first, max_chars)));
        true
    } else {
        false
    }
}

/// Compact agent-justification lines for the approval dialog: agent goal +
/// `Risk` only so the popup stays minimal and scannable. Full reasoning
/// (including expected outcome) remains in logs.
fn compact_justification_lines(just: &vtcode_core::tools::ToolJustification) -> Vec<String> {
    let mut lines = Vec::new();
    let reason = just.reason.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    if !reason.is_empty() {
        lines.push(format!(
            "{AGENT_GOAL_LABEL}: {}",
            vtcode_commons::formatting::truncate_middle(reason, MAX_CONTEXT_LINE_CHARS)
        ));
    }
    let risk = just.risk_level.trim();
    if !risk.is_empty() {
        lines.push(format!("Risk: {}", vtcode_commons::formatting::truncate_middle(risk, MAX_RISK_LINE_CHARS)));
    }
    lines
}

fn tool_args_diff_preview(tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
    let args = tool_args?.as_object()?;
    let (before, after) = match tool_name {
        "edit_file" => {
            let old_str = args.get("old_str").or_else(|| args.get("old_string")).and_then(Value::as_str)?;
            let new_str = args.get("new_str").or_else(|| args.get("new_string")).and_then(Value::as_str)?;
            (Some(old_str), new_str)
        }
        "write_file" | "create_file" => {
            let content = args.get("content").and_then(Value::as_str)?;
            (None, content)
        }
        "file_operation" => {
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
                    let old_str = args.get("old_str").or_else(|| args.get("old_string")).and_then(Value::as_str)?;
                    let new_str = args.get("new_str").or_else(|| args.get("new_string")).and_then(Value::as_str)?;
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
    let skipped = diff_preview.get("skipped").and_then(Value::as_bool).unwrap_or(false);
    if skipped {
        let reason = diff_preview
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("preview unavailable");
        return Some(vec![format!("diff: {}", reason)]);
    }

    let content = diff_preview.get("content").and_then(Value::as_str).unwrap_or("");
    if content.is_empty() {
        return Some(vec!["(no changes)".to_string()]);
    }

    let plain = vtcode_commons::ansi::strip_ansi(content);
    let lines = format_unified_diff_lines(&plain);
    if lines.len() <= 80 {
        return Some(lines);
    }

    let mut preview = Vec::with_capacity(61);
    preview.extend(lines.iter().take(40).cloned());
    preview.push(format!("… +{} lines", lines.len().saturating_sub(60)));
    preview.extend(lines.iter().skip(lines.len().saturating_sub(20)).cloned());
    Some(preview)
}

/// Overlay title for the approval popup. Shell commands ask the plain
/// question directly; every other tool keeps the generic title.
fn tool_permission_title(has_command_preview: bool) -> &'static str {
    if has_command_preview {
        "Would you like to run the following command?"
    } else {
        "Tool Permission Required"
    }
}

/// Intro rows for the approval popup, written as plain permission language.
/// Shell commands need no intro sentence: the overlay title already asks
/// the question and the command block follows. Other tools reuse the
/// human-friendly action summary directly in the intro sentence.
fn tool_permission_header_lines(
    tool_name: &str,
    display_name: &str,
    has_command_preview: bool,
    source_thread_label: Option<&str>,
) -> Vec<String> {
    let mut lines = Vec::new();
    if has_command_preview {
        // Title carries the question; fall through to the source row only.
    } else if !display_name.trim().is_empty() {
        let trimmed = display_name.trim();
        // `describe_tool_action` prefixes MCP tools with "MCP " (e.g. "MCP Search
        // code for docs"); move it to a trailing "via MCP" so the sentence keeps
        // verb agreement and the acronym is never lowercased mid-word.
        if let Some(rest) = trimmed.strip_prefix("MCP ") {
            lines.push(format!("The agent wants to {} via MCP and needs your approval.", lowercase_first_action(rest)));
        } else {
            lines.push(format!("The agent wants to {} and needs your approval.", lowercase_first_action(trimmed)));
        }
    } else {
        lines.push(format!("The agent wants to use {tool_name} and needs your approval."));
    }
    if let Some(source_label) = source_thread_label {
        lines.push(format!("Requested from: {source_label}"));
    }
    lines
}

/// Lowercase the initial of a Titlecase action ("Edit file" → "edit file") so
/// it fits mid-sentence. Leading acronyms ("MCP", "URL") are left alone so
/// they are never mangled into "mCP"/"uRL".
fn lowercase_first_action(action: &str) -> String {
    let mut chars = action.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => {
            let rest = chars.as_str();
            let next_is_upper = rest.chars().next().is_some_and(|next| next.is_uppercase());
            if first.is_uppercase() && !next_is_upper {
                first.to_lowercase().collect::<String>() + rest
            } else {
                action.to_string()
            }
        }
    }
}

/// Context rows explaining what the agent is trying to do and the risk level.
/// Pure helper so the dedup/fallback rules stay unit-testable without a UI
/// session: at most one agent-goal row (an explicit reason wins over the
/// ledger justification), plus `Risk` when present, plus a fallback sentence
/// when the agent provided no details at all.
fn build_permission_context_lines(
    approval_reason: Option<&str>,
    shell_justification: Option<&str>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut goal_emitted = false;

    if let Some(reason) = approval_reason {
        goal_emitted = push_context_line(&mut lines, AGENT_GOAL_LABEL, reason);
    } else if let Some(shell_reason) = shell_justification {
        goal_emitted = push_context_line(&mut lines, AGENT_GOAL_LABEL, shell_reason);
    }

    if let Some(just) = justification {
        // `compact_justification_lines` emits the agent goal + Risk only; skip
        // a duplicate goal row when an explicit reason already covers it.
        for line in compact_justification_lines(just) {
            if line.starts_with(AGENT_GOAL_LABEL) && goal_emitted {
                continue;
            }
            lines.push(line);
        }
    }

    if lines.is_empty() {
        lines.push("No additional details were provided. Review the action above before allowing.".to_string());
    }

    lines
}

fn build_tool_permission_options(
    prompt_kind: ToolPermissionPromptKind,
    persistent_approval_target: Option<&PersistentApprovalTarget>,
) -> Vec<vtcode_ui::tui::app::InlineListItem> {
    use vtcode_ui::tui::app::InlineListSelection;

    let mut options = vec![
        ui_list::action(
            "Approve Once",
            "Allow this time only",
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApproval(true)),
        )
        .with_search_value("approve yes allow once y 1".to_string()),
        // Scope badge is load-bearing: "Always" rows persist, "Session" does not.
        ui_list::action(
            if prompt_kind == ToolPermissionPromptKind::Mcp {
                "Approve this session"
            } else {
                "Allow for Session"
            },
            if prompt_kind == ToolPermissionPromptKind::Mcp {
                "Remember for this session"
            } else {
                "For the current session"
            },
            Some("Session".to_string()),
            Tone::Neutral,
            Some(InlineListSelection::ToolApprovalSession),
        )
        .with_search_value("session temporary temp 2".to_string()),
    ];

    if let Some(target) = persistent_approval_target {
        let short_label = match target {
            PersistentApprovalTarget::ToolLevel => "this tool".to_string(),
            PersistentApprovalTarget::ExactInvocation { display_label }
            | PersistentApprovalTarget::LearnedPattern { display_label, .. }
            | PersistentApprovalTarget::PrefixRule { display_label, .. } => {
                // Middle-truncation keeps both the executable prefix and the
                // trailing flags/destinations that can change what is
                // remembered — head-truncation would hide the dangerous
                // suffix behind the option-row budget.
                vtcode_commons::formatting::truncate_middle(display_label, MAX_APPROVAL_LABEL_CHARS)
            }
        };
        let subtitle = format!("Remember {short_label} in this workspace");
        options.push(
            ui_list::action(
                "Always approve",
                subtitle,
                Some("Permanent".to_string()),
                Tone::Neutral,
                Some(InlineListSelection::ToolApprovalPermanent),
            )
            .with_search_value("always permanent forever save 3".to_string()),
        );
    }

    options.push(ui_list::group_divider());

    options.push(
        ui_list::action(
            if prompt_kind == ToolPermissionPromptKind::Mcp {
                "Cancel"
            } else {
                "Deny Once"
            },
            if prompt_kind == ToolPermissionPromptKind::Mcp {
                "Cancel this tool call"
            } else {
                "Ask again next time"
            },
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApprovalDenyOnce),
        )
        .with_search_value(if prompt_kind == ToolPermissionPromptKind::Mcp {
            "cancel stop reject decline 4".to_string()
        } else {
            "deny no reject once temporary 4".to_string()
        }),
    );

    if matches!(persistent_approval_target, Some(PersistentApprovalTarget::ToolLevel))
        && prompt_kind != ToolPermissionPromptKind::Mcp
    {
        options.push(
            ui_list::action(
                "Always Deny",
                "Block this tool until policy is changed",
                Some("Persistent".to_string()),
                Tone::Neutral,
                Some(InlineListSelection::ToolApproval(false)),
            )
            .with_search_value("deny no reject cancel never always 5".to_string()),
        );
    }

    options
}

pub(super) async fn prompt_tool_permission<S: UiSession + ?Sized>(
    display_name: &str,
    tool_name: &str,
    tool_args: Option<&Value>,
    learning_target: &ApprovalLearningTarget,
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
    // Auto-approve if the approval recorder indicates high-frequency approval
    // history (≥3 approvals with >80% approval rate) for the exact invocation
    // OR any broader learned pattern (e.g. safe `find <subdir>` family).
    if let Some(recorder) = approval_recorder {
        for (key, _label) in learning_target.iter_keys() {
            if recorder.should_auto_approve(key).await {
                tracing::info!(
                    tool = %tool_name,
                    key = %key,
                    "Auto-approved based on learned approval pattern"
                );
                return Ok(HitlDecision::Approved);
            }
        }
    }
    let prompt_kind = tool_permission_prompt_kind(tool_name);
    let command_preview = shell_command_preview_lines(tool_name, tool_args);
    let has_command_preview = command_preview.is_some();
    let diff_preview = command_preview
        .is_none()
        .then(|| tool_args_diff_preview(tool_name, tool_args))
        .flatten();
    let mut description_lines =
        tool_permission_header_lines(tool_name, display_name, has_command_preview, source_thread_label);

    if has_command_preview && let Some(environment_row) = shell_environment_row(tool_name, tool_args) {
        description_lines.push(environment_row);
    }

    if let Some(url) = web_fetch_approval_url(tool_name, tool_args) {
        let url_line = format!("URL: {url}");
        if !description_lines.iter().any(|line| line.contains(&url)) {
            description_lines.push(url_line);
        }
    }

    if let Some(command_lines) = command_preview {
        description_lines.extend(format_command_preview_lines(command_lines));
    }

    if let Some(diff_lines) = diff_preview {
        description_lines.extend(diff_lines);
    }

    let shell_justification = extract_shell_approval_justification(tool_name, tool_args);
    description_lines.extend(build_permission_context_lines(
        approval_reason,
        shell_justification.as_deref(),
        justification,
    ));

    description_lines.push(choose_handling_line("this run"));
    let mut navigation_hint = if prompt_kind == ToolPermissionPromptKind::Mcp {
        APPROVAL_NAVIGATE_CANCEL.to_string()
    } else {
        APPROVAL_NAVIGATE_DENY.to_string()
    };
    if source_thread_label.is_some() {
        navigation_hint.push_str(" • o inspect source thread");
    }

    let options = build_tool_permission_options(prompt_kind, persistent_approval_target);
    let hotkeys = source_thread_label
        .map(|_| {
            vec![TransientHotkey {
                key: TransientHotkeyKey::Char('o'),
                action: TransientHotkeyAction::OpenSourceThread,
            }]
        })
        .unwrap_or_default();

    use vtcode_ui::tui::app::InlineListSelection;
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
    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::WaitingForApproval);
    let _placeholder_guard = PlaceholderGuard::new(handle, default_placeholder);
    let outcome = show_permission_and_wait(
        handle,
        session,
        TransientRequest::List(ListOverlayRequest {
            title: tool_permission_title(has_command_preview).to_string(),
            lines: description_lines,
            footer_hint: Some(navigation_hint),
            items: options,
            selected: Some(default_selection),
            search: None,
            hotkeys,
    status: None,
    plan_markdown: None,
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
        OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Deferred => Ok(cancelled_prompt_decision(prompt_kind)),
        OverlayWaitOutcome::Interrupted => Ok(HitlDecision::Interrupt),
        OverlayWaitOutcome::Exit => Ok(HitlDecision::Exit),
    }
}

/// Show a HITL prompt for a tool that was denied by policy config.
/// Offers an "Enable tool" option that updates the tool policy to Allow,
/// and a "Deny Once" option to skip this invocation.
pub(super) async fn prompt_policy_denied_tool<S: UiSession + ?Sized>(
    tool_name: &str,
    tool_args: Option<&Value>,
    diagnostic: Option<Value>,
    _renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    session: &mut S,
) -> Result<HitlDecision> {
    use vtcode_ui::tui::app::InlineListSelection;

    let mut description_lines = vec![
        format!("Tool: {tool_name}"),
        "## Policy".to_string(),
        "This tool is currently denied by tool policy configuration.".to_string(),
    ];

    if let Some(url) = web_fetch_approval_url(tool_name, tool_args) {
        description_lines.push(format!("URL: {url}"));
    }

    if let Some(diag) = &diagnostic {
        if let Some(impact) = diag["impact"].as_str() {
            push_context_line(&mut description_lines, "Impact", impact);
        }
        if let Some(fix_action) = diag["fix"]["action"].as_str() {
            push_context_line(&mut description_lines, "Fix", fix_action);
        }
        if let Some(files) = diag["fix"]["files"].as_array() {
            for file in files {
                if let Some(f) = file.as_str() {
                    description_lines.push(format!("  - {f}"));
                }
            }
        }
    }

    description_lines.push(choose_handling_line("this run"));

    let options = vec![
        ui_list::action(
            "Enable tool",
            "Allow and continue execution",
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApprovalEnable),
        )
        .with_search_value("enable allow fix policy continue yes 1".to_string()),
        ui_list::group_divider(),
        ui_list::action(
            "Deny Once",
            "Ask again next time",
            None,
            Tone::Neutral,
            Some(InlineListSelection::ToolApprovalDenyOnce),
        )
        .with_search_value("deny no reject skip once 2".to_string()),
    ];

    let navigation_hint = APPROVAL_NAVIGATE_DENY.to_string();

    let request = ListOverlayRequest {
        title: format!("Tool Policy: {tool_name}"),
        lines: description_lines,
        footer_hint: Some(navigation_hint),
        items: options,
        selected: None,
        search: None,
        hotkeys: Vec::new(),
        status: None,
        plan_markdown: None,
    };

    let overlay = TransientRequest::List(request);

    handle.set_progress_phase(vtcode_commons::ui_protocol::ProgressPhase::WaitingForApproval);
    let result =
        show_permission_and_wait(
            handle,
            session,
            overlay,
            ctrl_c_state,
            ctrl_c_notify,
            |submission| match submission {
                TransientSubmission::Selection(InlineListSelection::ToolApprovalEnable) => Some(HitlDecision::Enable),
                TransientSubmission::Selection(InlineListSelection::ToolApprovalDenyOnce) => {
                    Some(HitlDecision::DeniedOnce)
                }
                TransientSubmission::Selection(_) => Some(HitlDecision::DeniedOnce),
                _ => None,
            },
        )
        .await?;

    match result {
        OverlayWaitOutcome::Submitted(decision) => Ok(decision),
        OverlayWaitOutcome::Cancelled | OverlayWaitOutcome::Deferred => Ok(HitlDecision::DeniedOnce),
        OverlayWaitOutcome::Interrupted => Ok(HitlDecision::Interrupt),
        OverlayWaitOutcome::Exit => Ok(HitlDecision::Exit),
    }
}

#[cfg(test)]
mod tests;
