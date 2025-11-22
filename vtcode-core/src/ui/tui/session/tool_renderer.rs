use anstyle::AnsiColor;
use ratatui::{
    style::{Modifier, Style},
    text::Span,
};

use super::super::style::ratatui_style_from_inline;
use super::super::types::{InlineTextStyle, InlineTheme};
use super::message::MessageLine;

pub(super) fn render_tool_segments(
    line: &MessageLine,
    theme: &InlineTheme,
    strip_ansi_fn: impl Fn(&str) -> String,
) -> Vec<Span<'static>> {
    let mut combined = String::new();
    for segment in &line.segments {
        let stripped_text = strip_ansi_fn(&segment.text);
        combined.push_str(&stripped_text);
    }

    if combined.is_empty() {
        return Vec::new();
    }

    render_tool_header_line(&combined, theme)
}

pub(super) fn render_tool_header_line(text: &str, theme: &InlineTheme) -> Vec<Span<'static>> {
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
                        .with_color(Some(AnsiColor::Green.into()))
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
                .with_color(Some(AnsiColor::Cyan.into()))
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
                .with_color(theme.tool_accent.or(Some(AnsiColor::Yellow.into())))
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
        "read" => Some(AnsiColor::Blue.into()),
        "list" => Some(AnsiColor::Green.into()),
        "search" => Some(AnsiColor::Yellow.into()),
        "write" => Some(AnsiColor::Magenta.into()),
        "run" => Some(AnsiColor::Red.into()),
        "git" | "version_control" => Some(AnsiColor::Cyan.into()),
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
