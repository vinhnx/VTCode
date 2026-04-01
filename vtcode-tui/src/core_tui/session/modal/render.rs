use crate::config::constants::ui;
use crate::ui::markdown::render_markdown;
use crate::ui::tui::session::inline_list::{
    InlineListRow, row_height, selection_padding, selection_padding_width,
};
use crate::ui::tui::session::list_panel::{
    SharedListPanelSections, SharedListPanelStyles, SharedListWidgetModel, SharedSearchField,
    render_shared_list_panel, render_shared_search_field,
};
use crate::ui::tui::types::{InlineListSelection, SecurePromptConfig};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Tabs, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::layout::{ModalBodyContext, ModalRenderStyles, ModalSection};
use super::state::{ModalListState, ModalSearchState, WizardModalState, WizardStepState};
use crate::core_tui::session::transcript_links::{
    TranscriptFileLinkTarget, decorate_detected_link_lines,
};
use crate::ui::tui::session::wrapping;
use ratatui::style::Color as RatatuiColor;
use std::mem;
use std::path::Path;

fn modal_text_area_aligned_with_list(area: Rect) -> Rect {
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

fn markdown_to_plain_lines(text: &str) -> Vec<String> {
    let mut lines = render_markdown(text)
        .into_iter()
        .map(|line| {
            line.segments
                .into_iter()
                .map(|segment| segment.text)
                .collect::<String>()
        })
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn wrap_line_to_width(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![line.to_owned()];
    }

    if line.is_empty() {
        return vec![String::new()];
    }

    let mut rows = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;

    for ch in line.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch)
            .unwrap_or(0)
            .max(1);
        if current_width + ch_width > width && !current.is_empty() {
            rows.push(mem::take(&mut current));
            current_width = 0;
            if ch.is_whitespace() {
                continue;
            }
        }

        current.push(ch);
        current_width += ch_width;
    }

    if !current.is_empty() {
        rows.push(current);
    }

    if rows.is_empty() {
        vec![String::new()]
    } else {
        rows
    }
}

fn markdown_lines_for_modal(text: &str, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in markdown_to_plain_lines(text) {
        lines.push(Line::from(Span::styled(line, style)));
    }

    if lines.is_empty() {
        vec![Line::default()]
    } else {
        lines
    }
}

#[cfg(test)]
fn render_markdown_lines_for_modal(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    wrapping::wrap_lines_preserving_urls(markdown_lines_for_modal(text, style), width)
}

#[derive(Clone, Debug)]
pub struct ModalInlineEditor {
    item_index: usize,
    label: String,
    text: String,
    placeholder: Option<String>,
    active: bool,
}

#[derive(Default)]
pub(crate) struct ModalRenderOutcome {
    pub(crate) list_area: Option<Rect>,
    pub(crate) text_areas: Vec<Rect>,
    pub(crate) link_targets: Vec<TranscriptFileLinkTarget>,
}

impl ModalRenderOutcome {
    fn push_text_area(&mut self, area: Rect) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        self.text_areas.push(area);
    }
}

fn render_modal_text_lines(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: Vec<Line<'static>>,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    link_style: Style,
    hovered_link_style: Style,
    outcome: &mut ModalRenderOutcome,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (decorated, targets) = decorate_detected_link_lines(
        lines,
        area,
        workspace_root,
        last_mouse_position,
        link_style,
        hovered_link_style,
    );
    frame.render_widget(Paragraph::new(decorated).wrap(Wrap { trim: false }), area);
    outcome.push_text_area(area);
    outcome.link_targets.extend(targets);
}

struct ModalListPanelModel<'a> {
    list: &'a mut ModalListState,
    styles: &'a ModalRenderStyles,
    inline_editor: Option<&'a ModalInlineEditor>,
}

