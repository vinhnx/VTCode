use crate::config::constants::ui;
use crate::ui::tui::types::SecurePromptConfig;
use ratatui::prelude::*;
use terminal_size::{Height, Width, terminal_size};
use unicode_width::UnicodeWidthStr;

use super::state::{ModalListState, ModalSearchState};
use crate::ui::tui::session::measure_text_width;

pub struct ModalRenderStyles {
    pub border: Style,
    pub highlight: Style,
    pub badge: Style,
    pub header: Style,
    pub selectable: Style,
    pub detail: Style,
    pub search_match: Style,
    pub title: Style,
    pub divider: Style,
    pub instruction_border: Style,
    pub instruction_title: Style,
    pub instruction_bullet: Style,
    pub instruction_body: Style,
    pub hint: Style,
}

pub struct ModalListLayout {
    pub text_area: Option<Rect>,
    pub list_area: Rect,
}

pub struct ModalBodyContext<'a, 'b> {
    pub instructions: &'a [String],
    pub footer_hint: Option<&'a str>,
    pub list: Option<&'b mut ModalListState>,
    pub styles: &'a ModalRenderStyles,
    pub secure_prompt: Option<&'a SecurePromptConfig>,
    pub search: Option<&'a ModalSearchState>,
    pub input: &'a str,
    pub cursor: usize,
}

impl ModalListLayout {
    pub fn new(area: Rect, text_line_count: usize) -> Self {
        if text_line_count == 0 {
            let chunks = Layout::vertical([Constraint::Min(3)]).split(area);
            return Self {
                text_area: None,
                list_area: chunks[0],
            };
        }

        let paragraph_height = (text_line_count.min(u16::MAX as usize) as u16).saturating_add(1);
        let chunks = Layout::vertical([Constraint::Length(paragraph_height), Constraint::Min(3)])
            .split(area);

        Self {
            text_area: Some(chunks[0]),
            list_area: chunks[1],
        }
    }
}

fn terminal_dimensions() -> Option<(u16, u16)> {
    terminal_size().map(|(Width(width), Height(height))| (width, height))
}

pub fn compute_modal_area(
    viewport: Rect,
    text_lines: usize,
    prompt_lines: usize,
    search_lines: usize,
    has_list: bool,
) -> Rect {
    if viewport.width == 0 || viewport.height == 0 {
        return Rect::new(viewport.x, viewport.y, 0, 0);
    }

    let (term_width, term_height) = terminal_dimensions()
        .map(|(w, h)| (w.max(1), h.max(1)))
        .unwrap_or((viewport.width, viewport.height));
    let available_width = viewport.width.min(term_width);
    let available_height = viewport.height.min(term_height);

    let ratio_width = ((available_width as f32) * ui::MODAL_WIDTH_RATIO).round() as u16;
    let ratio_height = ((available_height as f32) * ui::MODAL_HEIGHT_RATIO).round() as u16;
    let max_width = ((available_width as f32) * ui::MODAL_MAX_WIDTH_RATIO).round() as u16;
    let max_height = ((available_height as f32) * ui::MODAL_MAX_HEIGHT_RATIO).round() as u16;

    let min_width = ui::MODAL_MIN_WIDTH.min(available_width.max(1));
    let base_min_height = ui::MODAL_MIN_HEIGHT.min(available_height.max(1));
    let min_height = if has_list {
        ui::MODAL_LIST_MIN_HEIGHT
            .min(available_height.max(1))
            .max(base_min_height)
    } else {
        base_min_height
    };

    let mut width = ratio_width.max(min_width);
    width = width.min(max_width.max(min_width)).min(available_width);

    let total_lines = text_lines
        .saturating_add(prompt_lines)
        .saturating_add(search_lines);
    let text_height = total_lines as u16;
    let mut height = text_height
        .saturating_add(ui::MODAL_CONTENT_VERTICAL_PADDING)
        .max(min_height)
        .max(ratio_height);
    if has_list {
        height = height.max(ui::MODAL_LIST_MIN_HEIGHT.min(available_height));
    }
    height = height.min(max_height.max(min_height)).min(available_height);

    let x = viewport.x + (viewport.width.saturating_sub(width)) / 2;
    let y = viewport.y + (viewport.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

pub fn modal_content_width(
    lines: &[String],
    list: Option<&ModalListState>,
    secure_prompt: Option<&SecurePromptConfig>,
    search: Option<&ModalSearchState>,
) -> u16 {
    let mut width = lines
        .iter()
        .map(|line| UnicodeWidthStr::width(line.as_str()) as u16)
        .max()
        .unwrap_or(0);

    if let Some(list_state) = list {
        for item in &list_state.items {
            let badge_width = item
                .badge
                .as_ref()
                .map(|badge| UnicodeWidthStr::width(badge.as_str()).saturating_add(3))
                .unwrap_or(0);
            let title_width = UnicodeWidthStr::width(item.title.as_str());
            let subtitle_width = item
                .subtitle
                .as_ref()
                .map(|text| UnicodeWidthStr::width(text.as_str()))
                .unwrap_or(0);
            let indent_width = usize::from(item.indent) * 2;

            let primary_width = indent_width
                .saturating_add(badge_width)
                .saturating_add(title_width) as u16;
            let secondary_width = indent_width.saturating_add(subtitle_width) as u16;

            width = width.max(primary_width).max(secondary_width);
        }
    }

    if let Some(prompt) = secure_prompt {
        let label_width = measure_text_width(prompt.label.as_str());
        let prompt_width = label_width.saturating_add(6).max(ui::MODAL_MIN_WIDTH);
        width = width.max(prompt_width);
    }

    if let Some(search_state) = search {
        let label_width = measure_text_width(search_state.label.as_str());
        let content_width = if search_state.query.is_empty() {
            search_state
                .placeholder
                .as_deref()
                .map(measure_text_width)
                .unwrap_or(0)
        } else {
            measure_text_width(search_state.query.as_str())
        };
        let search_width = label_width
            .saturating_add(content_width)
            .saturating_add(ui::MODAL_CONTENT_HORIZONTAL_PADDING);
        width = width.max(search_width.max(ui::MODAL_MIN_WIDTH));
    }

    width
}

#[derive(Clone, Copy)]
pub enum ModalSection {
    Search,
    Instructions,
    Prompt,
    List,
}
