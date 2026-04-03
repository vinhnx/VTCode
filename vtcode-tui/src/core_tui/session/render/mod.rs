use ratatui::prelude::*;
use std::borrow::Cow;

use super::{Session, message::MessageLine, message_renderer, text_utils};

mod modal_renderer;

pub(crate) use modal_renderer::floating_modal_area;
pub(crate) use modal_renderer::modal_render_styles;
pub use modal_renderer::{render_modal, split_inline_modal_area};

pub(super) fn render_message_spans(session: &Session, index: usize) -> Vec<Span<'static>> {
    let Some(line) = session.lines.get(index) else {
        return vec![Span::raw(String::new())];
    };
    session.render_message_spans_for_line(line)
}

pub(super) fn agent_prefix_spans(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    let prefix_style = |line: &MessageLine| session.prefix_style(line);
    message_renderer::agent_prefix_spans(line, &session.theme, &session.labels, &prefix_style)
}

pub(super) fn strip_ansi_codes(text: &str) -> Cow<'_, str> {
    text_utils::strip_ansi_codes(text)
}

pub(super) fn render_tool_segments(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    message_renderer::render_tool_segments(line, &session.theme)
}

#[allow(dead_code)]
pub fn render(session: &mut Session, frame: &mut Frame<'_>) {
    session.render(frame);
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.styles.modal_list_highlight_style()
}

pub fn apply_view_rows(session: &mut Session, rows: u16) {
    session.apply_view_rows(rows);
}

pub fn apply_transcript_rows(session: &mut Session, rows: u16) {
    session.apply_transcript_rows(rows);
}

pub fn apply_transcript_width(session: &mut Session, width: u16) {
    session.apply_transcript_width(width);
}

pub fn recalculate_transcript_rows(session: &mut Session) {
    session.recalculate_transcript_rows();
}
