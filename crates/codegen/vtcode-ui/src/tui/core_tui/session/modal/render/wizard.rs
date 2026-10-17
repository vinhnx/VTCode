//! Multi-step wizard modal rendering: tabs, per-step questions, and the wizard body.

use super::super::layout::ModalRenderStyles;
use super::super::state::{WizardModalState, WizardStepState};
use super::{
    ModalInlineEditor, ModalRenderOutcome, markdown_lines_for_modal, render_modal_list, render_modal_search,
    render_modal_text_lines,
};
use crate::tui::config::constants::ui;
use crate::tui::ui::tui::session::inline_list::selection_padding_width;
use crate::tui::ui::tui::session::wrapping;
use crate::tui::ui::tui::types::InlineListSelection;
use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Tabs, Wrap};
use ratatui_cheese::input::{Input, InputState, InputStyles};
use std::path::Path;

pub(crate) fn modal_text_area_aligned_with_list(area: Rect) -> Rect {
    let gutter = selection_padding_width().min(area.width as usize) as u16;
    if gutter == 0 || area.width <= gutter {
        area
    } else {
        Rect {
            x: area.x.saturating_add(gutter),
            width: area.width.saturating_sub(gutter),
            ..area
        }
    }
}
/// Render wizard tabs header showing steps with completion status
pub fn render_wizard_tabs(
    frame: &mut Frame<'_>,
    area: Rect,
    steps: &[WizardStepState],
    current_step: usize,
    styles: &ModalRenderStyles,
) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    if steps.len() <= 1 {
        let label = steps
            .first()
            .map(|step| {
                if step.completed {
                    format!("✔ {}", step.title)
                } else {
                    step.title.clone()
                }
            })
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(label, styles.highlight))).wrap(Wrap { trim: true }),
            area,
        );
        return;
    }

    let titles: Vec<Line<'static>> = steps
        .iter()
        .enumerate()
        .map(|(i, step)| {
            let icon = if step.completed { "✔" } else { "☐" };
            let text = format!("{} {}", icon, step.title);
            if i == current_step {
                Line::from(text).style(styles.highlight)
            } else if step.completed {
                Line::from(text).style(styles.selectable)
            } else {
                Line::from(text).style(styles.detail)
            }
        })
        .collect();

    let tabs = Tabs::new(titles)
        .select(Some(current_step))
        .divider(" │ ")
        .padding("", "")
        .highlight_style(styles.highlight);

    frame.render_widget(tabs, area);
}

