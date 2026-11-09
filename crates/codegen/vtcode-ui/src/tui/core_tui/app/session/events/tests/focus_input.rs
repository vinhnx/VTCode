use super::*;
use crate::tui::core_tui::app::types::{
    DiffHunk, DiffOverlayRequest, DiffPreviewMode, ListOverlayRequest, LocalAgentKind, LocalAgentStatus,
};
use crate::tui::core_tui::types::LocalAgentEntry;

fn focus_key() -> KeyEvent {
    KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT)
}

fn show_list_modal(session: &mut Session) {
    session.show_transient(TransientRequest::List(ListOverlayRequest {
        title: "Pick".to_string(),
        lines: Vec::new(),
        footer_hint: None,
        items: Vec::new(),
        selected: None,
        search: None,
        hotkeys: Vec::new(),
        status: None,
        plan_markdown: None,
    }));
}

fn open_drawer(session: &mut Session) {
    session.handle_command(InlineCommand::SetLocalAgents {
        entries: vec![LocalAgentEntry {
            program_status: vtcode_commons::program_status::ProgramState::Idle,
            updated_at: 0,
            id: "agent-1".to_string(),
            display_label: "rust-engineer".to_string(),
            agent_name: "rust-engineer".to_string(),
            color: Some("cyan".to_string()),
            kind: LocalAgentKind::Delegated,
            status: LocalAgentStatus::Running,
            summary: Some("Reviewing the workspace".to_string()),
            preview: "assistant: reviewing the workspace".to_string(),
            transcript_path: None,
        }],
    });
}

#[test]
fn focus_input_key_closes_viewer_and_modal() {
    let mut session = build_session();
    session.tool_output_blocks.push(ToolOutputBlock {
        lines: vec!["• Ran echo hello".to_string(), "  └ hello".to_string()],
        ..Default::default()
    });
    session.tool_output_revision = 1;
    let _ = session.process_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert!(session.tool_output_viewer_state().is_some());
    show_list_modal(&mut session);
    assert!(session.has_active_overlay());

    let event = session.process_key(focus_key());

    assert!(session.tool_output_viewer_state().is_none(), "viewer closed");
    assert!(!session.has_active_overlay(), "modal closed");
    assert!(event.is_some(), "modal cancel must emit, got {event:?}");
}

#[test]
fn focus_input_key_closes_agents_drawer() {
    let mut session = build_session();
    open_drawer(&mut session);
    assert!(session.local_agents_visible());

    let event = session.process_key(focus_key());

    assert!(event.is_none());
    assert!(!session.local_agents_visible(), "agents drawer closed");
}

#[test]
fn focus_input_leaves_diff_preview_decision_untouched() {
    let mut session = build_session();
    session.show_diff_overlay(DiffOverlayRequest {
        file_path: "src/main.rs".to_string(),
        before: "fn old() {}\n".to_string(),
        after: "fn new() {}\n".to_string(),
        hunks: vec![DiffHunk {
            old_start: 0,
            new_start: 0,
            old_lines: 1,
            new_lines: 1,
            display: "@@ -1 +1 @@".to_string(),
        }],
        current_hunk: 0,
        mode: DiffPreviewMode::EditApproval,
        unified: None,
    });
    assert!(session.diff_preview_state().is_some());

    let event = session.process_key(focus_key());

    assert!(event.is_none(), "no decision may be emitted, got {event:?}");
    assert!(session.diff_preview_state().is_some(), "edit approval stays open");
}

#[test]
fn focus_input_cancels_secure_prompt_like_escape() {
    let mut session = build_session_with_secure_prompt();
    for ch in ['s', 'k'] {
        let _ = session.process_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
    }
    assert_eq!(session.core.input_manager.content(), "sk");

    let event = session.process_key(focus_key());

    assert!(event.is_none(), "cancel is silent like Esc, got {event:?}");
    assert!(!session.has_active_overlay(), "secure prompt closed");
    assert!(session.core.input_manager.content().is_empty(), "secret scratch cleared");
}

#[test]
fn focus_input_unbound_ignores_chord() {
    let mut bindings = HashMap::new();
    bindings.insert("focus_input".to_string(), Vec::new());
    let mut session = Session::new_with_logs_and_bindings(
        InlineTheme::default(),
        None,
        24,
        true,
        None,
        Vec::new(),
        "Agent TUI".to_string(),
        BindingStore::new(bindings),
    );
    show_list_modal(&mut session);

    let event = session.process_key(focus_key());

    assert!(event.is_none());
    assert!(session.has_active_overlay(), "unbound chord must not dismiss");
}
