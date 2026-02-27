use super::*;

pub(super) fn render_message_spans(session: &Session, index: usize) -> Vec<Span<'static>> {
    let Some(line) = session.lines.get(index) else {
        return vec![Span::raw(String::new())];
    };
    let mut spans = Vec::new();
    if line.kind == InlineMessageKind::Agent {
        spans.extend(agent_prefix_spans(session, line));
    } else if let Some(prefix) = prefix_text(session, line.kind) {
        let style = prefix_style(session, line);
        spans.push(Span::styled(
            prefix,
            ratatui_style_from_inline(&style, session.theme.foreground),
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
        let tool_spans = render_tool_segments(session, line);
        if tool_spans.is_empty() {
            spans.push(Span::raw(String::new()));
        } else {
            spans.extend(tool_spans);
        }
        return spans;
    }

    if line.kind == InlineMessageKind::Pty {
        // Render PTY content directly without header decoration
        let fallback = text_fallback(session, line.kind).or(session.theme.foreground);
        for segment in &line.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            spans.push(Span::styled(segment.text.clone(), style));
        }
        if !spans.is_empty() {
            return spans;
        }
    }

    let fallback = text_fallback(session, line.kind).or(session.theme.foreground);
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, fallback);
        spans.push(Span::styled(segment.text.clone(), style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

pub(super) fn agent_prefix_spans(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let prefix_style_inline = prefix_style(session, line);
    let prefix_style_ratatui =
        ratatui_style_from_inline(&prefix_style_inline, session.theme.foreground);
    let has_label = session
        .labels
        .agent
        .as_ref()
        .is_some_and(|label| !label.is_empty());
    let prefix_has_trailing_space = ui::INLINE_AGENT_QUOTE_PREFIX
        .chars()
        .last()
        .is_some_and(|ch| ch.is_whitespace());
    if !ui::INLINE_AGENT_QUOTE_PREFIX.is_empty() {
        spans.push(Span::styled(
            ui::INLINE_AGENT_QUOTE_PREFIX.to_owned(),
            prefix_style_ratatui,
        ));
        if has_label && !prefix_has_trailing_space {
            spans.push(Span::styled(" ".to_owned(), prefix_style_ratatui));
        }
    }

    if let Some(label) = &session.labels.agent
        && !label.is_empty()
    {
        let label_style = ratatui_style_from_inline(&prefix_style_inline, session.theme.foreground);
        spans.push(Span::styled(label.clone(), label_style));
    }

    spans
}

/// Strips ANSI escape codes from text to ensure plain text output
pub(super) fn strip_ansi_codes(text: &str) -> std::borrow::Cow<'_, str> {
    text_utils::strip_ansi_codes(text)
}

pub(super) fn render_tool_segments(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    // Render tool output without header decorations - just display segments directly
    let mut spans = Vec::new();
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, session.theme.foreground);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    spans
}

/// Simplify tool call display text for better human readability
#[allow(dead_code)]
fn simplify_tool_display(text: &str) -> String {
    text_utils::simplify_tool_display(text)
}

#[allow(dead_code)]
fn tool_inline_style(session: &Session, tool_name: &str) -> InlineTextStyle {
    session.styles.tool_inline_style(tool_name)
}

pub(super) fn tool_border_style(session: &Session) -> InlineTextStyle {
    session.styles.tool_border_style()
}

pub(super) fn default_style(session: &Session) -> Style {
    session.styles.default_style()
}

#[allow(dead_code)]
fn accent_inline_style(session: &Session) -> InlineTextStyle {
    session.styles.accent_inline_style()
}

pub(super) fn accent_style(session: &Session) -> Style {
    session.styles.accent_style()
}

#[allow(dead_code)]
fn border_inline_style(session: &Session) -> InlineTextStyle {
    session.styles.border_inline_style()
}

pub(super) fn border_style(session: &Session) -> Style {
    session.styles.border_style()
}

fn prefix_text(session: &Session, kind: InlineMessageKind) -> Option<String> {
    match kind {
        InlineMessageKind::User => Some(
            session
                .labels
                .user
                .clone()
                .unwrap_or_else(|| USER_PREFIX.to_owned()),
        ),
        InlineMessageKind::Agent => None,
        InlineMessageKind::Policy => session.labels.agent.clone(),
        InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => None,
        InlineMessageKind::Info | InlineMessageKind::Warning => None,
    }
}

fn prefix_style(session: &Session, line: &MessageLine) -> InlineTextStyle {
    session.styles.prefix_style(line)
}

pub(super) fn text_fallback(session: &Session, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
    session.styles.text_fallback(kind)
}

fn viewport_height(session: &Session) -> usize {
    session.viewport_height()
}

pub(super) fn invalidate_scroll_metrics(session: &mut Session) {
    session.invalidate_scroll_metrics();
}
