//! Transcript-review viewer and diff-preview key routing.

use super::*;

pub(crate) enum ToolOutputViewerKeyResult {
    NotHandled,
    Handled,
    Emit(InlineEvent),
}

pub(crate) fn handle_tool_output_viewer_key(
    session: &mut Session,
    key: &KeyEvent,
    has_control: bool,
    has_alt: bool,
    has_command: bool,
) -> ToolOutputViewerKeyResult {
    let toggle_shortcut = has_control
        && !has_alt
        && !has_command
        && session
            .core
            .resolve_rebindable_action(key)
            .is_some_and(|action| action == Action::OpenTranscriptReview);
    let compatibility_alias =
        has_alt && !has_control && !has_command && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O'));
    if session.tool_output_viewer_state().is_none() {
        if !toggle_shortcut && !compatibility_alias {
            return ToolOutputViewerKeyResult::NotHandled;
        }

        let width = session.core.transcript_width.max(1);
        let height = session.core.transcript_rows.max(1);
        session.open_tool_output_viewer(width, height, None);
        return ToolOutputViewerKeyResult::Handled;
    }

    let complete_copy_shortcut =
        has_control && !has_alt && !has_command && matches!(key.code, KeyCode::Char('o') | KeyCode::Char('O'));
    if complete_copy_shortcut {
        let text = session
            .tool_output_viewer_state_mut()
            .map(|viewer| viewer.export_text())
            .unwrap_or_default();
        session.core.copy_text_to_clipboard(&text);
        session.mark_dirty();
        return ToolOutputViewerKeyResult::Handled;
    }

    if toggle_shortcut {
        session.close_tool_output_viewer();
        return ToolOutputViewerKeyResult::Handled;
    }

    let viewer_copy_shortcut =
        matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Char('\u{3}')) && has_control;
    if viewer_copy_shortcut && session.core.mouse_selection.has_selection {
        session.core.mouse_selection.request_copy();
        session.mark_dirty();
        return ToolOutputViewerKeyResult::Handled;
    }

    let fallback_height = session.core.transcript_rows.max(1);
    let Some(viewer) = session.tool_output_viewer_state_mut() else {
        return ToolOutputViewerKeyResult::Handled;
    };
    let viewport_height = viewer.content_height_or(fallback_height);

    if viewer.search_active() {
        match key.code {
            KeyCode::Esc => {
                viewer.cancel_search();
                session.mark_dirty();
                return ToolOutputViewerKeyResult::Handled;
            }
            KeyCode::Enter => {
                viewer.commit_search(viewport_height);
                session.mark_dirty();
                return ToolOutputViewerKeyResult::Handled;
            }
            KeyCode::Backspace => {
                viewer.backspace_search();
                session.mark_dirty();
                return ToolOutputViewerKeyResult::Handled;
            }
            KeyCode::Char(ch) if !has_control && !has_alt && !has_command => {
                viewer.insert_search_text(&ch.to_string());
                session.mark_dirty();
                return ToolOutputViewerKeyResult::Handled;
            }
            _ => {
                return ToolOutputViewerKeyResult::Handled;
            }
        }
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
            session.close_tool_output_viewer();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('/') if !has_control && !has_alt && !has_command => {
            viewer.start_search();
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('n') if !has_control && !has_alt && !has_command => {
            viewer.jump_next_match(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('N') if !has_control && !has_alt && !has_command => {
            viewer.jump_previous_match(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Up | KeyCode::Char('k') if !has_control && !has_alt && !has_command => {
            viewer.scroll_line_up(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Down | KeyCode::Char('j') if !has_control && !has_alt && !has_command => {
            viewer.scroll_line_down(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::PageUp => {
            viewer.scroll_half_page_up(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::PageDown => {
            viewer.scroll_half_page_down(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('u') | KeyCode::Char('U') if has_control && !has_alt && !has_command => {
            viewer.scroll_half_page_up(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('d') | KeyCode::Char('D') if has_control && !has_alt && !has_command => {
            viewer.scroll_half_page_down(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('b') | KeyCode::Char('B') if !has_alt && !has_command => {
            viewer.scroll_full_page_up(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('f') | KeyCode::Char('F') if has_control && !has_alt && !has_command => {
            viewer.scroll_full_page_down(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char(' ') if !has_control && !has_alt && !has_command => {
            viewer.scroll_full_page_down(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Home | KeyCode::Char('g') if !has_control && !has_alt && !has_command => {
            viewer.scroll_to_top();
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::End | KeyCode::Char('G') if !has_control && !has_alt && !has_command => {
            viewer.scroll_to_bottom(viewport_height);
            session.mark_dirty();
            ToolOutputViewerKeyResult::Handled
        }
        KeyCode::Char('[') if !has_control && !has_alt && !has_command => {
            ToolOutputViewerKeyResult::Emit(InlineEvent::OpenToolOutputScrollback(viewer.export_text()))
        }
        KeyCode::Char('v') | KeyCode::Char('V') if !has_control && !has_alt && !has_command => {
            ToolOutputViewerKeyResult::Emit(InlineEvent::OpenToolOutputInEditor(viewer.export_text()))
        }
        _ => ToolOutputViewerKeyResult::Handled,
    }
}

pub(crate) enum DiffPreviewKeyResult {
    Emit(InlineEvent),
    Handled,
    NotHandled,
}

pub(crate) fn handle_diff_preview_key(session: &mut Session, key: &KeyEvent) -> DiffPreviewKeyResult {
    let Some(mode) = session.diff_preview_state().map(|state| state.mode) else {
        return DiffPreviewKeyResult::NotHandled;
    };

    match key.code {
        KeyCode::Tab => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            if diff_state.current_hunk + 1 < diff_state.hunk_count() {
                diff_state.focus_hunk(diff_state.current_hunk + 1);
            }
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::BackTab => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            if diff_state.current_hunk > 0 {
                diff_state.focus_hunk(diff_state.current_hunk - 1);
            }
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::Up => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.scroll_by(-1);
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::Down => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.scroll_by(1);
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::PageUp => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.scroll_by(-10);
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::PageDown => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.scroll_by(10);
            session.mark_dirty();
            DiffPreviewKeyResult::Handled
        }
        KeyCode::Enter => {
            session.close_diff_overlay();
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::Submitted(match mode {
                DiffPreviewMode::EditApproval => TransientSubmission::DiffApply,
                DiffPreviewMode::FileConflict => TransientSubmission::DiffProceed,
                DiffPreviewMode::ReadonlyReview => TransientSubmission::DiffAbort,
            })))
        }
        KeyCode::Char('r') | KeyCode::Char('R') if matches!(mode, DiffPreviewMode::FileConflict) => {
            session.close_diff_overlay();
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::Submitted(
                TransientSubmission::DiffReload,
            )))
        }
        KeyCode::Esc => {
            session.close_diff_overlay();
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::Submitted(match mode {
                DiffPreviewMode::EditApproval => TransientSubmission::DiffReject,
                DiffPreviewMode::FileConflict => TransientSubmission::DiffAbort,
                DiffPreviewMode::ReadonlyReview => TransientSubmission::DiffAbort,
            })))
        }
        KeyCode::Char('1') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.trust_mode = crate::tui::core_tui::app::types::TrustMode::Once;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('2') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.trust_mode = crate::tui::core_tui::app::types::TrustMode::Session;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('3') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.trust_mode = crate::tui::core_tui::app::types::TrustMode::Always;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        KeyCode::Char('4') if matches!(mode, DiffPreviewMode::EditApproval) => {
            let Some(diff_state) = session.diff_preview_state_mut() else {
                return DiffPreviewKeyResult::NotHandled;
            };
            diff_state.trust_mode = crate::tui::core_tui::app::types::TrustMode::AutoTrust;
            let mode = diff_state.trust_mode;
            session.mark_dirty();
            DiffPreviewKeyResult::Emit(InlineEvent::Transient(TransientEvent::SelectionChanged(
                TransientSelectionChange::DiffTrustMode { mode },
            )))
        }
        _ => DiffPreviewKeyResult::NotHandled,
    }
}
