use super::*;

pub fn render_modal(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if viewport.width == 0 || viewport.height == 0 {
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
        let _is_multistep = wizard.mode == crate::ui::tui::types::WizardModalMode::MultiStep;
        let mut width_lines = Vec::new();
        width_lines.push(wizard.question_header());
        if let Some(step) = wizard.steps.get(wizard.current_step) {
            width_lines.push(step.question.clone());
        }
        if let Some(notes) = wizard.notes_line() {
            width_lines.push(notes);
        }
        width_lines.extend(wizard.instruction_lines());

        let list_state = wizard.steps.get(wizard.current_step).map(|step| &step.list);
        let width_hint =
            modal_content_width(&width_lines, list_state, None, wizard.search.as_ref());
        let text_lines = width_lines.len();
        let search_lines = wizard.search.as_ref().map(|_| 3).unwrap_or(0);
        let area = compute_modal_area(viewport, width_hint, text_lines, 0, search_lines, true);

        let block = Block::bordered()
            .title(Line::styled(wizard.title.clone(), styles.title))
            .border_type(terminal_capabilities::get_border_type())
            .border_style(styles.border);

        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        if area.width <= 2 || area.height <= 2 {
            return;
        }

        let inner = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(1),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(2),
        };

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        render_wizard_modal_body(frame, inner, wizard, &styles);
        return;
    }

    let Some(modal) = session.modal.as_mut() else {
        return;
    };

    let width_hint = modal_content_width(
        &modal.lines,
        modal.list.as_ref(),
        modal.secure_prompt.as_ref(),
        modal.search.as_ref(),
    );
    let prompt_lines = if modal.secure_prompt.is_some() { 2 } else { 0 };
    let search_lines = modal.search.as_ref().map(|_| 3).unwrap_or(0);
    let area = compute_modal_area(
        viewport,
        width_hint,
        modal.lines.len(),
        prompt_lines,
        search_lines,
        modal.list.is_some(),
    );

    let block = Block::bordered()
        .title(Line::styled(modal.title.clone(), styles.title))
        .border_type(terminal_capabilities::get_border_type())
        .border_style(styles.border);

    frame.render_widget(Clear, area);
    frame.render_widget(block, area);

    if area.width <= 2 || area.height <= 2 {
        return;
    }

    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    render_modal_body(
        frame,
        inner,
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
        badge: session.section_title_style().add_modifier(Modifier::DIM),
        header: session.section_title_style(),
        selectable: default_style(session).add_modifier(Modifier::BOLD),
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
