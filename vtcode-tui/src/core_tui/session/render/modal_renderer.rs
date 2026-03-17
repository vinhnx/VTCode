use super::*;
use super::spans::invalidate_scroll_metrics;
use ratatui::widgets::{Clear, Paragraph, Wrap};
use crate::ui::tui::session::modal::{
    ModalBodyContext, ModalListState, ModalRenderStyles, render_modal_body,
    render_wizard_modal_body,
};
use crate::ui::tui::types::InlineListSelection;

const MAX_INLINE_MODAL_HEIGHT: u16 = 20;
const MAX_INLINE_MODAL_HEIGHT_MULTILINE: u16 = 32;
const MAX_INLINE_INSTRUCTION_ROWS: usize = 6;

fn list_has_two_line_items(list: &ModalListState) -> bool {
    list.visible_indices.iter().any(|&index| {
        list.items.get(index).is_some_and(|item| {
            item.subtitle
                .as_ref()
                .is_some_and(|subtitle| !subtitle.trim().is_empty())
        })
    })
}

fn list_row_cap(list: &ModalListState) -> usize {
    if list_has_two_line_items(list) {
        ui::INLINE_LIST_MAX_ROWS_MULTILINE
    } else {
        ui::INLINE_LIST_MAX_ROWS
    }
}

fn list_desired_rows(list: &ModalListState) -> usize {
    list.visible_indices.len().clamp(1, list_row_cap(list))
}

fn modal_title_text(session: &Session) -> &str {
    session
        .wizard_overlay()
        .map(|wizard| wizard.title.as_str())
        .or_else(|| session.modal_state().map(|modal| modal.title.as_str()))
        .unwrap_or("")
}

fn modal_has_title(session: &Session) -> bool {
    !modal_title_text(session).trim().is_empty()
}

fn wizard_step_has_inline_custom_editor(
    wizard: &crate::ui::tui::session::modal::WizardModalState,
) -> bool {
    let Some(step) = wizard.steps.get(wizard.current_step) else {
        return false;
    };
    let Some(selected_visible) = step.list.list_state.selected() else {
        return false;
    };
    let Some(&item_index) = step.list.visible_indices.get(selected_visible) else {
        return false;
    };
    let Some(item) = step.list.items.get(item_index) else {
        return false;
    };
    matches!(
        item.selection.as_ref(),
        Some(InlineListSelection::RequestUserInputAnswer {
            selected,
            other,
            ..
        }) if selected.is_empty() && other.is_some()
    )
}

pub fn split_inline_modal_area(session: &Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (area, None);
    }

    let multiline_list_present = if let Some(wizard) = session.wizard_overlay() {
        wizard
            .steps
            .get(wizard.current_step)
            .is_some_and(|step| list_has_two_line_items(&step.list))
    } else if let Some(modal) = session.modal_state() {
        modal.list.as_ref().is_some_and(list_has_two_line_items)
    } else {
        false
    };

    let desired_lines = if let Some(wizard) = session.wizard_overlay() {
        let mut lines = 0usize;
        lines = lines.saturating_add(1); // tabs/header
        if wizard.search.is_some() {
            lines = lines.saturating_add(1);
        }
        lines = lines.saturating_add(2); // question and spacing
        let (list_rows, summary_rows) = wizard
            .steps
            .get(wizard.current_step)
            .map(|step| {
                (
                    list_desired_rows(&step.list),
                    step.list.summary_line_rows(None),
                )
            })
            .unwrap_or((1, 0));
        lines = lines.saturating_add(list_rows);
        lines = lines.saturating_add(summary_rows);
        if wizard
            .steps
            .get(wizard.current_step)
            .is_some_and(|step| step.notes_active || !step.notes.is_empty())
            && !wizard_step_has_inline_custom_editor(wizard)
        {
            lines = lines.saturating_add(1);
        }
        lines = lines.saturating_add(
            wizard
                .instruction_lines()
                .len()
                .min(MAX_INLINE_INSTRUCTION_ROWS),
        );
        if modal_has_title(session) {
            lines = lines.saturating_add(1); // title row
        }
        lines
    } else if let Some(modal) = session.modal_state() {
        let mut lines = modal.lines.len().clamp(1, MAX_INLINE_INSTRUCTION_ROWS);
        if modal.search.is_some() {
            lines = lines.saturating_add(1);
        }
        if modal.secure_prompt.is_some() {
            lines = lines.saturating_add(2);
        }
        if let Some(list) = modal.list.as_ref() {
            lines = lines.saturating_add(list_desired_rows(list));
            lines = lines.saturating_add(list.summary_line_rows(modal.footer_hint.as_deref()));
        } else {
            lines = lines.saturating_add(1);
        }
        if modal_has_title(session) {
            lines = lines.saturating_add(1); // title row
        }
        lines
    } else {
        return (area, None);
    };

    let max_panel_height = area.height.saturating_sub(1);
    if max_panel_height == 0 {
        return (area, None);
    }

    let min_height = ui::MODAL_MIN_HEIGHT.min(max_panel_height).max(1);
    let modal_height_cap = if multiline_list_present {
        MAX_INLINE_MODAL_HEIGHT_MULTILINE
    } else {
        MAX_INLINE_MODAL_HEIGHT
    };
    let capped_max = modal_height_cap.min(max_panel_height).max(min_height);
    let desired_height = (desired_lines.min(u16::MAX as usize) as u16)
        .max(min_height)
        .min(capped_max);

    let chunks =
        Layout::vertical([Constraint::Min(1), Constraint::Length(desired_height)]).split(area);
    (chunks[0], Some(chunks[1]))
}