pub(crate) fn inline_editor_for_step(step: &WizardStepState) -> Option<ModalInlineEditor> {
    let selected_visible = step.list.list_state.selected()?;
    let item_index = *step.list.visible_indices.get(selected_visible)?;
    let item = step.list.items.get(item_index)?;

    match item.selection.as_ref() {
        Some(InlineListSelection::RequestUserInputAnswer { selected, other, .. })
            if selected.is_empty() && other.is_some() =>
        {
            Some(ModalInlineEditor {
                item_index,
                label: step.freeform_label.clone().unwrap_or_else(|| "Custom note".to_string()),
                text: step.notes.clone(),
                placeholder: step.freeform_placeholder.clone(),
                active: step.notes_active,
            })
        }
        _ => None,
    }
}
/// Render wizard modal body including tabs, question, and list
pub(crate) fn render_wizard_modal_body(
    frame: &mut Frame<'_>,
    area: Rect,
    wizard: &mut WizardModalState,
    styles: &ModalRenderStyles,
    input_styles: &InputStyles,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    link_style: Style,
    hovered_link_style: Style,
) -> ModalRenderOutcome {
    let mut outcome = ModalRenderOutcome::default();
    if area.width == 0 || area.height == 0 {
        return outcome;
    }

    let is_multistep = wizard.mode == crate::tui::ui::tui::types::WizardModalMode::MultiStep;
    let text_alignment_fn: fn(Rect) -> Rect = if is_multistep {
        |rect| rect
    } else {
        modal_text_area_aligned_with_list
    };
    let content_width = text_alignment_fn(area).width.max(1) as usize;
    let current_step_state = wizard.steps.get(wizard.current_step);
    let inline_editor = current_step_state.and_then(inline_editor_for_step);
    let has_notes =
        current_step_state.is_some_and(|s| s.notes_active || !s.notes.is_empty()) && inline_editor.is_none();
    let instruction_lines = wizard.instruction_lines();
    let header_lines = if is_multistep {
        markdown_lines_for_modal(wizard.question_header().as_str(), styles.header)
    } else {
        Vec::new()
    };
    let question_lines = wizard
        .steps
        .get(wizard.current_step)
        .map(|step| markdown_lines_for_modal(step.question.as_str(), styles.header))
        .unwrap_or_else(|| vec![Line::default()]);

    let mut info_lines = question_lines;

    info_lines.extend(
        instruction_lines
            .into_iter()
            .map(|line| Line::from(Span::styled(line, styles.hint))),
    );
    let header_row_count = wrapping::wrap_lines_preserving_urls(header_lines.clone(), content_width)
        .len()
        .max(1);
    let info_row_count = wrapping::wrap_lines_preserving_urls(info_lines.clone(), content_width)
        .len()
        .max(1);

    // Layout: [Header] [Info] [Input?] [Search?] [Divider?] [List]
    let mut constraints = Vec::new();
    if is_multistep {
        constraints.push(Constraint::Length(header_row_count.min(u16::MAX as usize) as u16));
    } else {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(info_row_count.min(u16::MAX as usize) as u16));
    if has_notes {
        constraints.push(Constraint::Length(1));
    }
    if wizard.search.is_some() {
        constraints.push(Constraint::Length(1));
    }
    let show_list_divider = wizard.search.is_some();
    if show_list_divider {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Min(3));

    let chunks = Layout::vertical(constraints).split(area);

    let mut idx = 0;
    if is_multistep {
        let header_area = text_alignment_fn(chunks[idx]);
        render_modal_text_lines(
            frame,
            header_area,
            header_lines,
            workspace_root,
            last_mouse_position,
            link_style,
            hovered_link_style,
            &mut outcome,
        );
    } else {
        let tabs_area = text_alignment_fn(chunks[idx]);
        render_wizard_tabs(frame, tabs_area, &wizard.steps, wizard.current_step, styles);
        outcome.push_text_area(tabs_area);
    }
    idx += 1;

    render_modal_text_lines(
        frame,
        text_alignment_fn(chunks[idx]),
        info_lines,
        workspace_root,
        last_mouse_position,
        link_style,
        hovered_link_style,
        &mut outcome,
    );
    idx += 1;

    if has_notes
        && let Some(step) = wizard.steps.get(wizard.current_step)
        && idx < chunks.len()
    {
        let label_text = step.freeform_label.as_deref().unwrap_or("Custom note");
        let input_widget = Input::new(label_text)
            .placeholder(step.freeform_placeholder.as_deref().unwrap_or("Type here..."))
            .palette(&ratatui_cheese::theme::Palette::dark());
        let mut input_state = InputState::new();
        input_state.set_value(step.notes.clone());
        if step.notes_active {
            input_state.set_focused(true);
            // Position cursor at end
            for _ in 0..step.notes.chars().count() {
                input_state.move_right();
            }
        }
        let input_area = text_alignment_fn(chunks[idx]);
        frame.render_stateful_widget(&input_widget, input_area, &mut input_state);
        idx += 1;
    }

    if let Some(search) = wizard.search.as_ref()
        && idx < chunks.len()
    {
        render_modal_search(frame, text_alignment_fn(chunks[idx]), search, input_styles);
        idx += 1;
    }

    if show_list_divider && idx < chunks.len() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                ui::INLINE_BLOCK_HORIZONTAL.repeat(chunks[idx].width as usize),
                styles.border,
            )))
            .wrap(Wrap { trim: false }),
            chunks[idx],
        );
        idx += 1;
    }

    let wizard_numbers = wizard.numbered_shortcuts();
    if let Some(step) = wizard.steps.get_mut(wizard.current_step)
        && idx < chunks.len()
    {
        outcome.list_area = Some(render_modal_list(
            frame,
            chunks[idx],
            &mut step.list,
            styles,
            None,
            inline_editor.as_ref(),
            wizard_numbers,
            None,
        ));
    }

    outcome
}
