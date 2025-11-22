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
    index: usize,
    lines: &[MessageLine],
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
        let prev_is_pty = index
            .checked_sub(1)
            .and_then(|prev| lines.get(prev))
            .map(|prev| prev.kind == InlineMessageKind::Pty)
            .unwrap_or(false);
        if !prev_is_pty {
            let mut combined = String::new();
            for segment in &line.segments {
                combined.push_str(segment.text.as_str());
            }
            let header_text = if combined.trim().is_empty() {
                ui::INLINE_PTY_PLACEHOLDER.to_string()
            } else {
                combined.trim().to_string()
            };
            let label_style = InlineTextStyle::default()
                .with_color(theme.primary.or(theme.foreground))
                .bold();
            spans.push(Span::styled(
                format!("[{}]", ui::INLINE_PTY_HEADER_LABEL),
                ratatui_style_from_inline(&label_style, theme.foreground),
            ));
            spans.push(Span::raw(" "));

            let output_text = if header_text.lines().count() > 30 {
                let lines: Vec<&str> = header_text.lines().collect();
                let start = lines.len().saturating_sub(30);
                format!(
                    "[... {} lines truncated ...]\n{}",
                    lines.len() - 30,
                    lines[start..].join("\n")
                )
            } else {
                header_text.clone()
            };

            spans.push(Span::raw(output_text));
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
    let mut combined = String::new();
    for segment in &line.segments {
        let stripped_text = ansi_utils::strip_ansi_codes(&segment.text);
        combined.push_str(&stripped_text);
    }

    if combined.is_empty() {
        return Vec::new();
    }

    render_tool_header_line(&combined, theme)
}

fn render_tool_header_line(text: &str, theme: &InlineTheme) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let indent_len = text.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent: String = text.chars().take(indent_len).collect();
    let mut remaining = if indent_len < text.len() {
        &text[indent_len..]
    } else {
        ""
    };

    if !indent.is_empty() {
        let mut indent_style = InlineTextStyle::default();
        indent_style.color = theme.tool_body.or(theme.foreground);
        spans.push(Span::styled(
            indent,
            ratatui_style_from_inline(&indent_style, theme.foreground),
        ));
    }

    if remaining.is_empty() {
        return spans;
    }

    remaining = strip_tool_status_prefix(remaining);
    if remaining.is_empty() {
        return spans;
    }

    let (name, tail) = if remaining.starts_with('[') {
        if let Some(end) = remaining.find(']') {
            let name = &remaining[1..end];
            let tail = &remaining[end + 1..];
            (name, tail)
        } else {
            (remaining, "")
        }
    } else {
        let mut name_end = remaining.len();
        for (index, character) in remaining.char_indices() {
            if character.is_whitespace() {
                name_end = index;
                break;
            }
        }
        remaining.split_at(name_end)
    };

    if !name.is_empty() {
        let accent_style = accent_style(theme);
        spans.push(Span::styled("[", accent_style.add_modifier(Modifier::BOLD)));

        let tool_name_style = tool_inline_style(name, theme);
        spans.push(Span::styled(
            name.to_string(),
            ratatui_style_from_inline(&tool_name_style, theme.foreground),
        ));

        spans.push(Span::styled(
            "] ",
            accent_style.add_modifier(Modifier::BOLD),
        ));
    }

    let trimmed_tail = tail.trim_start();
    if !trimmed_tail.is_empty() {
        let parts: Vec<&str> = trimmed_tail.split(" · ").collect();
        if parts.len() > 1 {
            let action = parts[0];
            let body_style =
                InlineTextStyle::default().with_color(theme.tool_body.or(theme.foreground));

            render_styled_action_text(&mut spans, action, &body_style, theme);

            let max_parts = 3;
            for (i, part) in parts[1..].iter().enumerate() {
                if i >= max_parts {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(
                        "· ...",
                        accent_style(theme).add_modifier(Modifier::DIM),
                    ));
                    break;
                }
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    "·",
                    accent_style(theme).add_modifier(Modifier::DIM),
                ));
                spans.push(Span::raw(" "));

                let param_parts: Vec<&str> = part.split(": ").collect();
                if param_parts.len() > 1 {
                    spans.push(Span::styled(
                        format!("{}: ", param_parts[0]),
                        accent_style(theme).add_modifier(Modifier::BOLD),
                    ));

                    let value_style = InlineTextStyle::default()
                        .with_color(Some(anstyle::AnsiColor::Green.into()))
                        .bold();
                    spans.push(Span::styled(
                        param_parts[1].to_string(),
                        ratatui_style_from_inline(&value_style, theme.foreground),
                    ));
                } else {
                    spans.push(Span::styled(
                        part.to_string(),
                        ratatui_style_from_inline(&body_style, theme.foreground),
                    ));
                }
            }
        } else {
            let body_style =
                InlineTextStyle::default().with_color(theme.tool_body.or(theme.foreground));

            let mut simplified_text = simplify_tool_display(trimmed_tail);
            if simplified_text.len() > 100 {
                simplified_text = simplified_text.chars().take(97).collect::<String>() + "...";
            }
            spans.push(Span::styled(
                simplified_text,
                ratatui_style_from_inline(&body_style, theme.foreground),
            ));
        }
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
