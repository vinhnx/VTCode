//! Modal instruction-block rendering: markdown-ish headers, code blocks, metadata rows.

use super::super::layout::ModalRenderStyles;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::style::ratatui_style_from_inline;
use crate::tui::ui::shell_syntax::{ShellLineStyles, shell_syntax_segments};
use ratatui::{prelude::*, style::Color as RatatuiColor};
use unicode_width::UnicodeWidthStr;

enum DiffLineKind {
    Addition,
    Deletion,
    HunkHeader,
}

fn classify_diff_line(line: &str) -> Option<DiffLineKind> {
    let trimmed = line.trim();
    if trimmed.starts_with("@@ ") {
        Some(DiffLineKind::HunkHeader)
    } else if trimmed.starts_with('+') {
        Some(DiffLineKind::Addition)
    } else if trimmed.starts_with('-') {
        Some(DiffLineKind::Deletion)
    } else {
        None
    }
}

fn diff_line_style(kind: &DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Addition => Style::default().fg(RatatuiColor::LightGreen),
        DiffLineKind::Deletion => Style::default().fg(RatatuiColor::LightRed),
        DiffLineKind::HunkHeader => Style::default().add_modifier(Modifier::DIM),
    }
}

/// Plan-approval header rows already carry their own markers (`1. …`, `… and
/// N more`), so prepending the generic `•` bullet would double-mark them.
/// Detect those rows and render them as plain body text without a bullet.
pub(super) fn is_numbered_step_row(trimmed: &str) -> bool {
    let Some((number, rest)) = trimmed.split_once('.') else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit()) && !rest.trim().is_empty()
}

pub(super) fn is_plan_overflow_row(trimmed: &str) -> bool {
    trimmed.starts_with('…') || trimmed.starts_with("...") || trimmed.starts_with("·")
}

/// Split `Label: value` metadata rows (`Risk`, `Source`, the permission-popup
/// agent goal, the approval sandbox posture, …) so the label can render
/// muted and the value in body style. Returns the trimmed label and value;
/// `Tool:` stays a header and never matches here.
pub(super) fn split_context_row(trimmed: &str) -> Option<(&str, &str)> {
    const CONTEXT_LABELS: &[&str] = &[
        "Reason",
        "Risk",
        "Expected",
        "Suggestion",
        "Impact",
        "Fix",
        "Source",
        "Summary",
        "Plan",
        "Environment",
        "What the agent is trying to do",
        "Requested from",
    ];
    let (label, value) = trimmed.split_once(':')?;
    let label = label.trim();
    let value = value.trim();
    if !CONTEXT_LABELS.contains(&label) || value.is_empty() {
        return None;
    }
    Some((label, value))
}

