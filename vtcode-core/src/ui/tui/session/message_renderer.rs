use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use super::super::style::ratatui_style_from_inline;
use super::super::types::{InlineMessageKind, InlineTextStyle, InlineTheme};
use super::message::{MessageLabels, MessageLine};
use crate::config::constants::ui;

// Note: format_tool_parameters and simplify_tool_display are available in super::text_utils
// if needed for future use.

#[allow(dead_code)]
pub(super) fn render_message_spans(
    line: &MessageLine,
    theme: &InlineTheme,
    labels: &MessageLabels,
    prefix_text_fn: impl Fn(InlineMessageKind) -> Option<String>,
    prefix_style_fn: impl Fn(&MessageLine) -> InlineTextStyle,
    text_fallback_fn: impl Fn(InlineMessageKind) -> Option<anstyle::Color>,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if line.kind == InlineMessageKind::Agent {
        spans.extend(agent_prefix_spans(line, theme, labels, &prefix_style_fn));
    } else if let Some(prefix) = prefix_text_fn(line.kind) {
        let style = prefix_style_fn(line);
        spans.push(Span::styled(
            prefix,
            ratatui_style_from_inline(&style, theme.foreground),
        ));
    }

    if line.kind == InlineMessageKind::Agent {
        spans.push(Span::raw(ui::INLINE_AGENT_MESSAGE_LEFT_PADDING));
    }

    if line.segments.is_empty() {
        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }
        return spans;
    }

    if line.kind == InlineMessageKind::Tool {
        let tool_spans = render_tool_segments(line, theme);
        if tool_spans.is_empty() {
            spans.push(Span::raw(String::new()));
        } else {
            spans.extend(tool_spans);
        }
        return spans;
    }

    if line.kind == InlineMessageKind::Pty {
        // Render PTY content directly without header decoration
        let fallback = text_fallback_fn(line.kind).or(theme.foreground);
        for segment in &line.segments {
            let mut dim_style = InlineTextStyle::default().dim();
            dim_style.color = segment.style.color;
            dim_style.bg_color = segment.style.bg_color;
            dim_style.effects |= segment.style.effects;
            let style = ratatui_style_from_inline(&dim_style, fallback);
            spans.push(Span::styled(segment.text.clone(), style));
        }
        if !spans.is_empty() {
            return spans;
        }
    }

    let fallback = text_fallback_fn(line.kind).or(theme.foreground);
    for segment in &line.segments {
        let mut style = ratatui_style_from_inline(&segment.style, fallback);
        if line.kind == InlineMessageKind::Agent {
            style = style.add_modifier(Modifier::ITALIC);
            style = style.remove_modifier(Modifier::BOLD);
        } else if line.kind == InlineMessageKind::User {
            style = style.add_modifier(Modifier::BOLD);
        }
        spans.push(Span::styled(segment.text.clone(), style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

#[allow(dead_code)]
fn agent_prefix_spans(
    line: &MessageLine,
    theme: &InlineTheme,
    labels: &MessageLabels,
    prefix_style_fn: &impl Fn(&MessageLine) -> InlineTextStyle,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let prefix_style = ratatui_style_from_inline(&prefix_style_fn(line), theme.foreground);
    if !ui::INLINE_AGENT_QUOTE_PREFIX.is_empty() {
        spans.push(Span::styled(
            ui::INLINE_AGENT_QUOTE_PREFIX.to_owned(),
            prefix_style,
        ));
    }

    if let Some(label) = &labels.agent
        && !label.is_empty()
    {
        let label_style = ratatui_style_from_inline(&prefix_style_fn(line), theme.foreground);
        spans.push(Span::styled(label.clone(), label_style));
    }

    spans
}

#[allow(dead_code)]
fn render_tool_segments(line: &MessageLine, theme: &InlineTheme) -> Vec<Span<'static>> {
    // Render tool output without header decorations - just display segments directly
    let mut spans = Vec::new();
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, theme.foreground);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    spans
}

#[allow(dead_code)]
fn render_styled_action_text(
    spans: &mut Vec<Span<'static>>,
    action: &str,
    body_style: &InlineTextStyle,
    theme: &InlineTheme,
) {
    // Iterate directly without collecting to Vec
    for (i, word) in action.split_whitespace().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }

        if word == "in" {
            let in_style = InlineTextStyle::default()
                .with_color(Some(anstyle::AnsiColor::Cyan.into()))
                .italic();
            spans.push(Span::styled(
                word.to_owned(),
                ratatui_style_from_inline(&in_style, theme.foreground),
            ));
        } else if i < 2
            && (word.starts_with("List")
                || word.starts_with("Read")
                || word.starts_with("Write")
                || word.starts_with("Find")
                || word.starts_with("Search")
                || word.starts_with("Run"))
        {
            let action_style = InlineTextStyle::default()
                .with_color(theme.tool_accent.or(Some(anstyle::AnsiColor::Cyan.into())))
                .bold();
            spans.push(Span::styled(
                word.to_owned(),
                ratatui_style_from_inline(&action_style, theme.foreground),
            ));
        } else {
            spans.push(Span::styled(
                word.to_owned(),
                ratatui_style_from_inline(body_style, theme.foreground),
            ));
        }
    }
}

#[allow(dead_code)]
fn strip_tool_status_prefix(text: &str) -> &str {
    let trimmed = text.trim_start();
    const STATUS_ICONS: [&str; 4] = ["✓", "✗", "~", "✕"];
    for icon in STATUS_ICONS {
        if let Some(rest) = trimmed.strip_prefix(icon) {
            return rest.trim_start();
        }
    }
    text
}

#[allow(dead_code)]
fn normalize_tool_name(tool_name: &str) -> &'static str {
    match tool_name.to_lowercase().as_str() {
        "grep" | "rg" | "ripgrep" | "grep_file" | "search" | "find" | "ag" => "search",
        "list" | "ls" | "dir" | "list_files" => "list",
        "read" | "cat" | "file" | "read_file" => "read",
        "write" | "edit" | "save" | "insert" | "edit_file" => "write",
        "run" | "command" | "bash" | "sh" => "run",
        _ => "other",
    }
}

#[allow(dead_code)]
fn tool_inline_style(tool_name: &str, theme: &InlineTheme) -> InlineTextStyle {
    let normalized_name = normalize_tool_name(tool_name);
    let mut style = InlineTextStyle::default().bold();

    style.color = match normalized_name {
        "read" => Some(anstyle::AnsiColor::Cyan.into()),
        "list" => Some(anstyle::AnsiColor::Green.into()),
        "search" => Some(anstyle::AnsiColor::Cyan.into()),
        "write" => Some(anstyle::AnsiColor::Magenta.into()),
        "run" => Some(anstyle::AnsiColor::Red.into()),
        "git" | "version_control" => Some(anstyle::AnsiColor::Cyan.into()),
        _ => theme.tool_accent.or(theme.primary).or(theme.foreground),
    };

    style
}

#[allow(dead_code)]
fn accent_style(theme: &InlineTheme) -> Style {
    let accent_inline = InlineTextStyle {
        color: theme.primary.or(theme.foreground),
        ..InlineTextStyle::default()
    };
    ratatui_style_from_inline(&accent_inline, theme.foreground)
}