impl SharedListWidgetModel for ModalListPanelModel<'_> {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.list.visible_indices.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(
                        ui::MODAL_LIST_NO_RESULTS_MESSAGE.to_owned(),
                        self.styles.detail,
                    )),
                    self.styles.detail,
                ),
                1_u16,
            )];
        }

        let selection_gutter = selection_padding_width() as u16;
        let content_width = width.saturating_sub(selection_gutter) as usize;
        self.list
            .visible_indices
            .iter()
            .enumerate()
            .map(|(visible_index, &item_index)| {
                let lines = modal_list_item_lines(
                    self.list,
                    visible_index,
                    item_index,
                    self.styles,
                    content_width,
                    self.inline_editor,
                );
                (
                    InlineListRow {
                        lines: lines.clone(),
                        style: self.styles.selectable,
                    },
                    row_height(&lines),
                )
            })
            .collect()
    }

    fn selected(&self) -> Option<usize> {
        self.list.list_state.selected()
    }

    fn set_selected(&mut self, selected: Option<usize>) {
        self.list.list_state.select(selected);
    }

    fn set_scroll_offset(&mut self, offset: usize) {
        *self.list.list_state.offset_mut() = offset;
    }

    fn set_viewport_rows(&mut self, rows: u16) {
        self.list.set_viewport_rows(rows);
        self.list.ensure_visible(rows);
    }
}

pub fn render_modal_list(
    frame: &mut Frame<'_>,
    area: Rect,
    list: &mut ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
    inline_editor: Option<&ModalInlineEditor>,
) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let summary = modal_list_summary_line(list, styles, footer_hint);
    let mut panel_model = ModalListPanelModel {
        list,
        styles,
        inline_editor,
    };
    let sections = SharedListPanelSections {
        header: Vec::new(),
        info: summary.into_iter().collect(),
        search: None,
    };
    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: styles.selectable,
            selected_style: Some(styles.highlight),
            text_style: styles.detail,
            divider_style: Some(styles.border),
        },
        &mut panel_model,
    );

    area
}

/// Render wizard tabs header showing steps with completion status
#[allow(dead_code)]
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
            Paragraph::new(Line::from(Span::styled(label, styles.highlight)))
                .wrap(Wrap { trim: true }),
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

fn inline_editor_for_step(step: &WizardStepState) -> Option<ModalInlineEditor> {
    let selected_visible = step.list.list_state.selected()?;
    let item_index = *step.list.visible_indices.get(selected_visible)?;
    let item = step.list.items.get(item_index)?;

    match item.selection.as_ref() {
        Some(InlineListSelection::RequestUserInputAnswer {
            selected, other, ..
        }) if selected.is_empty() && other.is_some() => Some(ModalInlineEditor {
            item_index,
            label: step
                .freeform_label
                .clone()
                .unwrap_or_else(|| "Custom note".to_string()),
            text: step.notes.clone(),
            placeholder: step.freeform_placeholder.clone(),
            active: step.notes_active,
        }),
        _ => None,
    }
}

/// Render wizard modal body including tabs, question, and list
#[allow(dead_code)]
pub(crate) fn render_wizard_modal_body(
    frame: &mut Frame<'_>,
    area: Rect,
    wizard: &mut WizardModalState,
    styles: &ModalRenderStyles,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    link_style: Style,
    hovered_link_style: Style,
) -> ModalRenderOutcome {
    let mut outcome = ModalRenderOutcome::default();
    if area.width == 0 || area.height == 0 {
        return outcome;
    }

    let is_multistep = wizard.mode == crate::ui::tui::types::WizardModalMode::MultiStep;
    let text_alignment_fn: fn(Rect) -> Rect = if is_multistep {
        |rect| rect
    } else {
        modal_text_area_aligned_with_list
    };
    let content_width = text_alignment_fn(area).width.max(1) as usize;
    let current_step_state = wizard.steps.get(wizard.current_step);
    let inline_editor = current_step_state.and_then(inline_editor_for_step);
    let has_notes = current_step_state.is_some_and(|s| s.notes_active || !s.notes.is_empty())
        && inline_editor.is_none();
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
    if let Some(step) = wizard.steps.get(wizard.current_step)
        && has_notes
    {
        let label_text = step.freeform_label.as_deref().unwrap_or("›");
        let mut spans = vec![Span::styled(format!("{} ", label_text), styles.header)];

        if step.notes.is_empty() {
            if let Some(placeholder) = step.freeform_placeholder.as_ref() {
                spans.push(Span::styled(placeholder.clone(), styles.detail));
            }
        } else {
            spans.push(Span::styled(step.notes.clone(), styles.selectable));
        }

        if step.notes_active {
            spans.push(Span::styled("▌", styles.highlight));
        }
        info_lines.push(Line::from(spans));
    }

    info_lines.extend(
        instruction_lines
            .into_iter()
            .map(|line| Line::from(Span::styled(line, styles.hint))),
    );
    let header_row_count =
        wrapping::wrap_lines_preserving_urls(header_lines.clone(), content_width)
            .len()
            .max(1);
    let info_row_count = wrapping::wrap_lines_preserving_urls(info_lines.clone(), content_width)
        .len()
        .max(1);

    // Layout: [Header] [Info] [Search?] [Main content list]
    let mut constraints = Vec::new();
    if is_multistep {
        constraints.push(Constraint::Length(
            header_row_count.min(u16::MAX as usize) as u16
        ));
    } else {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(
        info_row_count.min(u16::MAX as usize) as u16
    ));
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

    if let Some(search) = wizard.search.as_ref()
        && idx < chunks.len()
    {
        render_modal_search(frame, text_alignment_fn(chunks[idx]), search, styles);
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
        ));
    }

    outcome
}

