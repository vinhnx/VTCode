use anstyle::Effects;
use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

use crate::tui::config::constants::ui;

use super::super::message::MessageLine;
use crate::tui::core_tui::types::InlineMessageKind;

/// Rule fill pattern for Fieldset-style info/warning/error blocks.
///
/// Mirrors `ratatui_cheese::fieldset::FieldsetFill`, mapping each message kind
/// to a distinct fill: Error → `Slash`, Info → `Dash`, Warning → `Thick`. The
/// Unicode glyphs fall back to ASCII on terminals without Unicode support.
pub(super) fn rule_fill(kind: InlineMessageKind, border_type: ratatui::widgets::BorderType) -> &'static str {
    let unicode = matches!(border_type, ratatui::widgets::BorderType::Rounded);
    match kind {
        // Slash fill (`/`) — already ASCII-safe.
        InlineMessageKind::Error => "/",
        // Thick fill (`━`) with an ASCII fallback.
        InlineMessageKind::Warning => {
            if unicode {
                "━"
            } else {
                "="
            }
        }
        // Dash fill (`─`) with an ASCII fallback.
        _ => {
            if unicode {
                ui::INLINE_BLOCK_HORIZONTAL
            } else {
                "-"
            }
        }
    }
}

/// Check if trimmed, ANSI-stripped text starts with a tool summary prefix.
///
/// Shared by both `is_tool_summary_line` and `reflow_tool_lines` to avoid
/// duplicating the prefix list (DRY).
pub(super) fn has_summary_prefix(text: &str) -> bool {
    let stripped = super::super::text_utils::strip_ansi_codes(text);
    stripped.starts_with("• ") || stripped.starts_with("  └ ") || stripped.starts_with("  │ ")
}

pub(super) fn is_tool_summary_line(message: &MessageLine) -> bool {
    let text: String = message.segments.iter().map(|segment| segment.text.as_str()).collect();
    has_summary_prefix(&text)
}

pub(super) fn agent_code_continuation_prefix(message: &MessageLine) -> Option<String> {
    let first_segment = message.segments.iter().find(|segment| !segment.text.is_empty())?;
    if !first_segment.style.effects.contains(Effects::DIMMED) {
        return None;
    }

    numbered_code_gutter_prefix(&first_segment.text)
}

fn numbered_code_gutter_prefix(text: &str) -> Option<String> {
    let mut chars = text.char_indices().peekable();
    let mut prefix_end = 0usize;

    while let Some((idx, ch)) = chars.peek().copied() {
        if ch == ' ' {
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }

    let mut saw_digits = false;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            saw_digits = true;
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if !saw_digits {
        return None;
    }

    if let Some((idx, '-')) = chars.peek().copied() {
        prefix_end = idx + 1;
        chars.next();

        let mut saw_range_digits = false;
        while let Some((idx, ch)) = chars.peek().copied() {
            if ch.is_ascii_digit() {
                saw_range_digits = true;
                prefix_end = idx + ch.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        if !saw_range_digits {
            return None;
        }
    }

    let mut trailing_spaces = 0usize;
    while let Some((idx, ch)) = chars.peek().copied() {
        if ch == ' ' {
            trailing_spaces += 1;
            prefix_end = idx + ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    if trailing_spaces < 2 {
        return None;
    }

    Some(" ".repeat(UnicodeWidthStr::width(&text[..prefix_end])))
}

pub(super) fn split_tool_spans(spans: Vec<Span<'static>>) -> Vec<Vec<Span<'static>>> {
    let mut lines: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();

    for span in spans {
        let style = span.style;
        let text = span.content.into_owned();
        let mut parts = text.split('\n').peekable();
        while let Some(part) = parts.next() {
            if !part.is_empty() {
                current.push(Span::styled(part.to_string(), style));
            }
            if parts.peek().is_some() {
                lines.push(std::mem::take(&mut current));
            }
        }
    }

    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }

    lines
}

pub(super) fn is_info_box_line(message: &MessageLine) -> bool {
    matches!(message.kind, InlineMessageKind::Error | InlineMessageKind::Warning)
        || (message.kind == InlineMessageKind::Info && !is_tool_summary_line(message))
}
