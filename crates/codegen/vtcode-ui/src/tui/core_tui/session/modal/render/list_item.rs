//! Modal list-item row rendering: cursor, badges, subtitles, inline editors.

use super::super::layout::ModalRenderStyles;
use super::super::state::ModalListState;
use super::ModalInlineEditor;
use crate::design::constants::GROUP_HEADER_ANCHOR;
use crate::tui::config::constants::ui;
use crate::tui::ui::tui::session::inline_list::{list_cursor, selection_padding, selection_padding_width};
use crate::tui::ui::tui::types::InlineTone;
use ratatui::{prelude::*, style::Color as RatatuiColor};
use std::mem;
use unicode_width::UnicodeWidthStr;

fn wrap_line_to_width(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_owned()];
    }

    if line.is_empty() {
        return vec![String::new()];
    }

    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0).max(1);
        if current_width + ch_width > width && !current.is_empty() {
            rows.push(mem::take(&mut current));
            current_width = 0;
            if ch.is_whitespace() {
                continue;
            }
        }

        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() {
        rows.push(current);
    }

    if rows.is_empty() { vec![String::new()] } else { rows }
}
pub(crate) fn highlight_segments(
    text: &str,
    normal_style: Style,
    highlight_style: Style,
    terms: &[String],
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::styled(String::new(), normal_style)];
    }

    if terms.is_empty() {
        return vec![Span::styled(text.to_owned(), normal_style)];
    }

    let lower = text.to_ascii_lowercase();
    let mut char_offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    char_offsets.push(text.len());
    let char_count = char_offsets.len().saturating_sub(1);
    if char_count == 0 {
        return vec![Span::styled(text.to_owned(), normal_style)];
    }

    let mut highlight_flags = vec![false; char_count];
    for term in terms {
        let needle = term.as_str();
        if needle.is_empty() {
            continue;
        }

        let mut search_start = 0usize;
        while search_start < lower.len() {
            let Some(pos) = lower[search_start..].find(needle) else {
                break;
            };
            let byte_start = search_start + pos;
            let byte_end = byte_start + needle.len();
            let start_index = char_offsets.partition_point(|offset| *offset < byte_start);
            let end_index = char_offsets.partition_point(|offset| *offset < byte_end);
            for flag in highlight_flags.iter_mut().take(end_index.min(char_count)).skip(start_index) {
                *flag = true;
            }
            search_start = byte_end;
        }
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_highlight = highlight_flags.first().copied().unwrap_or(false);
    for (idx, ch) in text.chars().enumerate() {
        let highlight = highlight_flags.get(idx).copied().unwrap_or(false);
        if idx == 0 {
            current_highlight = highlight;
        } else if highlight != current_highlight {
            let style = if current_highlight {
                highlight_style
            } else {
                normal_style
            };
            segments.push(Span::styled(mem::take(&mut current), style));
            current_highlight = highlight;
        }
        current.push(ch);
    }

    if !current.is_empty() {
        let style = if current_highlight {
            highlight_style
        } else {
            normal_style
        };
        segments.push(Span::styled(current, style));
    }

    if segments.is_empty() {
        segments.push(Span::styled(String::new(), normal_style));
    }

    segments
}
pub fn modal_list_item_lines(
    list: &ModalListState,
    visible_index: usize,
    item_index: usize,
    styles: &ModalRenderStyles,
    content_width: usize,
    inline_editor: Option<&ModalInlineEditor>,
    is_selected: bool,
    shortcut_number: Option<usize>,
) -> Vec<Line<'static>> {
    let item = match list.items.get(item_index) {
        Some(i) => i,
        None => {
            tracing::warn!("modal list item index {item_index} out of bounds");
            return vec![Line::default()];
        }
    };
    if item.is_divider {
        // Untitled dividers span the list content width so option groups
        // (approve vs deny) read as clearly separated sections.
        let divider = if item.title.is_empty() {
            ui::INLINE_BLOCK_HORIZONTAL.repeat(content_width.max(8))
        } else {
            item.title.clone()
        };
        let selection_pad = selection_padding();
        let mut spans = Vec::new();
        if !selection_pad.is_empty() {
            spans.push(Span::raw(selection_pad));
        }
        spans.push(Span::styled(divider, styles.divider));
        return vec![Line::from(spans)];
    }

    let indent = "  ".repeat(item.indent as usize);
    let gutter_width = selection_padding_width();
    let blank_gutter = selection_padding();
    let cursor_indicator = list_cursor(is_selected);

    let cursor_style = if is_selected {
        styles.highlight
    } else {
        styles.selectable
    };
    let mut primary_spans = Vec::new();
    if gutter_width > 0 {
        primary_spans.push(Span::styled(cursor_indicator, cursor_style));
    }

    // Numbered shortcut badge (`1.`–`9.`) for search-less modals, mirroring
    // the digit keys that jump to each option. Non-selectable rows (dividers,
    // separators) carry no number.
    if let Some(number) = shortcut_number {
        primary_spans.push(Span::styled(format!("{number}."), styles.detail));
        primary_spans.push(Span::raw(" "));
    }

    if !indent.is_empty() {
        primary_spans.push(Span::raw(indent.clone()));
    }

    if let Some(badge) = &item.badge {
        let badge_label = format!("[{badge}]");
        primary_spans.push(Span::styled(badge_label, modal_badge_style(badge.as_str(), item.badge_tone, styles)));
        primary_spans.push(Span::raw(" "));
    }

    let is_header_row = item.is_header();
    let title_style = if item.is_hint() {
        styles.detail
    } else if is_selected && item.selection.is_some() {
        styles.highlight
    } else if item.selection.is_some() {
        styles.selectable
    } else if is_header_row {
        styles.header
    } else {
        styles.detail
    };

    if is_header_row {
        primary_spans.push(Span::styled(GROUP_HEADER_ANCHOR, title_style));
    }
    let title_spans = highlight_segments(item.title.as_str(), title_style, styles.search_match, list.highlight_terms());
    primary_spans.extend(title_spans);

    // Live value for setting rows: trailing column after a dimmed separator.
    // Tone follows `badge_tone` (On → success, Off/unset → dimmed, else accent).
    if let Some(value) = &item.value {
        // Pad short titles so values line up as a column when possible.
        let title_width: usize = item.title.chars().count();
        let target = crate::design::constants::VALUE_COL;
        if title_width < target {
            primary_spans.push(Span::raw(" ".repeat(target - title_width)));
        } else {
            primary_spans.push(Span::raw("  "));
        }
        primary_spans.push(Span::styled("·  ", styles.detail));
        let value_style = if is_selected {
            styles.highlight
        } else if item.badge_tone == InlineTone::Neutral {
            styles.detail
        } else {
            tone_style(item.badge_tone, styles)
        };
        primary_spans.extend(highlight_segments(
            value.as_str(),
            value_style,
            styles.search_match,
            list.highlight_terms(),
        ));
    }
    // Group headers open with the shared `»` anchor so sections read as
    // labeled headings (`» Title`) instead of plain bold text.

    // Shared rhythm: every selectable row keeps one blank separator row
    // after it so dense subtitle lists (settings, model picker, permission
    // groups) stay scannable. Dividers keep a single full-width rule.
    let mut lines = Vec::new();
    if is_header_row {
        if visible_index > 0 {
            lines.push(Line::default());
        }
        lines.push(Line::from(primary_spans));
        lines.push(Line::default());
    } else {
        lines.push(Line::from(primary_spans));
    }

    if let Some(subtitle) = &item.subtitle {
        let indent_width = item.indent as usize * 2;
        let wrapped_width = content_width.saturating_sub(indent_width).max(1);
        let wrapped_lines = wrap_line_to_width(subtitle.as_str(), wrapped_width);

        for wrapped in wrapped_lines {
            let mut secondary_spans = Vec::new();
            if !blank_gutter.is_empty() {
                secondary_spans.push(Span::raw(blank_gutter.clone()));
            }
            if !indent.is_empty() {
                secondary_spans.push(Span::raw(indent.clone()));
            }
            let subtitle_spans =
                highlight_segments(wrapped.as_str(), styles.detail, styles.search_match, list.highlight_terms());
            secondary_spans.extend(subtitle_spans);
            lines.push(Line::from(secondary_spans));
        }
    }

    if let Some(editor) = inline_editor
        && editor.item_index == item_index
    {
        let mut editor_spans = Vec::new();
        if !blank_gutter.is_empty() {
            editor_spans.push(Span::raw(blank_gutter));
        }
        if !indent.is_empty() {
            editor_spans.push(Span::raw(indent.clone()));
        }

        editor_spans.push(Span::styled(format!("{} ", editor.label), styles.header));
        if editor.text.is_empty() {
            if let Some(placeholder) = editor.placeholder.as_ref() {
                editor_spans.push(Span::styled(placeholder.clone(), styles.detail));
            }
        } else {
            editor_spans.push(Span::styled(editor.text.clone(), styles.selectable));
        }

        if editor.active {
            editor_spans.push(Span::styled("▌", styles.highlight));
        }

        lines.push(Line::from(editor_spans));
    }

    if item.selection.is_some() {
        lines.push(Line::default());
    }
    lines
}
pub(crate) fn tone_style(tone: InlineTone, styles: &ModalRenderStyles) -> Style {
    match tone {
        InlineTone::Neutral => styles.badge,
        InlineTone::Accent => styles.accent,
        InlineTone::Success => styles.success,
        InlineTone::Warning => styles.warning,
        InlineTone::Danger => styles.danger,
        InlineTone::Current => styles.accent.add_modifier(Modifier::BOLD),
    }
}
pub(crate) fn modal_badge_style(badge: &str, tone: InlineTone, styles: &ModalRenderStyles) -> Style {
    if tone != InlineTone::Neutral {
        return tone_style(tone, styles);
    }
    // Fallback for callers that set only a badge label (no tone).
    match badge {
        "Active" | "Action" | "Current" => styles.header.add_modifier(Modifier::BOLD),
        "Read-only" => styles.detail.add_modifier(Modifier::ITALIC),
        _ => styles.badge,
    }
}
