use super::*;
use crate::tui::core_tui::app::types::{InlineListItem, ListOverlayRequest, ModalOverlayRequest};
use crate::tui::core_tui::app::{InlineListSelection, TransientEvent, TransientSubmission};

fn approval_item(title: &str, selection: InlineListSelection) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        selection: Some(selection),
        ..Default::default()
    }
}

fn show_approval_modal(session: &mut Session) {
    session.show_transient(TransientRequest::List(ListOverlayRequest {
        title: "Command approval".to_string(),
        lines: Vec::new(),
        footer_hint: None,
        items: vec![
            approval_item("Approve Once", InlineListSelection::ToolApproval(true)),
            approval_item("Deny", InlineListSelection::ToolApproval(false)),
        ],
        selected: Some(InlineListSelection::ToolApproval(true)),
        search: None,
        hotkeys: Vec::new(),
        status: None,
        plan_markdown: None,
    }));
}

#[test]
fn approval_modal_digit_then_enter_submits_approval() {
    let mut session = build_session();
    show_approval_modal(&mut session);
    assert!(session.has_active_overlay(), "approval modal must be open");

    let digit = session.process_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    let enter = session.process_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        matches!(
            enter,
            Some(InlineEvent::Transient(TransientEvent::Submitted(TransientSubmission::Selection(
                InlineListSelection::ToolApproval(true)
            ))))
        ),
        "digit+Enter must submit approval, got {enter:?} (digit={digit:?})"
    );
}

/// Replacing a stale active overlay must not drop overlays still waiting in
/// the queue; their callers are blocked on them just like the new prompt.
#[test]
fn blocking_overlay_preserves_queued_overlays() {
    let mut session = build_session();
    let modal = |title: &str| {
        TransientRequest::Modal(ModalOverlayRequest {
            title: title.to_string(),
            lines: Vec::new(),
            secure_prompt: None,
            is_help_modal: false,
        })
    };

    session.show_transient(modal("Active"));
    // A core-level request queues behind the active overlay. Its caller is
    // waiting on that prompt and must not be stranded by the replacement.
    session.core.show_overlay(crate::tui::core_tui::types::OverlayRequest::Modal(
        crate::tui::core_tui::types::ModalOverlayRequest {
            title: "Queued".to_string(),
            lines: Vec::new(),
            secure_prompt: None,
            is_help_modal: false,
        },
    ));
    session.show_transient(modal("Blocking"));

    let title = |session: &Session| session.modal_state().map(|modal| modal.title.clone()).unwrap_or_default();
    assert_eq!(title(&session), "Blocking");

    session.handle_command(InlineCommand::CloseTransient);
    assert_eq!(title(&session), "Queued", "the queued overlay must resurface");
}

/// A blocking HITL prompt must become the *visible* overlay. Queueing it
/// behind an already-active modal leaves the runtime waiting for input no
/// visible UI can produce, so the session hangs on "Waiting for approval"
/// while only the queue status is shown.
#[test]
fn approval_prompt_replaces_a_stale_overlay() {
    let mut session = build_session();
    session.show_transient(TransientRequest::Modal(ModalOverlayRequest {
        title: "Stale".to_string(),
        lines: Vec::new(),
        secure_prompt: None,
        is_help_modal: false,
    }));
    assert!(session.has_active_overlay(), "stale modal active");

    show_approval_modal(&mut session);

    let visible = session.modal_state().map(|modal| modal.title.clone()).unwrap_or_default();
    assert_eq!(
        visible, "Command approval",
        "permission overlay must be the visible modal, not a stale overlay ({visible})"
    );

    // The replaced overlay's selection must not anchor the prompt to its last
    // row: the explicit default ("Approve Once") must stay selected.
    let selection = session
        .modal_state()
        .and_then(|modal| modal.list.as_ref())
        .and_then(|list| list.current_selection());
    assert_eq!(selection, Some(InlineListSelection::ToolApproval(true)));
}
