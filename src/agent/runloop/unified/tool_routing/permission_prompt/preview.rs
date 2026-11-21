use super::*;

pub(super) fn format_command_preview_line(line: &str) -> String {
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
pub(super) fn format_command_preview_lines(command_lines: Vec<String>) -> Vec<String> {
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
pub(super) fn prefix_shell_marker(first_row: Option<&mut String>) {
    if let Some(row) = first_row
        && let Some(body) = row.strip_prefix('`')
    {
        *row = format!("`$ {body}");
    }
}

/// Friendly label for the agent's goal in the permission popup. The modal
/// renderer styles this as a subordinate context row (no bullet), matching
/// the previous `Reason:` treatment without the technical `WHY` header.
pub(super) const AGENT_GOAL_LABEL: &str = "What the agent is trying to do";

pub(super) fn push_context_line(description_lines: &mut Vec<String>, label: &str, value: &str) -> bool {
    push_capped_context_line(description_lines, label, value, MAX_CONTEXT_LINE_CHARS)
}

pub(super) fn push_capped_context_line(
    description_lines: &mut Vec<String>,
    label: &str,
    value: &str,
    max_chars: usize,
) -> bool {
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
pub(super) fn compact_justification_lines(just: &vtcode_core::tools::ToolJustification) -> Vec<String> {
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

pub(super) fn tool_args_diff_preview(tool_name: &str, tool_args: Option<&Value>) -> Option<Vec<String>> {
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