#[allow(clippy::const_is_empty)]
fn modal_list_summary_line(
    list: &ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
) -> Option<Line<'static>> {
    if !list.filter_active() {
        let message = list.non_filter_summary_text(footer_hint)?;
        return Some(Line::from(Span::styled(message, styles.hint)));
    }

    let mut spans = Vec::new();
    let matches = list.visible_selectable_count();
    let total = list.total_selectable();
    if matches == 0 {
        spans.push(Span::styled(
            ui::MODAL_LIST_SUMMARY_NO_MATCHES.to_owned(),
            styles.search_match,
        ));
        if !ui::MODAL_LIST_SUMMARY_RESET_HINT.is_empty() {
            spans.push(Span::styled(
                format!(
                    "{}{}",
                    ui::MODAL_LIST_SUMMARY_SEPARATOR,
                    ui::MODAL_LIST_SUMMARY_RESET_HINT
                ),
                styles.hint,
            ));
        }
    } else {
        spans.push(Span::styled(
            format!(
                "{} {} {} {}",
                ui::MODAL_LIST_SUMMARY_MATCHES_LABEL,
                matches,
                ui::MODAL_LIST_SUMMARY_TOTAL_LABEL,
                total
            ),
            styles.detail,
        ));
    }

    if spans.is_empty() {
        None
    } else {
        Some(Line::from(spans))
    }
}

pub(crate) fn render_modal_body(
    frame: &mut Frame<'_>,
    area: Rect,
    context: ModalBodyContext<'_, '_>,
    workspace_root: Option<&Path>,
    last_mouse_position: Option<(u16, u16)>,
    link_style: Style,
    hovered_link_style: Style,
) -> ModalRenderOutcome {
    let mut outcome = ModalRenderOutcome::default();
    if area.width == 0 || area.height == 0 {
        return outcome;
    }

    let mut sections = Vec::new();
    let has_instructions = context
        .instructions
        .iter()
        .any(|line| !line.trim().is_empty());
    let instruction_lines = if has_instructions {
        modal_instruction_lines(area, context.instructions, context.styles)
    } else {
        Vec::new()
    };
    let instruction_row_count =
        wrapping::wrap_lines_preserving_urls(instruction_lines.clone(), area.width.max(1) as usize)
            .len()
            .clamp(1, 6);
    if has_instructions {
        sections.push(ModalSection::Instructions);
    }
    if context.secure_prompt.is_some() {
        sections.push(ModalSection::Prompt);
    }
    if context.search.is_some() {
        sections.push(ModalSection::Search);
    }
    if context.list.is_some() {
        sections.push(ModalSection::List);
    }

    if sections.is_empty() {
        return outcome;
    }

    let mut constraints = Vec::new();
    for section in &sections {
        match section {
            ModalSection::Search => constraints.push(Constraint::Length(1.min(area.height))),
            ModalSection::Instructions => {
                let visible_rows = instruction_row_count as u16;
                constraints.push(Constraint::Length(visible_rows.min(area.height)));
            }
            ModalSection::Prompt => constraints.push(Constraint::Length(2.min(area.height))),
            ModalSection::List => constraints.push(Constraint::Min(1)),
        }
    }
    let show_list_divider = context.list.is_some() && context.search.is_some();
    if show_list_divider {
        let insert_at = constraints.len().saturating_sub(1);
        constraints.insert(insert_at, Constraint::Length(1));
    }

    let chunks = Layout::vertical(constraints).split(area);
    let mut list_state = context.list;

    let mut chunk_idx = 0usize;
    for section in sections {
        let chunk = chunks[chunk_idx];
        match section {
            ModalSection::Instructions => {
                if chunk.height > 0 && !instruction_lines.is_empty() {
                    render_modal_text_lines(
                        frame,
                        chunk,
                        instruction_lines.clone(),
                        workspace_root,
                        last_mouse_position,
                        link_style,
                        hovered_link_style,
                        &mut outcome,
                    );
                }
            }
            ModalSection::Prompt => {
                if let Some(config) = context.secure_prompt {
                    render_secure_prompt(frame, chunk, config, context.input, context.cursor);
                }
            }
            ModalSection::Search => {
                if let Some(config) = context.search {
                    render_modal_search(frame, chunk, config, context.styles);
                }
            }
            ModalSection::List => {
                if show_list_divider && chunk_idx > 0 {
                    let divider_chunk = chunks[chunk_idx];
                    frame.render_widget(
                        Paragraph::new(Line::from(Span::styled(
                            ui::INLINE_BLOCK_HORIZONTAL.repeat(divider_chunk.width as usize),
                            context.styles.border,
                        )))
                        .wrap(Wrap { trim: false }),
                        divider_chunk,
                    );
                    chunk_idx += 1;
                }
                if let Some(list_state) = list_state.as_deref_mut() {
                    outcome.list_area = Some(render_modal_list(
                        frame,
                        chunks[chunk_idx],
                        list_state,
                        context.styles,
                        context.footer_hint,
                        None,
                    ));
                }
            }
        }
        chunk_idx += 1;
    }

    outcome
}

