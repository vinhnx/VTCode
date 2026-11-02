#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(super) use super::super::*;
pub(super) use super::helpers::*;
pub(super) use crate::tui::core_tui::session::{input, status_requires_shimmer};
pub(super) use std::time::Instant;

// ─── Helpers ────────────────────────────────────────────────────────────────

fn fresh_session() -> Session {
    let mut session = Session::new(InlineTheme::default(), None, VIEW_ROWS);
    session.appearance.hide_header = false;
    session
}

fn header_line_text(session: &mut Session) -> String {
    let lines = session.header_lines();
    assert_eq!(lines.len(), 1, "expected single-line header");
    lines[0].spans.iter().map(|span| span.content.clone().into_owned()).collect()
}

fn assert_header_contains_badge(session: &mut Session, badge_text: &str) {
    let text = header_line_text(session);
    assert!(text.contains(badge_text), "header should contain badge '{badge_text}', got: {text}");
}

fn setup_shimmer_session(session: &mut Session, left_status: &str) {
    session.handle_command(InlineCommand::SetInputStatus { left: Some(left_status.to_string()), right: None });
}

fn clear_shimmer_session(session: &mut Session) {
    session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });
}

fn input_data(session: &mut Session) -> input::InputWidgetData {
    session.build_input_widget_data(VIEW_WIDTH, 1)
}

fn session_with_highlights(highlights: Vec<InlineHeaderHighlight>) -> Session {
    let mut session = fresh_session();
    session.header_context.highlights = highlights;
    session.input_manager.set_content("notes".to_string());
    session.input_manager.set_cursor(session.input_manager.content().len());
    session
}

mod agent_status;
mod badges;
mod footer_copy_cursor;
mod header_blocks;
mod pills;
mod service_tier;
mod title_shell;