pub(super) fn modal_instruction_lines(
    area: Rect,
    instructions: &[String],
    styles: &ModalRenderStyles,
) -> Vec<Line<'static>> {
    fn parse_instruction_highlight_markup(text: &str) -> (String, bool) {
        let trimmed = text.trim();
        match trimmed
            .strip_prefix("**")
            .and_then(|value| value.strip_suffix("**"))
            .map(str::trim)
        {
            Some(value) if !value.is_empty() => (value.to_string(), true),
            _ => (trimmed.to_string(), false),
        }
    }

    fn wrap_instruction_lines(text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![text.to_owned()];
        }

        let mut lines = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            let word_width = UnicodeWidthStr::width(word);
            if current.is_empty() {
                current.push_str(word);
                continue;
            }

            let current_width = UnicodeWidthStr::width(current.as_str());
            let candidate_width = current_width.saturating_add(1).saturating_add(word_width);
            if candidate_width > width {
                lines.push(current);
                current = word.to_owned();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        if lines.is_empty() { vec![text.to_owned()] } else { lines }
    }

    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }

    let mut items: Vec<Vec<Line<'static>>> = Vec::new();
    let mut first_content_rendered = false;
    let mut first_code_row_seen = false;
    let content_width = area.width.saturating_sub(2) as usize;
    let bullet_prefix = format!("{} ", ui::MODAL_INSTRUCTIONS_BULLET);
    let bullet_indent = " ".repeat(UnicodeWidthStr::width(bullet_prefix.as_str()));
    let shell_styles = ShellLineStyles::new();
    // Full syntax highlighting for the approval command block: command,
    // options, strings, variables, and separators keep distinct token
    // colors so the reviewable invocation stays scannable. Indented (not
    // `•`-bulleted) to read as one code block without the old `│` gutter.
    let code_gutter = "    ";

    for line in instructions {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            items.push(vec![Line::default()]);
            continue;
        }

        if let Some(header) = trimmed.strip_prefix("## ") {
            // Blank row before each new section gives header-led sections
            // clear visual separation without extra chrome.
            if first_content_rendered {
                items.push(vec![Line::default()]);
            }
            first_content_rendered = true;
            items.push(vec![Line::from(Span::styled(
                header.to_uppercase(),
                styles.header.add_modifier(Modifier::BOLD),
            ))]);
            continue;
        }

        if let Some(code) = trimmed.strip_prefix('`').and_then(|value| value.strip_suffix('`')) {
            let command = code.trim();
            first_content_rendered = true;
            let mut spans = vec![Span::styled(code_gutter.to_string(), styles.divider)];
            // The approval command block opens with a `$ ` shell marker on
            // its first row. Paint it dimmed and keep it out of the syntax
            // tokenizer, where a lone `$` would highlight as a variable.
            // Only the first code row is eligible, so a continuation line
            // that literally starts with `$ ` keeps its token colors.
            // Generic path: any modal's first `$ `-prefixed code fence gets
            // this treatment; today only the approval preview emits one.
            let (marker, body) = if !first_code_row_seen {
                first_code_row_seen = true;
                command.strip_prefix("$ ").map(|rest| ("$ ", rest)).unwrap_or(("", command))
            } else {
                ("", command)
            };
            if !marker.is_empty() {
                spans.push(Span::styled(marker.to_string(), styles.detail));
            }
            for segment in shell_syntax_segments(body, &shell_styles, true) {
                spans.push(Span::styled(segment.text, ratatui_style_from_inline(&segment.style, None)));
            }
            items.push(vec![Line::from(spans)]);
            continue;
        }

        let (display_text, is_highlighted) = parse_instruction_highlight_markup(trimmed);
        let wrapped = wrap_instruction_lines(&display_text, content_width);
        if wrapped.is_empty() {
            items.push(vec![Line::default()]);
            continue;
        }

        if let Some(diff_kind) = classify_diff_line(trimmed) {
            first_content_rendered = true;
            let style = diff_line_style(&diff_kind);
            items.push(vec![Line::from(vec![
                Span::styled(bullet_indent.clone(), Style::default()),
                Span::styled(display_text, style),
            ])]);
        } else if let Some((label, value)) = split_context_row(trimmed) {
            // `Reason:` / `Risk:` / `Source:` rows render as dim label +
            // body value with a hanging indent — no bullet — so metadata
            // reads as subordinate to the COMMAND code block above.
            // `Summary:` / `Plan:` use the same treatment so the plan-approval
            // header reads as an overview section instead of a bulleted list.
            first_content_rendered = true;
            let wrapped_value = wrap_instruction_lines(value, content_width.saturating_sub(2).max(1));
            let mut lines = Vec::new();
            for (index, segment) in wrapped_value.into_iter().enumerate() {
                if index == 0 {
                    lines.push(Line::from(vec![
                        Span::styled("  ".to_string(), Style::default()),
                        Span::styled(format!("{label}:"), styles.detail),
                        Span::raw(" ".to_string()),
                        Span::styled(segment, styles.instruction_body),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled("    ".to_string(), Style::default()),
                        Span::styled(segment, styles.instruction_body),
                    ]));
                }
            }
            if lines.is_empty() {
                lines.push(Line::default());
            }
            items.push(lines);
        } else if !first_content_rendered {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let style = if is_highlighted {
                    styles.highlight.add_modifier(Modifier::BOLD)
                } else if index == 0 {
                    styles.header
                } else {
                    styles.instruction_body
                };
                lines.push(Line::from(Span::styled(segment, style)));
            }
            items.push(lines);
            first_content_rendered = true;
        } else if is_plan_overflow_row(trimmed) {
            // `… and N more plan steps` is meta-evidence, not content: dim it
            // and skip the bullet so the header stays scannable.
            first_content_rendered = true;
            let mut lines = Vec::new();
            for segment in wrapped {
                lines.push(Line::from(vec![
                    Span::styled(bullet_indent.clone(), Style::default()),
                    Span::styled(segment, styles.hint),
                ]));
            }
            items.push(lines);
        } else if is_numbered_step_row(trimmed) {
            // Numbered plan steps (`1. …`) already carry a list marker.
            // Render them indented but bullet-free to avoid `• 1. …`.
            first_content_rendered = true;
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let body_style = if is_highlighted {
                    styles.highlight.add_modifier(Modifier::BOLD)
                } else {
                    styles.instruction_body
                };
                if index == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_indent.clone(), Style::default()),
                        Span::styled(segment, body_style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_indent.clone(), Style::default()),
                        Span::styled(format!("  {segment}"), body_style),
                    ]));
                }
            }
            items.push(lines);
        } else {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let body_style = if is_highlighted {
                    styles.highlight.add_modifier(Modifier::BOLD)
                } else {
                    styles.instruction_body
                };
                if index == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_prefix.clone(), styles.instruction_bullet),
                        Span::styled(segment, body_style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_indent.clone(), styles.instruction_bullet),
                        Span::styled(segment, body_style),
                    ]));
                }
            }
            items.push(lines);
        }
    }

    if items.is_empty() {
        items.push(vec![Line::default()]);
    }

    let mut rendered_lines = Vec::new();

    for lines in items {
        rendered_lines.extend(lines);
    }

    rendered_lines
}