enum DiffLineKind {
    Addition,
    Deletion,
    HunkHeader,
}

fn classify_diff_line(line: &str) -> Option<DiffLineKind> {
    let trimmed = line.trim();
    if trimmed.starts_with("@@ ") {
        Some(DiffLineKind::HunkHeader)
    } else if trimmed.starts_with('+') {
        Some(DiffLineKind::Addition)
    } else if trimmed.starts_with('-') {
        Some(DiffLineKind::Deletion)
    } else {
        None
    }
}

fn diff_line_style(kind: &DiffLineKind) -> Style {
    match kind {
        DiffLineKind::Addition => Style::default().fg(RatatuiColor::LightGreen),
        DiffLineKind::Deletion => Style::default().fg(RatatuiColor::LightRed),
        DiffLineKind::HunkHeader => Style::default().add_modifier(Modifier::DIM),
    }
}

fn modal_instruction_lines(
    area: Rect,
    instructions: &[String],
    styles: &ModalRenderStyles,
) -> Vec<Line<'static>> {
    fn parse_instruction_highlight_markup(text: &str) -> (String, bool) {
        let trimmed = text.trim();
        match trimmed
            .strip_prefix("**")
            .and_then(|value| value.strip_suffix("**"))
            .map(str::trim)
        {
            Some(value) if !value.is_empty() => (value.to_string(), true),
            _ => (trimmed.to_string(), false),
        }
    }

    fn wrap_instruction_lines(text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![text.to_owned()];
        }

        let mut lines = Vec::new();
        let mut current = String::new();

        for word in text.split_whitespace() {
            let word_width = UnicodeWidthStr::width(word);
            if current.is_empty() {
                current.push_str(word);
                continue;
            }

            let current_width = UnicodeWidthStr::width(current.as_str());
            let candidate_width = current_width.saturating_add(1).saturating_add(word_width);
            if candidate_width > width {
                lines.push(current);
                current = word.to_owned();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        if lines.is_empty() {
            vec![text.to_owned()]
        } else {
            lines
        }
    }

    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }

    let mut items: Vec<Vec<Line<'static>>> = Vec::new();
    let mut first_content_rendered = false;
    let content_width = area.width.saturating_sub(2) as usize;
    let bullet_prefix = format!("{} ", ui::MODAL_INSTRUCTIONS_BULLET);
    let bullet_indent = " ".repeat(UnicodeWidthStr::width(bullet_prefix.as_str()));

    for line in instructions {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            items.push(vec![Line::default()]);
            continue;
        }

        let (display_text, is_highlighted) = parse_instruction_highlight_markup(trimmed);
        let wrapped = wrap_instruction_lines(&display_text, content_width);
        if wrapped.is_empty() {
            items.push(vec![Line::default()]);
            continue;
        }

        if let Some(diff_kind) = classify_diff_line(trimmed) {
            first_content_rendered = true;
            let style = diff_line_style(&diff_kind);
            items.push(vec![Line::from(vec![
                Span::styled(bullet_indent.clone(), Style::default()),
                Span::styled(display_text, style),
            ])]);
        } else if !first_content_rendered {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let style = if is_highlighted {
                    styles.highlight.add_modifier(Modifier::BOLD)
                } else if index == 0 {
                    styles.header
                } else {
                    styles.instruction_body
                };
                lines.push(Line::from(Span::styled(segment, style)));
            }
            items.push(lines);
            first_content_rendered = true;
        } else {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let body_style = if is_highlighted {
                    styles.highlight.add_modifier(Modifier::BOLD)
                } else {
                    styles.instruction_body
                };
                if index == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_prefix.clone(), styles.instruction_bullet),
                        Span::styled(segment, body_style),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_indent.clone(), styles.instruction_bullet),
                        Span::styled(segment, body_style),
                    ]));
                }
            }
            items.push(lines);
        }
    }

    if items.is_empty() {
        items.push(vec![Line::default()]);
    }

    let mut rendered_lines = Vec::new();
    if !ui::MODAL_INSTRUCTIONS_TITLE.is_empty() {
        rendered_lines.push(Line::from(Span::styled(
            ui::MODAL_INSTRUCTIONS_TITLE.to_owned(),
            styles.instruction_title,
        )));
    }

    for lines in items {
        rendered_lines.extend(lines);
    }

    rendered_lines
}

