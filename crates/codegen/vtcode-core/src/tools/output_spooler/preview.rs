use super::*;
use vtcode_commons::preview::{
    condense_text_bytes, excerpt_text_lines_with_limit, format_hidden_lines_summary, tail_preview_text,
};

pub(super) const CONDENSE_HEAD_BYTES: usize = 8_000;
pub(super) const CONDENSE_TAIL_BYTES: usize = 4_000;
const INSPECTION_PREVIEW_HEAD_BYTES: usize = 4 * 1024;
const INSPECTION_PREVIEW_TAIL_BYTES: usize = 2 * 1024;
const COMMAND_PREVIEW_MAX_BYTES: usize = 6 * 1024;
const COMMAND_PREVIEW_MAX_LINES: usize = 80;
const PREVIEW_NOTICE_RESERVE_BYTES: usize = 64;

pub(super) fn condense_content(content: &str) -> String {
    condense_text_bytes(content, CONDENSE_HEAD_BYTES, CONDENSE_TAIL_BYTES)
}

fn render_line_bounded_preview(content: &str, max_lines: usize) -> String {
    let preview = excerpt_text_lines_with_limit(content, max_lines, max_lines / 2);
    if preview.hidden_count == 0 {
        return content.to_string();
    }

    let mut lines = Vec::with_capacity(preview.head.len() + preview.tail.len() + 1);
    lines.extend(preview.head.into_iter().map(str::to_string));
    lines.push(format_hidden_lines_summary(preview.hidden_count));
    lines.extend(preview.tail.into_iter().map(str::to_string));
    lines.join("\n")
}

fn command_preview_byte_limit(max_preview_bytes: Option<usize>) -> usize {
    max_preview_bytes
        .unwrap_or(COMMAND_PREVIEW_MAX_BYTES)
        .min(COMMAND_PREVIEW_MAX_BYTES)
}

/// Shared budget half of the bounded command previews below: the byte limit
/// plus the content budget (limit minus notice reserve, at least 1 byte).
/// Returns `None` when previews are disabled so inspection and verification
/// share one zero-guard instead of drifting apart.
fn preview_content_budget(max_preview_bytes: Option<usize>) -> Option<(usize, usize)> {
    let byte_limit = command_preview_byte_limit(max_preview_bytes);
    if byte_limit == 0 {
        return None;
    }
    Some((byte_limit, byte_limit.saturating_sub(PREVIEW_NOTICE_RESERVE_BYTES).max(1)))
}

pub(super) fn inspection_preview_content(content: &str, max_preview_bytes: Option<usize>) -> String {
    let Some((byte_limit, content_budget)) = preview_content_budget(max_preview_bytes) else {
        return String::new();
    };

    let head_bytes = content_budget.saturating_mul(2).div_ceil(3).min(INSPECTION_PREVIEW_HEAD_BYTES);
    let tail_bytes = content_budget.saturating_sub(head_bytes).min(INSPECTION_PREVIEW_TAIL_BYTES);
    let byte_bounded = condense_text_bytes(content, head_bytes, tail_bytes);
    let line_bounded = render_line_bounded_preview(&byte_bounded, COMMAND_PREVIEW_MAX_LINES);
    truncate_byte_budget(&line_bounded, byte_limit, "")
}

pub(super) fn verification_preview_content(content: &str, max_preview_bytes: Option<usize>) -> String {
    let Some((byte_limit, content_budget)) = preview_content_budget(max_preview_bytes) else {
        return String::new();
    };

    let preview = tail_preview_text(content, content_budget, COMMAND_PREVIEW_MAX_LINES.saturating_sub(1));
    truncate_byte_budget(&preview, byte_limit, "")
}

/// Render the bounded model-facing preview for command-session output already
/// held in memory by a response producer.
///
/// This deliberately accepts content rather than a spool path: consumers must
/// not reopen spool files while shaping the model response.
pub(crate) fn command_preview_content(
    tool_name: &str,
    response: &Value,
    content: &str,
    max_preview_bytes: Option<usize>,
) -> String {
    let args = json!({
        "action": "run",
        "command": response.get("command").cloned().unwrap_or(Value::Null),
    });
    match crate::tools::tool_intent::classify_shell_activity(tool_name, &args) {
        crate::tools::tool_intent::ShellActivity::Inspection => inspection_preview_content(content, max_preview_bytes),
        crate::tools::tool_intent::ShellActivity::Verification | crate::tools::tool_intent::ShellActivity::Mutation => {
            verification_preview_content(content, max_preview_bytes)
        }
    }
}
