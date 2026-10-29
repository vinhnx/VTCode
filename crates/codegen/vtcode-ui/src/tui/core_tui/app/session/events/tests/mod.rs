pub(super) use super::*;
pub(super) use crate::tui::core_tui::app::types::{
    CompactActivityMetadata, InlineCommand, LocalAgentsTransientRequest, ModalOverlayRequest, TransientActivitySignal,
    TransientRequest,
};
pub(super) use crate::tui::core_tui::session::action::BindingStore;
pub(super) use crate::tui::core_tui::types::{
    InlineCommand as CoreInlineCommand, InlineMessageKind, InlineSegment, InlineTextStyle, InlineTheme,
    SecurePromptConfig,
};
pub(super) use hashbrown::HashMap;
pub(super) use ratatui::Terminal;
pub(super) use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn build_session() -> Session {
    let mut session = Session::new(InlineTheme::default(), None, 24);
    session.core.set_fullscreen_active(true);
    session.core.apply_transcript_rows(8);
    session.core.apply_transcript_width(60);
    session
}

fn text_segment(text: impl Into<String>) -> InlineSegment {
    InlineSegment {
        text: text.into(),
        style: Arc::new(InlineTextStyle::default()),
    }
}

fn rendered_buffer_text(terminal: &Terminal<ratatui::backend::TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|row| {
            (0..buffer.area.width)
                .map(|column| buffer.cell((column, row)).expect("buffer cell").symbol())
                .collect::<Vec<_>>()
                .concat()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn add_compact_activity(session: &mut Session, id: u64, command: &str) {
    session.handle_command(InlineCommand::RecordToolOutput {
        id,
        lines: vec![format!("• Ran {command}"), "  └ complete output".to_string()],
    });
    session.handle_command(InlineCommand::AppendCompactActivity(CompactActivityMetadata {
        group_id: id,
        command_count: 1,
        command: Some(command.to_string().into()),
        hidden_line_count: 1,
        suffix: None,
        review_anchor: Some(id),
        review_anchors: vec![id],
    }));
}

fn build_session_with_secure_prompt() -> Session {
    let mut session = build_session();
    session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
        title: "Secure API key setup".to_string(),
        lines: vec!["Paste the key — it will be auto-detected and saved securely.".to_string()],
        secure_prompt: Some(SecurePromptConfig {
            label: "API key".to_string(),
            placeholder: None,
            mask_input: true,
        }),
        is_help_modal: false,
    }));
    assert!(session.has_active_overlay(), "secure prompt modal should be open");
    session
}

mod keybindings;
mod panels_viewer;
mod review_hints;
mod routing;
mod secure_tail;