fn render_modal_search(
    frame: &mut Frame<'_>,
    area: Rect,
    search: &ModalSearchState,
    styles: &ModalRenderStyles,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let search = SharedSearchField {
        label: search.label.clone(),
        placeholder: search.placeholder.clone(),
        query: search.query.clone(),
    };
    render_shared_search_field(
        frame,
        area,
        &search,
        styles.header,
        styles.selectable,
        styles.detail,
        styles.highlight,
    );
}

fn render_secure_prompt(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &SecurePromptConfig,
    input: &str,
    _cursor: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let display_text = if input.is_empty() {
        config.placeholder.clone().unwrap_or_default()
    } else if config.mask_input {
        let grapheme_count = input.chars().count();
        std::iter::repeat_n('•', grapheme_count).collect()
    } else {
        input.to_owned()
    };

    // Render label
    let label_paragraph = Paragraph::new(config.label.clone());
    let label_area = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1.min(area.height),
    };
    frame.render_widget(label_paragraph, label_area);

    // Render input field
    if area.height > 1 {
        let input_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: (area.height - 1).max(1),
        };

        let input_paragraph = Paragraph::new(display_text);
        frame.render_widget(input_paragraph, input_area);
    }
}

pub(super) fn highlight_segments(
    text: &str,
    normal_style: Style,
    highlight_style: Style,
    terms: &[String],
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::styled(String::new(), normal_style)];
    }

    if terms.is_empty() {
        return vec![Span::styled(text.to_owned(), normal_style)];
    }

    let lower = text.to_ascii_lowercase();
    let mut char_offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    char_offsets.push(text.len());
    let char_count = char_offsets.len().saturating_sub(1);
    if char_count == 0 {
        return vec![Span::styled(text.to_owned(), normal_style)];
    }

    let mut highlight_flags = vec![false; char_count];
    for term in terms {
        let needle = term.as_str();
        if needle.is_empty() {
            continue;
        }

        let mut search_start = 0usize;
        while search_start < lower.len() {
            let Some(pos) = lower[search_start..].find(needle) else {
                break;
            };
            let byte_start = search_start + pos;
            let byte_end = byte_start + needle.len();
            let start_index = char_offsets.partition_point(|offset| *offset < byte_start);
            let end_index = char_offsets.partition_point(|offset| *offset < byte_end);
            for flag in highlight_flags
                .iter_mut()
                .take(end_index.min(char_count))
                .skip(start_index)
            {
                *flag = true;
            }
            search_start = byte_end;
        }
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut current_highlight = highlight_flags.first().copied().unwrap_or(false);
    for (idx, ch) in text.chars().enumerate() {
        let highlight = highlight_flags.get(idx).copied().unwrap_or(false);
        if idx == 0 {
            current_highlight = highlight;
        } else if highlight != current_highlight {
            let style = if current_highlight {
                highlight_style
            } else {
                normal_style
            };
            segments.push(Span::styled(mem::take(&mut current), style));
            current_highlight = highlight;
        }
        current.push(ch);
    }

    if !current.is_empty() {
        let style = if current_highlight {
            highlight_style
        } else {
            normal_style
        };
        segments.push(Span::styled(current, style));
    }

    if segments.is_empty() {
        segments.push(Span::styled(String::new(), normal_style));
    }

    segments
}

