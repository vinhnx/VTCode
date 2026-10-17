use crate::tui::config::constants::ui;
use crate::tui::ui::markdown::render_markdown;
use crate::tui::ui::tui::session::inline_list::{
    InlineListRow, list_cursor, row_height, selection_padding, selection_padding_width,
};
use crate::tui::ui::tui::session::list_panel::{
    SharedListPanelSections, SharedListPanelStyles, SharedListWidgetModel, render_shared_list_panel,
};
use crate::tui::ui::tui::types::{InlineItemKind, InlineListSelection, InlineStatus, InlineTone, SecurePromptConfig};
use ratatui::{
    prelude::*,
    widgets::{Paragraph, Tabs, Wrap},
};
use ratatui_cheese::input::{Input, InputState, InputStyles};
use unicode_width::UnicodeWidthStr;

use super::layout::{ModalBodyContext, ModalRenderStyles, ModalSection};
use super::state::{ModalListState, ModalSearchState, WizardModalState, WizardStepState};
use crate::tui::core_tui::session::transcript_links::{TranscriptFileLinkTarget, decorate_detected_link_lines};
use crate::tui::core_tui::style::ratatui_style_from_inline;
use crate::tui::ui::shell_syntax::{ShellLineStyles, shell_syntax_segments};
use crate::tui::ui::tui::session::wrapping;
use ratatui::style::Color as RatatuiColor;
use std::mem;
use std::path::Path;

mod inputs;
mod instructions;
mod list_item;
mod wizard;

pub(crate) use inputs::*;
pub(crate) use instructions::*;
pub(crate) use list_item::{highlight_segments, modal_badge_style, modal_list_item_lines, tone_style};
pub(super) use wizard::modal_text_area_aligned_with_list;
pub(crate) use wizard::*;

fn markdown_to_plain_lines(text: &str) -> Vec<String> {
    let mut lines = render_markdown(text)
        .into_iter()
        .map(|line| line.segments.into_iter().map(|segment| segment.text).collect::<String>())
        .collect::<Vec<_>>();

    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    if lines.is_empty() { vec![String::new()] } else { lines }
}
fn markdown_lines_for_modal(text: &str, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in markdown_to_plain_lines(text) {
        lines.push(Line::from(Span::styled(line, style)));
    }

    if lines.is_empty() { vec![Line::default()] } else { lines }
}

#[cfg(test)]
fn render_markdown_lines_for_modal(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    wrapping::wrap_lines_preserving_urls(markdown_lines_for_modal(text, style), width)
}

#[derive(Clone, Debug)]
pub struct ModalInlineEditor {
    pub(crate) item_index: usize,
    pub(crate) label: String,
    pub(crate) text: String,
    pub(crate) placeholder: Option<String>,
    pub(crate) active: bool,
}

#[derive(Default)]
pub(crate) struct ModalRenderOutcome {
    pub(crate) list_area: Option<Rect>,
    pub(crate) text_areas: Vec<Rect>,
    pub(crate) link_targets: Vec<TranscriptFileLinkTarget>,
}

impl ModalRenderOutcome {
    pub(crate) fn push_text_area(&mut self, area: Rect) {
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

    let (decorated, targets) =
        decorate_detected_link_lines(lines, area, workspace_root, last_mouse_position, link_style, hovered_link_style);
    frame.render_widget(Paragraph::new(decorated).wrap(Wrap { trim: false }), area);
    outcome.push_text_area(area);
    outcome.link_targets.extend(targets);
}
struct ModalListPanelModel<'a> {
    list: &'a mut ModalListState,
    styles: &'a ModalRenderStyles,
    inline_editor: Option<&'a ModalInlineEditor>,
    show_numbers: bool,
}

