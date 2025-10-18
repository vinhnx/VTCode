use crate::config::constants::ui;
use crate::ui::tui::types::{
    InlineListItem, InlineListSearchConfig, InlineListSelection, SecurePromptConfig,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};
use terminal_size::{Height, Width, terminal_size};
use tui_popup::PopupState;
use tui_prompts::{Prompt, State, TextPrompt, TextRenderStyle, TextState};
use unicode_width::UnicodeWidthStr;

use super::measure_text_width;
use std::mem;

#[derive(Clone)]
pub struct ModalState {
    pub title: String,
    pub lines: Vec<String>,
    pub list: Option<ModalListState>,
    pub secure_prompt: Option<SecurePromptConfig>,
    pub popup_state: PopupState,
    pub restore_input: bool,
    pub restore_cursor: bool,
    pub search: Option<ModalSearchState>,
}

#[derive(Clone)]
pub struct ModalListState {
    pub items: Vec<ModalListItem>,
    pub visible_indices: Vec<usize>,
    pub list_state: ListState,
    pub total_selectable: usize,
    pub filter_terms: Vec<String>,
    pub filter_query: Option<String>,
}

#[derive(Clone)]
pub struct ModalListItem {
    pub title: String,
    pub subtitle: Option<String>,
    pub badge: Option<String>,
    pub indent: u8,
    pub selection: Option<InlineListSelection>,
    pub search_value: Option<String>,
    pub is_divider: bool,
}

#[derive(Clone)]
pub struct ModalSearchState {
    pub label: String,
    pub placeholder: Option<String>,
    pub query: String,
}

impl From<InlineListSearchConfig> for ModalSearchState {
    fn from(config: InlineListSearchConfig) -> Self {
        Self {
            label: config.label,
            placeholder: config.placeholder,
            query: String::new(),
        }
    }
}

impl ModalSearchState {
    pub fn insert(&mut self, value: &str) {
        for ch in value.chars() {
            if matches!(ch, '\n' | '\r') {
                continue;
            }
            self.query.push(ch);
        }
    }

    pub fn push_char(&mut self, ch: char) {
        self.query.push(ch);
    }

    pub fn backspace(&mut self) -> bool {
        if self.query.pop().is_some() {
            return true;
        }
        false
    }

    pub fn clear(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        self.query.clear();
        true
    }
}

impl ModalListItem {
    fn is_header(&self) -> bool {
        self.selection.is_none() && !self.is_divider
    }

    fn matches(&self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let Some(value) = self.search_value.as_ref() else {
            return false;
        };
        fuzzy_match(query, value)
    }
}

#[allow(clippy::const_is_empty)]
pub fn is_divider_title(item: &InlineListItem) -> bool {
    if item.selection.is_some() {
        return false;
    }
    if item.indent != 0 {
        return false;
    }
    if item.subtitle.is_some() || item.badge.is_some() {
        return false;
    }
    let symbol = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL;
    if symbol.is_empty() {
        return false;
    }
    item.title
        .chars()
        .all(|ch| symbol.chars().any(|needle| needle == ch))
}

pub fn normalize_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<String>>()
        .join(" ")
}

pub fn fuzzy_match(query: &str, candidate: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    query
        .split_whitespace()
        .filter(|segment| !segment.is_empty())
        .all(|segment| fuzzy_subsequence(segment, candidate))
}

pub fn fuzzy_subsequence(needle: &str, haystack: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let mut needle_chars = needle.chars();
    let mut current = match needle_chars.next() {
        Some(value) => value,
        None => return true,
    };
    for ch in haystack.chars() {
        if ch == current {
            match needle_chars.next() {
                Some(next) => current = next,
                None => return true,
            }
        }
    }
    false
}

pub struct ModalRenderStyles {
    pub border: Style,
    pub highlight: Style,
    pub badge: Style,
    pub header: Style,
    pub selectable: Style,
    pub detail: Style,
    pub search_match: Style,
    pub title: Style,
    pub index: Style,
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
            let chunks = Layout::vertical(vec![Constraint::Min(3)]).split(area);
            return Self {
                text_area: None,
                list_area: chunks[0],
            };
        }

        let paragraph_height = (text_line_count.min(u16::MAX as usize) as u16).saturating_add(1);
        let chunks = Layout::vertical(vec![
            Constraint::Length(paragraph_height),
            Constraint::Min(3),
        ])
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
    width_hint: u16,
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

    let mut width = width_hint
        .saturating_add(ui::MODAL_CONTENT_HORIZONTAL_PADDING)
        .max(min_width)
        .max(ratio_width);
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