pub fn modal_list_item_lines(
    list: &ModalListState,
    _visible_index: usize,
    item_index: usize,
    styles: &ModalRenderStyles,
    content_width: usize,
    inline_editor: Option<&ModalInlineEditor>,
) -> Vec<Line<'static>> {
    let item = match list.items.get(item_index) {
        Some(i) => i,
        None => {
            tracing::warn!("modal list item index {item_index} out of bounds");
            return vec![Line::default()];
        }
    };
    if item.is_divider {
        let divider = if item.title.is_empty() {
            ui::INLINE_BLOCK_HORIZONTAL.repeat(8)
        } else {
            item.title.clone()
        };
        return vec![Line::from(Span::styled(divider, styles.divider))];
    }

    let indent = "  ".repeat(item.indent as usize);
    let selection_padding = selection_padding();

    let mut primary_spans = Vec::new();
    if !selection_padding.is_empty() {
        primary_spans.push(Span::raw(selection_padding.clone()));
    }

    if !indent.is_empty() {
        primary_spans.push(Span::raw(indent.clone()));
    }

    if let Some(badge) = &item.badge {
        let badge_label = format!("[{}]", badge);
        primary_spans.push(Span::styled(
            badge_label,
            modal_badge_style(badge.as_str(), styles),
        ));
        primary_spans.push(Span::raw(" "));
    }

    let title_style = if item.selection.is_some() {
        styles.selectable
    } else if item.is_header() {
        styles.header
    } else {
        styles.detail
    };

    let title_spans = highlight_segments(
        item.title.as_str(),
        title_style,
        styles.search_match,
        list.highlight_terms(),
    );
    primary_spans.extend(title_spans);

    let mut lines = vec![Line::from(primary_spans)];

    if let Some(subtitle) = &item.subtitle {
        let indent_width = item.indent as usize * 2;
        let wrapped_width = content_width.saturating_sub(indent_width).max(1);
        let wrapped_lines = wrap_line_to_width(subtitle.as_str(), wrapped_width);

        for wrapped in wrapped_lines {
            let mut secondary_spans = Vec::new();
            if !selection_padding.is_empty() {
                secondary_spans.push(Span::raw(selection_padding.clone()));
            }
            if !indent.is_empty() {
                secondary_spans.push(Span::raw(indent.clone()));
            }
            let subtitle_spans = highlight_segments(
                wrapped.as_str(),
                styles.detail,
                styles.search_match,
                list.highlight_terms(),
            );
            secondary_spans.extend(subtitle_spans);
            lines.push(Line::from(secondary_spans));
        }
    }

    if let Some(editor) = inline_editor
        && editor.item_index == item_index
    {
        let mut editor_spans = Vec::new();
        if !selection_padding.is_empty() {
            editor_spans.push(Span::raw(selection_padding.clone()));
        }
        if !indent.is_empty() {
            editor_spans.push(Span::raw(indent.clone()));
        }

        editor_spans.push(Span::styled(format!("{} ", editor.label), styles.header));
        if editor.text.is_empty() {
            if let Some(placeholder) = editor.placeholder.as_ref() {
                editor_spans.push(Span::styled(placeholder.clone(), styles.detail));
            }
        } else {
            editor_spans.push(Span::styled(editor.text.clone(), styles.selectable));
        }

        if editor.active {
            editor_spans.push(Span::styled("▌", styles.highlight));
        }

        lines.push(Line::from(editor_spans));
    }

    if !list.compact_rows() && item.selection.is_some() {
        lines.push(Line::default());
    }
    lines
}

