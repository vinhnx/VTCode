use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use super::super::style::ratatui_style_from_inline;
use super::super::types::{InlineMessageKind, InlineTextStyle, InlineTheme};
use super::ansi_utils;
use super::message::{MessageLabels, MessageLine};
use crate::config::constants::ui;

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
            let style = ratatui_style_from_inline(&segment.style, fallback);
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
        }
        spans.push(Span::styled(segment.text.clone(), style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

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
            ui::INLINE_AGENT_QUOTE_PREFIX.to_string(),
            prefix_style,
        ));
    }

    if let Some(label) = labels.agent.clone() {
        if !label.is_empty() {
            let label_style = ratatui_style_from_inline(&prefix_style_fn(line), theme.foreground);
            spans.push(Span::styled(label, label_style));
        }
    }

    spans
}

fn render_tool_segments(line: &MessageLine, theme: &InlineTheme) -> Vec<Span<'static>> {
    // Render tool output without header decorations - just display segments directly
    let mut spans = Vec::new();
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, theme.foreground);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    spans
}


fn render_styled_action_text(
    spans: &mut Vec<Span<'static>>,
    action: &str,
    body_style: &InlineTextStyle,
    theme: &InlineTheme,
) {
    let words: Vec<&str> = action.split_whitespace().collect();

    for (i, word) in words.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }

        if *word == "in" {
            let in_style = InlineTextStyle::default()
                .with_color(Some(anstyle::AnsiColor::Cyan.into()))
                .italic();
            spans.push(Span::styled(
                word.to_string(),
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
                .with_color(
                    theme
                        .tool_accent
                        .or(Some(anstyle::AnsiColor::Yellow.into())),
                )
                .bold();
            spans.push(Span::styled(
                word.to_string(),
                ratatui_style_from_inline(&action_style, theme.foreground),
            ));
        } else {
            spans.push(Span::styled(
                word.to_string(),
                ratatui_style_from_inline(body_style, theme.foreground),
            ));
        }
    }
}

fn strip_tool_status_prefix(text: &str) -> &str {
    let trimmed = text.trim_start();
    const STATUS_ICONS: [&str; 4] = ["✓", "✗", "~", "✕"];
    for icon in STATUS_ICONS {
        if trimmed.starts_with(icon) {
            return trimmed[icon.len()..].trim_start();
        }
    }
    text
}

fn simplify_tool_display(text: &str) -> String {
    let simplified = if text.starts_with("file ") {
        text.replacen("file ", "accessing ", 1)
    } else if text.starts_with("path: ") {
        text.replacen("path: ", "file: ", 1)
    } else if text.contains(" → file ") {
        text.replace(" → file ", " → ")
    } else if text.starts_with("grep ") {
        text.replacen("grep ", "searching for ", 1)
    } else if text.starts_with("find ") {
        text.replacen("find ", "finding ", 1)
    } else if text.starts_with("list ") {
        text.replacen("list ", "listing ", 1)
    } else {
        text.to_string()
    };

    format_tool_parameters(&simplified)
}

fn format_tool_parameters(text: &str) -> String {
    let mut formatted = text.to_string();

    if formatted.contains("pattern: ") {
        formatted = formatted.replace("pattern: ", "matching '");
        if formatted.contains(" · ") {
            formatted = formatted.replacen(" · ", "' · ", 1);
        } else if formatted.contains("  ") {
            formatted = formatted.replacen("  ", "' ", 1);
        } else {
            formatted.push('\'');
        }
    }

    if formatted.contains("path: ") {
        formatted = formatted.replace("path: ", "in '");
        if formatted.contains(" · ") {
            formatted = formatted.replacen(" · ", "' · ", 1);
        } else if formatted.contains("  ") {
            formatted = formatted.replacen("  ", "' ", 1);
        } else {
            formatted.push('\'');
        }
    }

    formatted
}

fn normalize_tool_name(tool_name: &str) -> String {
    match tool_name.to_lowercase().as_str() {
        "grep" | "rg" | "ripgrep" | "grep_file" | "search" | "find" | "ag" => "search".to_string(),
        "list" | "ls" | "dir" | "list_files" => "list".to_string(),
        "read" | "cat" | "file" | "read_file" => "read".to_string(),
        "write" | "edit" | "save" | "insert" | "edit_file" => "write".to_string(),
        "run" | "command" | "bash" | "sh" => "run".to_string(),
        _ => tool_name.to_string(),
    }
}

fn tool_inline_style(tool_name: &str, theme: &InlineTheme) -> InlineTextStyle {
    let normalized_name = normalize_tool_name(tool_name);
    let mut style = InlineTextStyle::default().bold();

    style.color = match normalized_name.to_lowercase().as_str() {
        "read" => Some(anstyle::AnsiColor::Blue.into()),
        "list" => Some(anstyle::AnsiColor::Green.into()),
        "search" => Some(anstyle::AnsiColor::Yellow.into()),
        "write" => Some(anstyle::AnsiColor::Magenta.into()),
        "run" => Some(anstyle::AnsiColor::Red.into()),
        "git" | "version_control" => Some(anstyle::AnsiColor::Cyan.into()),
        _ => theme.tool_accent.or(theme.primary).or(theme.foreground),
    };

    style
}

fn accent_style(theme: &InlineTheme) -> Style {
    let accent_inline = InlineTextStyle {
        color: theme.primary.or(theme.foreground),
        ..InlineTextStyle::default()
    };
    ratatui_style_from_inline(&accent_inline, theme.foreground)
}