pub fn render_modal_list(
    frame: &mut Frame<'_>,
    area: Rect,
    list: &mut ModalListState,
    styles: &ModalRenderStyles,
) {
    if list.visible_indices.is_empty() {
        list.list_state.select(None);
        *list.list_state.offset_mut() = 0;
        let message = Paragraph::new(Line::from(Span::styled(
            ui::MODAL_LIST_NO_RESULTS_MESSAGE.to_string(),
            styles.detail,
        )))
        .block(modal_list_block(list, styles))
        .wrap(Wrap { trim: true });
        frame.render_widget(message, area);
        return;
    }

    list.ensure_visible(area.height);
    let items = modal_list_items(list, styles);
    let widget = List::new(items)
        .block(modal_list_block(list, styles))
        .highlight_style(styles.highlight)
        .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(widget, area, &mut list.list_state);
}

fn modal_list_block(list: &ModalListState, styles: &ModalRenderStyles) -> Block<'static> {
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(styles.border);
    if let Some(summary) = modal_list_summary_line(list, styles) {
        block = block.title_bottom(summary);
    }
    block
}

#[allow(clippy::const_is_empty)]
fn modal_list_summary_line(
    list: &ModalListState,
    styles: &ModalRenderStyles,
) -> Option<Line<'static>> {
    if !list.filter_active() {
        return None;
    }

    let mut spans = Vec::new();
    if let Some(query) = list.filter_query().filter(|value| !value.is_empty()) {
        spans.push(Span::styled(
            format!("{}:", ui::MODAL_LIST_SUMMARY_FILTER_LABEL),
            styles.detail,
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(query.to_string(), styles.selectable));
    }

    let matches = list.visible_selectable_count();
    let total = list.total_selectable();
    if matches == 0 {
        if !spans.is_empty() {
            spans.push(Span::styled(
                ui::MODAL_LIST_SUMMARY_SEPARATOR.to_string(),
                styles.detail,
            ));
        }
        spans.push(Span::styled(
            ui::MODAL_LIST_SUMMARY_NO_MATCHES.to_string(),
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
        if !spans.is_empty() {
            spans.push(Span::styled(
                ui::MODAL_LIST_SUMMARY_SEPARATOR.to_string(),
                styles.detail,
            ));
        }
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

#[derive(Clone, Copy)]
pub enum ModalSection {
    Search,
    Instructions,
    Prompt,
    List,
}

pub fn render_modal_body(frame: &mut Frame<'_>, area: Rect, context: ModalBodyContext<'_, '_>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut sections = Vec::new();
    let has_instructions = context
        .instructions
        .iter()
        .any(|line| !line.trim().is_empty());
    if context.search.is_some() {
        sections.push(ModalSection::Search);
    }
    if has_instructions {
        sections.push(ModalSection::Instructions);
    }
    if context.secure_prompt.is_some() {
        sections.push(ModalSection::Prompt);
    }
    if context.list.is_some() {
        sections.push(ModalSection::List);
    }

    if sections.is_empty() {
        return;
    }

    let mut constraints = Vec::new();
    for section in &sections {
        match section {
            ModalSection::Search => constraints.push(Constraint::Length(3.min(area.height))),
            ModalSection::Instructions => {
                let visible_rows = context.instructions.len().max(1) as u16;
                let height = visible_rows.saturating_add(2);
                constraints.push(Constraint::Length(height.min(area.height)));
            }
            ModalSection::Prompt => constraints.push(Constraint::Length(3.min(area.height))),
            ModalSection::List => constraints.push(Constraint::Min(3)),
        }
    }

    let chunks = Layout::vertical(constraints).split(area);
    let mut list_state = context.list;

    for (section, chunk) in sections.into_iter().zip(chunks.iter()) {
        match section {
            ModalSection::Instructions => {
                if chunk.height > 0 && has_instructions {
                    render_modal_instructions(frame, *chunk, context.instructions, context.styles);
                }
            }
            ModalSection::Prompt => {
                if let Some(config) = context.secure_prompt {
                    render_secure_prompt(frame, *chunk, config, context.input, context.cursor);
                }
            }
            ModalSection::Search => {
                if let Some(config) = context.search {
                    render_modal_search(frame, *chunk, config, context.styles);
                }
            }
            ModalSection::List => {
                if let Some(list_state) = list_state.as_deref_mut() {
                    render_modal_list(frame, *chunk, list_state, context.styles);
                }
            }
        }
    }
}

fn render_modal_instructions(
    frame: &mut Frame<'_>,
    area: Rect,
    instructions: &[String],
    styles: &ModalRenderStyles,
) {
    fn wrap_instruction_lines(text: &str, width: usize) -> Vec<String> {
        if width == 0 {
            return vec![text.to_string()];
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
                current = word.to_string();
            } else {
                current.push(' ');
                current.push_str(word);
            }
        }

        if !current.is_empty() {
            lines.push(current);
        }

        if lines.is_empty() {
            vec![text.to_string()]
        } else {
            lines
        }
    }

    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut items = Vec::new();
    let mut first_content_rendered = false;
    let content_width = area.width.saturating_sub(4) as usize;
    let bullet_prefix = format!("{} ", ui::MODAL_INSTRUCTIONS_BULLET);
    let bullet_indent = " ".repeat(UnicodeWidthStr::width(bullet_prefix.as_str()));

    for line in instructions {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            items.push(ListItem::new(Line::default()));
            continue;
        }

        let wrapped = wrap_instruction_lines(trimmed, content_width);
        if wrapped.is_empty() {
            items.push(ListItem::new(Line::default()));
            continue;
        }

        if !first_content_rendered {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                let style = if index == 0 {
                    styles.header
                } else {
                    styles.instruction_body
                };
                lines.push(Line::from(Span::styled(segment, style)));
            }
            items.push(ListItem::new(lines));
            first_content_rendered = true;
        } else {
            let mut lines = Vec::new();
            for (index, segment) in wrapped.into_iter().enumerate() {
                if index == 0 {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_prefix.clone(), styles.instruction_bullet),
                        Span::styled(segment, styles.instruction_body),
                    ]));
                } else {
                    lines.push(Line::from(vec![
                        Span::styled(bullet_indent.clone(), styles.instruction_bullet),
                        Span::styled(segment, styles.instruction_body),
                    ]));
                }
            }
            items.push(ListItem::new(lines));
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::default()));
    }

    let block = Block::default()
        .title(Span::styled(
            ui::MODAL_INSTRUCTIONS_TITLE.to_string(),
            styles.instruction_title,
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(styles.instruction_border);

    let widget = List::new(items)
        .block(block)
        .style(styles.instruction_body)
        .highlight_symbol("")
        .repeat_highlight_symbol(false);

    frame.render_widget(widget, area);
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

    let mut spans = Vec::new();
    if search.query.is_empty() {
        if let Some(placeholder) = &search.placeholder {
            spans.push(Span::styled(placeholder.clone(), styles.detail));
        }
    } else {
        spans.push(Span::styled(search.query.clone(), styles.selectable));
    }
    spans.push(Span::styled("▌".to_string(), styles.highlight));

    let block = Block::default()
        .title(Span::styled(search.label.clone(), styles.header))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(styles.border);

    let paragraph = Paragraph::new(Line::from(spans))
        .block(block)
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn render_secure_prompt(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &SecurePromptConfig,
    input: &str,
    cursor: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let grapheme_count = input.chars().count();
    let sanitized: String = std::iter::repeat('•').take(grapheme_count).collect();
    let cursor_chars = input[..cursor].chars().count();

    let mut state = TextState::new().with_value(sanitized);
    state.focus();
    *state.position_mut() = cursor_chars;

    let prompt =
        TextPrompt::from(config.label.clone()).with_render_style(TextRenderStyle::Password);
    prompt.draw(frame, area, &mut state);
}

fn highlight_segments(
    text: &str,
    normal_style: Style,
    highlight_style: Style,
    terms: &[String],
) -> Vec<Span<'static>> {
    if text.is_empty() {
        return vec![Span::styled(String::new(), normal_style)];
    }

    if terms.is_empty() {
        return vec![Span::styled(text.to_string(), normal_style)];
    }

    let lower = text.to_ascii_lowercase();
    let mut char_offsets: Vec<usize> = text.char_indices().map(|(offset, _)| offset).collect();
    char_offsets.push(text.len());
    let char_count = char_offsets.len().saturating_sub(1);
    if char_count == 0 {
        return vec![Span::styled(text.to_string(), normal_style)];
    }

    let mut highlight_flags = vec![false; char_count];
    for term in terms {
        let needle = term.to_ascii_lowercase();
        if needle.is_empty() {
            continue;
        }

        let mut search_start = 0usize;
        while search_start < lower.len() {
            let Some(pos) = lower[search_start..].find(&needle) else {
                break;
            };
            let byte_start = search_start + pos;
            let byte_end = byte_start + needle.len();
            let start_index = char_offsets.partition_point(|offset| *offset < byte_start);
            let end_index = char_offsets.partition_point(|offset| *offset < byte_end);
            for index in start_index..end_index.min(char_count) {
                highlight_flags[index] = true;
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

pub fn modal_list_items(
    list: &ModalListState,
    styles: &ModalRenderStyles,
) -> Vec<ListItem<'static>> {
    list.visible_indices
        .iter()
        .enumerate()
        .map(|(visible_index, &index)| modal_list_item(list, visible_index, index, styles))
        .collect()
}

fn modal_list_item(
    list: &ModalListState,
    visible_index: usize,
    item_index: usize,
    styles: &ModalRenderStyles,
) -> ListItem<'static> {
    let item = &list.items[item_index];
    if item.is_divider {
        let divider = if item.title.is_empty() {
            ui::INLINE_BLOCK_HORIZONTAL.repeat(8)
        } else {
            item.title.clone()
        };
        return ListItem::new(vec![Line::from(Span::styled(divider, styles.divider))]);
    }

    let indent = "  ".repeat(item.indent as usize);
    let ordinal = if item.selection.is_some() {
        format!("{:>2}", visible_index + 1)
    } else {
        String::new()
    };

    let mut primary_spans = Vec::new();
    if !ordinal.is_empty() {
        primary_spans.push(Span::styled(ordinal.clone(), styles.index));
        primary_spans.push(Span::styled(" ".to_string(), styles.index));
    }

    if !indent.is_empty() {
        primary_spans.push(Span::raw(indent.clone()));
    }

    if let Some(badge) = &item.badge {
        let badge_label = format!("[{}]", badge);
        primary_spans.push(Span::styled(badge_label, styles.badge));
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
        let mut secondary_spans = Vec::new();
        if !ordinal.is_empty() {
            let placeholder = " ".repeat(ordinal.chars().count());
            secondary_spans.push(Span::styled(placeholder, styles.index));
            secondary_spans.push(Span::styled(" ".to_string(), styles.index));
        }
        if !indent.is_empty() {
            secondary_spans.push(Span::raw(indent.clone()));
        }
        let subtitle_spans = highlight_segments(
            subtitle,
            styles.detail,
            styles.search_match,
            list.highlight_terms(),
        );
        secondary_spans.extend(subtitle_spans);
        lines.push(Line::from(secondary_spans));
    }

    lines.push(Line::default());
    ListItem::new(lines)
}

impl ModalListState {
    pub fn new(items: Vec<InlineListItem>, selected: Option<InlineListSelection>) -> Self {
        let converted: Vec<ModalListItem> = items
            .into_iter()
            .map(|item| {
                let is_divider = is_divider_title(&item);
                let search_value = item
                    .search_value
                    .as_ref()
                    .map(|value| value.to_ascii_lowercase());
                ModalListItem {
                    title: item.title,
                    subtitle: item.subtitle,
                    badge: item.badge,
                    indent: item.indent,
                    selection: item.selection,
                    search_value,
                    is_divider,
                }
            })
            .collect();
        let total_selectable = converted
            .iter()
            .filter(|item| item.selection.is_some())
            .count();
        let mut modal_state = Self {
            visible_indices: (0..converted.len()).collect(),
            items: converted,
            list_state: ListState::default(),
            total_selectable,
            filter_terms: Vec::new(),
            filter_query: None,
        };
        modal_state.select_initial(selected);
        modal_state
    }

    pub fn current_selection(&self) -> Option<InlineListSelection> {
        self.list_state
            .selected()
            .and_then(|index| self.visible_indices.get(index))
            .and_then(|&item_index| self.items.get(item_index))
            .and_then(|item| item.selection.clone())
    }

    pub fn select_previous(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let Some(mut index) = self.list_state.selected() else {
            if let Some(last) = self.last_selectable_index() {
                self.list_state.select(Some(last));
            }
            return;
        };

        while index > 0 {
            index -= 1;
            let item_index = self.visible_indices[index];
            if self.items[item_index].selection.is_some() {
                self.list_state.select(Some(index));
                return;
            }
        }

        if let Some(first) = self.first_selectable_index() {
            self.list_state.select(Some(first));
        } else {
            self.list_state.select(None);
        }
    }

    pub fn select_next(&mut self) {
        if self.visible_indices.is_empty() {
            return;
        }
        let mut index = self.list_state.selected().unwrap_or(usize::MAX);
        if index == usize::MAX {
            if let Some(first) = self.first_selectable_index() {
                self.list_state.select(Some(first));
            }
            return;
        }
        while index + 1 < self.visible_indices.len() {
            index += 1;
            let item_index = self.visible_indices[index];
            if self.items[item_index].selection.is_some() {
                self.list_state.select(Some(index));
                break;
            }
        }
    }

    fn ensure_visible(&mut self, viewport: u16) {
        let Some(selected) = self.list_state.selected() else {
            return;
        };
        if viewport == 0 {
            return;
        }
        let visible = viewport as usize;
        let offset = self.list_state.offset();
        if selected < offset {
            *self.list_state.offset_mut() = selected;
        } else if selected >= offset + visible {
            *self.list_state.offset_mut() = selected + 1 - visible;
        }
    }

    pub fn apply_search(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            self.visible_indices = (0..self.items.len()).collect();
            self.filter_terms.clear();
            self.filter_query = None;
            self.select_initial(None);
            return;
        }

        let normalized_query = normalize_query(trimmed);
        let terms = normalized_query
            .split_whitespace()
            .filter(|term| !term.is_empty())
            .map(|term| term.to_string())
            .collect::<Vec<_>>();
        let mut indices = Vec::new();
        let mut pending_divider: Option<usize> = None;
        let mut current_header: Option<usize> = None;
        let mut header_matches = false;
        let mut header_included = false;

        for (index, item) in self.items.iter().enumerate() {
            if item.is_divider {
                pending_divider = Some(index);
                current_header = None;
                header_matches = false;
                header_included = false;
                continue;
            }

            if item.is_header() {
                current_header = Some(index);
                header_matches = item.matches(&normalized_query);
                header_included = false;
                if header_matches {
                    if let Some(divider_index) = pending_divider.take() {
                        indices.push(divider_index);
                    }
                    indices.push(index);
                    header_included = true;
                }
                continue;
            }

            let item_matches = item.matches(&normalized_query);
            let include_item = header_matches || item_matches;
            if include_item {
                if let Some(divider_index) = pending_divider.take() {
                    indices.push(divider_index);
                }
                if let Some(header_index) = current_header {
                    if !header_included {
                        indices.push(header_index);
                        header_included = true;
                    }
                }
                indices.push(index);
            }
        }
        self.visible_indices = indices;
        self.filter_terms = terms;
        self.filter_query = Some(trimmed.to_string());
        self.select_initial(None);
    }

    fn select_initial(&mut self, preferred: Option<InlineListSelection>) {
        let mut selection_index = preferred.and_then(|needle| {
            self.visible_indices
                .iter()
                .position(|&idx| self.items[idx].selection.as_ref() == Some(&needle))
        });

        if selection_index.is_none() {
            selection_index = self.first_selectable_index();
        }

        self.list_state.select(selection_index);
        *self.list_state.offset_mut() = 0;
    }

    fn first_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .position(|&idx| self.items[idx].selection.is_some())
    }

    fn last_selectable_index(&self) -> Option<usize> {
        self.visible_indices
            .iter()
            .rposition(|&idx| self.items[idx].selection.is_some())
    }

    fn filter_active(&self) -> bool {
        self.filter_query
            .as_ref()
            .is_some_and(|value| !value.is_empty())
    }

    fn filter_query(&self) -> Option<&str> {
        self.filter_query.as_deref()
    }

    fn highlight_terms(&self) -> &[String] {
        &self.filter_terms
    }

    fn visible_selectable_count(&self) -> usize {
        self.visible_indices
            .iter()
            .filter(|&&idx| self.items[idx].selection.is_some())
            .count()
    }

    fn total_selectable(&self) -> usize {
        self.total_selectable
    }
}