fn modal_badge_style(badge: &str, styles: &ModalRenderStyles) -> Style {
    match badge {
        "Active" | "Action" => styles.header.add_modifier(Modifier::BOLD),
        "Read-only" => styles.detail.add_modifier(Modifier::ITALIC),
        _ => styles.badge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tui::InlineListItem;
    use ratatui::{Terminal, backend::TestBackend};

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect::<String>()
    }

    fn modal_render_styles() -> ModalRenderStyles {
        ModalRenderStyles {
            border: Style::default(),
            highlight: Style::default(),
            badge: Style::default(),
            header: Style::default(),
            selectable: Style::default(),
            detail: Style::default(),
            search_match: Style::default(),
            title: Style::default(),
            divider: Style::default(),
            instruction_border: Style::default(),
            instruction_title: Style::default(),
            instruction_bullet: Style::default(),
            instruction_body: Style::default(),
            hint: Style::default(),
        }
    }

    fn render_modal_lines(search: ModalSearchState) -> Vec<String> {
        let styles = modal_render_styles();
        let mut list = ModalListState::new(
            vec![InlineListItem {
                title: "Alpha".to_string(),
                subtitle: Some("First item".to_string()),
                badge: Some("OpenAI".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::Model(0)),
                search_value: Some("alpha".to_string()),
            }],
            None,
        );
        let instructions = vec!["Choose a model".to_string()];
        let backend = TestBackend::new(80, 8);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                render_modal_body(
                    frame,
                    Rect::new(0, 0, 80, 8),
                    ModalBodyContext {
                        instructions: &instructions,
                        footer_hint: None,
                        list: Some(&mut list),
                        styles: &styles,
                        secure_prompt: None,
                        search: Some(&search),
                        input: "",
                        cursor: 0,
                    },
                    None,
                    None,
                    Style::default(),
                    Style::default(),
                );
            })
            .expect("modal render should succeed");

        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn render_markdown_lines_for_modal_wraps_long_questions() {
        let lines = render_markdown_lines_for_modal(
            "What user-visible outcome should this change deliver, and what constraints or non-goals must remain unchanged?",
            40,
            Style::default(),
        );

        assert!(lines.len() > 1, "long question should wrap across lines");
        for line in &lines {
            let text = line_text(line);
            assert!(
                UnicodeWidthStr::width(text.as_str()) <= 40,
                "line exceeded modal width: {text}"
            );
        }
    }

    #[test]
    fn render_markdown_lines_for_modal_renders_markdown_headings() {
        let lines =
            render_markdown_lines_for_modal("### Goal\n- Reduce prompt size", 80, Style::default());

        let rendered = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(rendered.contains("Goal"));
        assert!(!rendered.contains("### Goal"));
        assert!(rendered.contains("Reduce prompt size"));
    }

    #[test]
    fn config_list_summary_uses_navigation_hint_instead_of_density() {
        let list = ModalListState::new(
            vec![InlineListItem {
                title: "Permission mode".to_string(),
                subtitle: Some("permissions.default_mode = auto".to_string()),
                badge: Some("Toggle".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::ConfigAction(
                    "permissions.default_mode:cycle".to_string(),
                )),
                search_value: None,
            }],
            None,
        );

        let styles = ModalRenderStyles {
            border: Style::default(),
            highlight: Style::default(),
            badge: Style::default(),
            header: Style::default(),
            selectable: Style::default(),
            detail: Style::default(),
            search_match: Style::default(),
            title: Style::default(),
            divider: Style::default(),
            instruction_border: Style::default(),
            instruction_title: Style::default(),
            instruction_bullet: Style::default(),
            instruction_body: Style::default(),
            hint: Style::default(),
        };

        let summary = modal_list_summary_line(&list, &styles, None)
            .expect("expected summary line for config list");
        let text = line_text(&summary);
        assert!(text.contains("Navigation:"));
        assert!(!text.contains("Alt+D"));
        assert!(!text.contains("Density:"));
    }

    #[test]
    fn non_config_list_summary_omits_density_hint() {
        let list = ModalListState::new(
            vec![InlineListItem {
                title: "gpt-5".to_string(),
                subtitle: Some("General reasoning".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::Model(0)),
                search_value: Some("gpt-5".to_string()),
            }],
            None,
        );

        let styles = ModalRenderStyles {
            border: Style::default(),
            highlight: Style::default(),
            badge: Style::default(),
            header: Style::default(),
            selectable: Style::default(),
            detail: Style::default(),
            search_match: Style::default(),
            title: Style::default(),
            divider: Style::default(),
            instruction_border: Style::default(),
            instruction_title: Style::default(),
            instruction_bullet: Style::default(),
            instruction_body: Style::default(),
            hint: Style::default(),
        };

        let summary = modal_list_summary_line(&list, &styles, None);
        assert!(summary.is_none(), "density summary should be hidden");
    }

    #[test]
    fn modal_text_area_alignment_reserves_selection_gutter() {
        let area = Rect::new(10, 3, 20, 4);
        let aligned = modal_text_area_aligned_with_list(area);
        let gutter = selection_padding_width() as u16;

        assert_eq!(aligned.x, area.x + gutter);
        assert_eq!(aligned.width, area.width - gutter);
        assert_eq!(aligned.y, area.y);
        assert_eq!(aligned.height, area.height);
    }

    #[test]
    fn modal_text_area_alignment_keeps_narrow_areas_unchanged() {
        let gutter = selection_padding_width() as u16;
        let area = Rect::new(2, 1, gutter, 2);
        let aligned = modal_text_area_aligned_with_list(area);
        assert_eq!(aligned, area);
    }

    #[test]
    fn modal_search_field_renders_placeholder_inside_brackets() {
        let lines = render_modal_lines(ModalSearchState {
            label: "Search models".to_string(),
            placeholder: Some("provider, name, id".to_string()),
            query: String::new(),
        });

        let search_line = lines
            .iter()
            .find(|line| line.contains("Search models:"))
            .expect("search line should render");
        assert!(search_line.contains("[provider, name, id"));
        assert!(search_line.contains("]"));
    }

    #[test]
    fn modal_search_field_renders_query_above_list() {
        let lines = render_modal_lines(ModalSearchState {
            label: "Search models".to_string(),
            placeholder: Some("provider, name, id".to_string()),
            query: "openrouter".to_string(),
        });

        let search_index = lines
            .iter()
            .position(|line| line.contains("Search models: [openrouter"))
            .expect("search query should render");
        let item_index = lines
            .iter()
            .position(|line| line.contains("Alpha"))
            .expect("list item should render");

        assert!(lines[search_index].contains("Esc clears"));
        assert!(search_index < item_index);
    }

    #[test]
    fn filtered_modal_summary_shows_matches_without_repeating_query() {
        let list = ModalListState::new(
            vec![InlineListItem {
                title: "gpt-5".to_string(),
                subtitle: Some("General reasoning".to_string()),
                badge: None,
                indent: 0,
                selection: Some(InlineListSelection::Model(0)),
                search_value: Some("gpt-5".to_string()),
            }],
            None,
        );
        let styles = modal_render_styles();
        let mut list = list;
        list.apply_search("gpt");

        let summary = modal_list_summary_line(&list, &styles, None).expect("summary should exist");
        let text = line_text(&summary);

        assert!(text.contains("Matches 1 of 1"));
        assert!(!text.contains("gpt"));
        assert!(!text.contains("Filter:"));
    }

    #[test]
    fn instruction_highlight_markup_strips_bold_markers() {
        let styles = modal_render_styles();
        let mut list = ModalListState::new(Vec::new(), None);
        let instructions = vec!["Header".to_string(), "**ABCD-EFGH**".to_string()];
        let backend = TestBackend::new(40, 8);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| {
                render_modal_body(
                    frame,
                    Rect::new(0, 0, 40, 8),
                    ModalBodyContext {
                        instructions: &instructions,
                        footer_hint: None,
                        list: Some(&mut list),
                        styles: &styles,
                        secure_prompt: None,
                        search: None,
                        input: "",
                        cursor: 0,
                    },
                    None,
                    None,
                    Style::default(),
                    Style::default(),
                );
            })
            .expect("modal render should succeed");

        let buffer = terminal.backend().buffer();
        let rendered = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .filter_map(|x| buffer.cell((x, y)).map(|cell| cell.symbol().to_string()))
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("ABCD-EFGH"));
        assert!(!rendered.contains("**ABCD-EFGH**"));
    }
}