impl SharedListWidgetModel for ModalListPanelModel<'_> {
    fn rows(&self, width: u16) -> Vec<(InlineListRow, u16)> {
        if self.list.visible_indices.is_empty() {
            return vec![(
                InlineListRow::single(
                    Line::from(Span::styled(ui::MODAL_LIST_NO_RESULTS_MESSAGE.to_owned(), self.styles.detail)),
                    self.styles.detail,
                ),
                1_u16,
            )];
        }

        let selection_gutter = selection_padding_width() as u16;
        let content_width = width.saturating_sub(selection_gutter) as usize;
        let selected_visible = self.list.list_state.selected();
        // Gate once: per-row numbers stay cheap so crowded lists do not
        // pay a quadratic scan for badges they will not show.
        let numbered = self.show_numbers && self.list.numbered_shortcuts();
        self.list
            .visible_indices
            .iter()
            .enumerate()
            .map(|(visible_index, &item_index)| {
                let is_selected = selected_visible == Some(visible_index)
                    && self.list.items.get(item_index).is_some_and(|i| i.selection.is_some());
                let shortcut_number = numbered.then(|| self.list.shortcut_number(visible_index)).flatten();
                let lines = modal_list_item_lines(
                    self.list,
                    visible_index,
                    item_index,
                    self.styles,
                    content_width,
                    self.inline_editor,
                    is_selected,
                    shortcut_number,
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
        let max_offset = self.list.max_scroll_offset();
        *self.list.list_state.offset_mut() = offset.min(max_offset);
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
    show_numbers: bool,
    status: Option<&InlineStatus>,
) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let mut info = Vec::new();
    // Status strip sits above the keyboard/filter hint so the last action is
    // still readable while the user keeps navigating.
    if let Some(status) = status {
        let tone = tone_style(status.tone, styles);
        info.push(Line::from(vec![Span::styled("• ", tone), Span::styled(status.message.clone(), tone)]));
    }
    info.extend(modal_list_summary_line(list, styles, footer_hint));
    let mut panel_model = ModalListPanelModel { list, styles, inline_editor, show_numbers };
    let sections = SharedListPanelSections { header: Vec::new(), info, search: None };
    render_shared_list_panel(
        frame,
        area,
        sections,
        SharedListPanelStyles {
            base_style: styles.background,
            selected_style: Some(styles.highlight),
            text_style: styles.detail,
            divider_style: Some(styles.border),
            input_styles: InputStyles::default(),
            show_divider: false,
        },
        &mut panel_model,
    );

    area
}
fn modal_list_summary_line(
    list: &ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
) -> Vec<Line<'static>> {
    if !list.filter_active() {
        return match list.non_filter_summary_text(footer_hint) {
            Some(message) => vec![Line::from(Span::styled(message, styles.hint))],
            None => Vec::new(),
        };
    }

    let mut spans = Vec::new();
    let matches = list.visible_selectable_count();
    let total = list.total_selectable();
    if matches == 0 {
        spans.push(Span::styled(ui::MODAL_LIST_SUMMARY_NO_MATCHES.to_owned(), styles.search_match));
        if !ui::MODAL_LIST_SUMMARY_RESET_HINT.is_empty() {
            spans.push(Span::styled(
                format!("{}{}", ui::MODAL_LIST_SUMMARY_SEPARATOR, ui::MODAL_LIST_SUMMARY_RESET_HINT),
                styles.hint,
            ));
        }
    } else {
        spans.push(Span::styled(format!("{matches} / {total}"), styles.detail));
    }

    if spans.is_empty() {
        Vec::new()
    } else {
        vec![Line::from(spans)]
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
    let has_instructions = context.instructions.iter().any(|line| !line.trim().is_empty());
    let has_secure_prompt = context.secure_prompt.is_some();
    let has_search = context.search.is_some();
    let has_list = context.list.is_some();
    // Eight wrapped visual rows: prompt + full-text summary (up to four
    // wrapped rows) + steps + overflow. Matches `MAX_INLINE_INSTRUCTION_ROWS`
    // in `modal_renderer.rs` so the body viewport and the claimed modal
    // height agree and no blank gap or clipped summary appears.
    let instruction_row_limit = if has_secure_prompt || has_search || has_list {
        8
    } else {
        area.height.max(1) as usize
    };
    let instruction_lines = if has_instructions {
        modal_instruction_lines(area, context.instructions, context.styles)
    } else {
        Vec::new()
    };
    let instruction_row_count =
        wrapping::wrap_lines_preserving_urls(instruction_lines.clone(), area.width.max(1) as usize)
            .len()
            .clamp(1, instruction_row_limit);
    if has_instructions {
        sections.push(ModalSection::Instructions);
    }
    if has_secure_prompt {
        sections.push(ModalSection::Prompt);
    }
    if has_search {
        sections.push(ModalSection::Search);
    }
    if has_list {
        sections.push(ModalSection::List);
    }

    if sections.is_empty() {
        return outcome;
    }

    let mut constraints = Vec::new();
    for section in &sections {
        match section {
            ModalSection::Search => {
                let rows = context
                    .search
                    .map(|s| if s.label.is_empty() { 1 } else { 2 })
                    .unwrap_or(2)
                    .min(area.height);
                constraints.push(Constraint::Length(rows));
            }
            ModalSection::Instructions => {
                let visible_rows = instruction_row_count as u16;
                constraints.push(Constraint::Length(visible_rows.min(area.height)));
            }
            ModalSection::Prompt => constraints.push(Constraint::Length(2.min(area.height))),
            ModalSection::List => constraints.push(Constraint::Min(1)),
        }
    }
    let show_list_divider = context.list.is_some();
    if show_list_divider {
        // The divider is spliced directly before the List chunk, so List must
        // be the final section for the `chunk_idx` bookkeeping below to hold.
        debug_assert!(
            matches!(sections.last(), Some(ModalSection::List)),
            "list divider assumes List is the final modal section"
        );
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
                    render_secure_prompt(frame, chunk, config, context.input, context.cursor, context.input_styles);
                }
            }
            ModalSection::Search => {
                if let Some(config) = context.search {
                    render_modal_search(frame, chunk, config, context.input_styles);
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
                        context.search.is_none(),
                        context.status,
                    ));
                }
            }
        }
        chunk_idx += 1;
    }

    outcome
}

#[cfg(test)]
mod tests;