pub fn render_modal(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.width == 0 || area.height == 0 {
        session.set_modal_list_area(None);
        return;
    }

    // Auto-approve modals when skip_confirmations is set (for tests and headless mode)
    if session.skip_confirmations
        && let Some(mut modal) = session.take_modal_state()
    {
        if let Some(list) = &mut modal.list
            && let Some(_selection) = list.current_selection()
        {
            // Note: We can't easily emit an event from here without access to the sender.
            // Instead, we just clear the modal and assume the tool execution logic
            // or whatever triggered the modal will check skip_confirmations as well.
            // This is handled in ensure_tool_permission.
        }
        session.input_enabled = modal.restore_input;
        session.cursor_visible = modal.restore_cursor;
        session.needs_full_clear = true;
        session.needs_redraw = true;
        session.set_modal_list_area(None);
        return;
    }

    let styles = modal_render_styles(session);
    let title = modal_title_text(session).trim().to_owned();
    let body_area = if title.is_empty() {
        area
    } else {
        let chunks = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(area);
        let title_area = chunks[0];
        frame.render_widget(Clear, title_area);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(title, styles.title))).wrap(Wrap { trim: true }),
            title_area,
        );
        chunks[1]
    };

    if let Some(wizard) = session.wizard_overlay_mut() {
        frame.render_widget(Clear, body_area);
        if body_area.width == 0 || body_area.height == 0 {
            session.set_modal_list_area(None);
            return;
        }
        let list_area = render_wizard_modal_body(frame, body_area, wizard, &styles);
        session.set_modal_list_area(list_area);
        return;
    }

    let input = session.input_manager.content().to_owned();
    let cursor = session.input_manager.cursor();
    let Some(modal) = session.modal_state_mut() else {
        session.set_modal_list_area(None);
        return;
    };

    frame.render_widget(Clear, body_area);
    if body_area.width == 0 || body_area.height == 0 {
        session.set_modal_list_area(None);
        return;
    }
    let list_area = render_modal_body(
        frame,
        body_area,
        ModalBodyContext {
            instructions: &modal.lines,
            footer_hint: modal.footer_hint.as_deref(),
            list: modal.list.as_mut(),
            styles: &styles,
            secure_prompt: modal.secure_prompt.as_ref(),
            search: modal.search.as_ref(),
            input: &input,
            cursor,
        },
    );
    session.set_modal_list_area(list_area);
}

pub(crate) fn modal_render_styles(session: &Session) -> ModalRenderStyles {
    ModalRenderStyles {
        border: border_style(session),
        highlight: modal_list_highlight_style(session),
        badge: border_style(session).add_modifier(Modifier::DIM | Modifier::BOLD),
        header: accent_style(session).add_modifier(Modifier::BOLD),
        selectable: default_style(session),
        detail: default_style(session).add_modifier(Modifier::DIM),
        search_match: accent_style(session).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        title: accent_style(session).add_modifier(Modifier::BOLD),
        divider: default_style(session).add_modifier(Modifier::DIM | Modifier::ITALIC),
        instruction_border: border_style(session),
        instruction_title: session.section_title_style(),
        instruction_bullet: accent_style(session).add_modifier(Modifier::BOLD),
        instruction_body: default_style(session),
        hint: default_style(session).add_modifier(Modifier::DIM | Modifier::ITALIC),
    }
}

#[allow(dead_code)]
pub(super) fn handle_tool_code_fence_marker(session: &mut Session, text: &str) -> bool {
    let trimmed = text.trim();
    let stripped = trimmed
        .strip_prefix("```")
        .or_else(|| trimmed.strip_prefix("~~~"));

    let Some(rest) = stripped else {
        return false;
    };

    if rest.contains("```") || rest.contains("~~~") {
        return false;
    }

    if session.in_tool_code_fence {
        session.in_tool_code_fence = false;
        remove_trailing_empty_tool_line(session);
    } else {
        session.in_tool_code_fence = true;
    }

    true
}

#[allow(dead_code)]
fn remove_trailing_empty_tool_line(session: &mut Session) {
    let should_remove = session
        .lines
        .last()
        .map(|line| line.kind == InlineMessageKind::Tool && line.segments.is_empty())
        .unwrap_or(false);
    if should_remove {
        session.lines.pop();
        invalidate_scroll_metrics(session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tui::InlineTheme;

    #[test]
    fn modal_title_text_uses_modal_title_and_empty_default() {
        let mut session = Session::new(InlineTheme::default(), None, 20);
        assert_eq!(modal_title_text(&session), "");

        session.show_modal("Config".to_owned(), vec![], None);
        assert_eq!(modal_title_text(&session), "Config");
    }

    #[test]
    fn modal_title_style_is_accent_and_bold() {
        let session = Session::new(InlineTheme::default(), None, 20);
        let styles = modal_render_styles(&session);

        assert_eq!(
            styles.title,
            accent_style(&session).add_modifier(Modifier::BOLD)
        );
    }
}
