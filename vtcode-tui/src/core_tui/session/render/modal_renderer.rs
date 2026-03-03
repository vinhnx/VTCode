use super::*;
use crate::ui::tui::session::modal::{
    ModalBodyContext, ModalListState, ModalRenderStyles, render_modal_body,
    render_wizard_modal_body,
};

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

pub fn split_inline_modal_area(session: &Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (area, None);
    }

    let multiline_list_present = if let Some(wizard) = session.wizard_modal.as_ref() {
        wizard
            .steps
            .get(wizard.current_step)
            .is_some_and(|step| list_has_two_line_items(&step.list))
    } else if let Some(modal) = session.modal.as_ref() {
        modal.list.as_ref().is_some_and(list_has_two_line_items)
    } else {
        false
    };

    let desired_lines = if let Some(wizard) = session.wizard_modal.as_ref() {
        let mut lines = 0usize;
        lines = lines.saturating_add(1); // tabs/header
        if wizard.search.is_some() {
            lines = lines.saturating_add(1);
        }
        lines = lines.saturating_add(2); // question and spacing
        let list_rows = wizard
            .steps
            .get(wizard.current_step)
            .map(|step| list_desired_rows(&step.list))
            .unwrap_or(1);
        lines = lines.saturating_add(list_rows);
        if wizard
            .steps
            .get(wizard.current_step)
            .is_some_and(|step| step.notes_active || !step.notes.is_empty())
        {
            lines = lines.saturating_add(1);
        }
        lines = lines.saturating_add(
            wizard
                .instruction_lines()
                .len()
                .min(MAX_INLINE_INSTRUCTION_ROWS),
        );
        lines
    } else if let Some(modal) = session.modal.as_ref() {
        let mut lines = modal.lines.len().clamp(1, MAX_INLINE_INSTRUCTION_ROWS);
        if modal.search.is_some() {
            lines = lines.saturating_add(1);
        }
        if modal.secure_prompt.is_some() {
            lines = lines.saturating_add(2);
        }
        if let Some(list) = modal.list.as_ref() {
            lines = lines.saturating_add(list_desired_rows(list));
        } else {
            lines = lines.saturating_add(1);
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
        return;
    }

    // Auto-approve modals when skip_confirmations is set (for tests and headless mode)
    if session.skip_confirmations
        && let Some(mut modal) = session.modal.take()
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
        return;
    }

    let styles = modal_render_styles(session);
    if let Some(wizard) = session.wizard_modal.as_mut() {
        frame.render_widget(Clear, area);
        if area.width == 0 || area.height == 0 {
            return;
        }
        render_wizard_modal_body(frame, area, wizard, &styles);
        return;
    }

    let Some(modal) = session.modal.as_mut() else {
        return;
    };

    frame.render_widget(Clear, area);
    if area.width == 0 || area.height == 0 {
        return;
    }
    render_modal_body(
        frame,
        area,
        ModalBodyContext {
            instructions: &modal.lines,
            footer_hint: modal.footer_hint.as_deref(),
            list: modal.list.as_mut(),
            styles: &styles,
            secure_prompt: modal.secure_prompt.as_ref(),
            search: modal.search.as_ref(),
            input: session.input_manager.content(),
            cursor: session.input_manager.cursor(),
        },
    );
}

fn modal_render_styles(session: &Session) -> ModalRenderStyles {
    ModalRenderStyles {
        border: border_style(session),
        highlight: modal_list_highlight_style(session),
        badge: border_style(session).add_modifier(Modifier::DIM | Modifier::BOLD),
        header: accent_style(session).add_modifier(Modifier::BOLD),
        selectable: default_style(session),
        detail: default_style(session).add_modifier(Modifier::DIM),
        search_match: accent_style(session).add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        title: Style::default().add_modifier(Modifier::BOLD),
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
