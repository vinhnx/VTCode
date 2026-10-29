#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use super::super::*;
pub(super) use super::helpers::*;
pub(super) use crate::tui::core_tui::style::{ratatui_color_from_ansi, ratatui_style_from_inline};

// ---------------------------------------------------------------------------
// Common test helpers extracted from repeated patterns
// ---------------------------------------------------------------------------

fn make_pty_segment(text: &str) -> InlineSegment {
    InlineSegment {
        text: text.to_string(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn push_pty_line(session: &mut Session, text: &str) {
    session.push_line(InlineMessageKind::Pty, vec![make_pty_segment(text)]);
}

fn make_styled_line(session: &Session, text: &str) -> Line<'static> {
    Line::from(vec![Span::styled(
        text.to_string(),
        ratatui_style_from_inline(&session.default_style(), None),
    )])
}

fn agent_append_line_command(text: &str) -> InlineCommand {
    InlineCommand::AppendLine {
        kind: InlineMessageKind::Agent,
        segments: vec![InlineSegment {
            text: text.to_string(),
            style: Arc::new(InlineTextStyle::default()),
        }],
    }
}

// ---------------------------------------------------------------------------
// Agent section dividers (Tool/Pty -> Agent prose breaks)
// ---------------------------------------------------------------------------

fn divider_positions(texts: &[String]) -> Vec<usize> {
    texts
        .iter()
        .enumerate()
        .filter(|(_, text)| !text.is_empty() && text.chars().all(|ch| ch == '─'))
        .map(|(idx, _)| idx)
        .collect()
}

// ---------------------------------------------------------------------------
// Transcript eviction (bounded snapshot)
// ---------------------------------------------------------------------------

fn collapsed_json_payload() -> String {
    let mut json = String::from("{\n");
    let line_total = ui::INLINE_JSON_COLLAPSE_LINE_THRESHOLD + 5;
    for idx in 0..line_total {
        json.push_str(&format!("  \"key{idx}\": \"value{idx}\",\n"));
    }
    json.push_str("  \"end\": true\n}");
    json
}

mod blocks;
mod dividers;
mod eviction_misc;
mod headers_styles;
mod messages;
mod scrolling;
mod spacing;
mod streaming_state;
mod wrapping;
