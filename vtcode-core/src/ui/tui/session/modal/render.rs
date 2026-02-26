use crate::config::constants::ui;
use crate::ui::markdown::render_markdown;
use crate::ui::tui::types::SecurePromptConfig;
use ratatui::{
    prelude::*,
    widgets::{Block, List, ListItem, Paragraph, Tabs, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::layout::{ModalBodyContext, ModalRenderStyles, ModalSection};
use super::state::{ModalListState, ModalSearchState, WizardModalState, WizardStepState};
use crate::ui::tui::session::terminal_capabilities;
use std::mem;

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
            rows.push(std::mem::take(&mut current));
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

fn render_markdown_lines_for_modal(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for line in markdown_to_plain_lines(text) {
        let wrapped = wrap_line_to_width(line.as_str(), width);
        for wrapped_line in wrapped {
            lines.push(Line::from(Span::styled(wrapped_line, style)));
        }
    }

    if lines.is_empty() {
        vec![Line::default()]
    } else {
        lines
    }
}

pub fn render_modal_list(
    frame: &mut Frame<'_>,
    area: Rect,
    list: &mut ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
) {
    if list.visible_indices.is_empty() {
        list.list_state.select(None);
        *list.list_state.offset_mut() = 0;
        let message = Paragraph::new(Line::from(Span::styled(
            ui::MODAL_LIST_NO_RESULTS_MESSAGE.to_owned(),
            styles.detail,
        )))
        .block(modal_list_block(list, styles, footer_hint))
        .wrap(Wrap { trim: true });
        frame.render_widget(message, area);
        return;
    }

    let viewport_rows = area.height.saturating_sub(2);
    list.set_viewport_rows(viewport_rows);
    list.ensure_visible(viewport_rows);
    let items = modal_list_items(list, styles);
    let widget = List::new(items)
        .block(modal_list_block(list, styles, footer_hint))
        .highlight_style(styles.highlight)
        .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
        .repeat_highlight_symbol(true);
    frame.render_stateful_widget(widget, area, &mut list.list_state);
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
        .divider(" ")
        .padding("← ", " →")
        .highlight_style(styles.highlight);

    frame.render_widget(tabs, area);
}

/// Render wizard modal body including tabs, question, and list
#[allow(dead_code)]
pub fn render_wizard_modal_body(
    frame: &mut Frame<'_>,
    area: Rect,
    wizard: &mut WizardModalState,
    styles: &ModalRenderStyles,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let is_multistep = wizard.mode == crate::ui::tui::types::WizardModalMode::MultiStep;
    let current_step_state = wizard.steps.get(wizard.current_step);
    let has_notes = current_step_state.is_some_and(|s| s.notes_active || !s.notes.is_empty());
    let instruction_lines = wizard.instruction_lines();
    let content_width = area.width.max(1) as usize;
    let header_lines = if is_multistep {
        render_markdown_lines_for_modal(
            wizard.question_header().as_str(),
            content_width,
            styles.header,
        )
    } else {
        Vec::new()
    };
    let question_lines = wizard
        .steps
        .get(wizard.current_step)
        .map(|step| {
            render_markdown_lines_for_modal(step.question.as_str(), content_width, styles.header)
        })
        .unwrap_or_else(|| vec![Line::default()]);

    // Layout: [Header (1)] [Search (optional)] [Question (2)] [List] [Notes?] [Instructions?]
    let mut constraints = Vec::new();
    if is_multistep {
        constraints.push(Constraint::Length(
            header_lines.len().min(u16::MAX as usize) as u16,
        ));
    } else {
        constraints.push(Constraint::Length(1));
    }
    if wizard.search.is_some() {
        constraints.push(Constraint::Length(3));
    }
    constraints.push(Constraint::Length(
        question_lines.len().max(1).min(u16::MAX as usize) as u16,
    ));
    constraints.push(Constraint::Min(3));
    if has_notes {
        constraints.push(Constraint::Length(1));
    }
    if !instruction_lines.is_empty() {
        constraints.push(Constraint::Length(
            instruction_lines.len().min(u16::MAX as usize) as u16,
        ));
    }

    let chunks = Layout::vertical(constraints).split(area);

    let mut idx = 0;
    if is_multistep {
        let header = Paragraph::new(header_lines).wrap(Wrap { trim: false });
        frame.render_widget(header, chunks[idx]);
    } else {
        render_wizard_tabs(
            frame,
            chunks[idx],
            &wizard.steps,
            wizard.current_step,
            styles,
        );
    }
    idx += 1;

    if let Some(search) = wizard.search.as_ref() {
        render_modal_search(frame, chunks[idx], search, styles);
        idx += 1;
    }

    let question = Paragraph::new(question_lines).wrap(Wrap { trim: false });
    frame.render_widget(question, chunks[idx]);
    idx += 1;

    if let Some(step) = wizard.steps.get_mut(wizard.current_step) {
        render_modal_list(frame, chunks[idx], &mut step.list, styles, None);
    }
    idx += 1;

    if let Some(step) = wizard.steps.get(wizard.current_step)
        && (step.notes_active || !step.notes.is_empty())
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

        let notes = Paragraph::new(Line::from(spans));
        frame.render_widget(notes, chunks[idx]);
        idx += 1;
    }

    if !instruction_lines.is_empty() && idx < chunks.len() {
        let lines = instruction_lines
            .into_iter()
            .map(|line| Line::from(Span::styled(line, styles.hint)))
            .collect::<Vec<_>>();
        let instructions = Paragraph::new(lines);
        frame.render_widget(instructions, chunks[idx]);
    }
}

fn modal_list_block(
    list: &ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
) -> Block<'static> {
    let mut block = Block::bordered()
        .border_type(terminal_capabilities::get_border_type())
        .border_style(styles.border);
    if let Some(summary) = modal_list_summary_line(list, styles, footer_hint) {
        block = block.title_bottom(summary);
    }
    block
}

#[allow(clippy::const_is_empty)]
fn modal_list_summary_line(
    list: &ModalListState,
    styles: &ModalRenderStyles,
    footer_hint: Option<&str>,
) -> Option<Line<'static>> {
    if !list.filter_active() {
        return footer_hint.map(|hint| Line::from(Span::styled(hint.to_string(), styles.hint)));
    }

    let mut spans = Vec::new();
    if let Some(query) = list.filter_query().filter(|value| !value.is_empty()) {
        spans.push(Span::styled(
            format!("{}:", ui::MODAL_LIST_SUMMARY_FILTER_LABEL),
            styles.detail,
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(query.to_owned(), styles.selectable));
    }

    let matches = list.visible_selectable_count();
    let total = list.total_selectable();
    if matches == 0 {
        if !spans.is_empty() {
            spans.push(Span::styled(
                ui::MODAL_LIST_SUMMARY_SEPARATOR.to_owned(),
                styles.detail,
            ));
        }
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
        if !spans.is_empty() {
            spans.push(Span::styled(
                ui::MODAL_LIST_SUMMARY_SEPARATOR.to_owned(),
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
                    render_modal_list(
                        frame,
                        *chunk,
                        list_state,
                        context.styles,
                        context.footer_hint,
                    );
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

    let block = Block::bordered()
        .title(Span::styled(
            ui::MODAL_INSTRUCTIONS_TITLE.to_owned(),
            styles.instruction_title,
        ))
        .border_type(terminal_capabilities::get_border_type())
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
    spans.push(Span::styled("▌".to_owned(), styles.highlight));

    let block = Block::bordered()
        .title(Span::styled(search.label.clone(), styles.header))
        .border_type(terminal_capabilities::get_border_type())
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
    _visible_index: usize,
    item_index: usize,
    styles: &ModalRenderStyles,
) -> ListItem<'static> {
    let item = match list.items.get(item_index) {
        Some(i) => i,
        None => {
            tracing::warn!("modal list item index {item_index} out of bounds");
            return ListItem::new("");
        }
    };
    if item.is_divider {
        let divider = if item.title.is_empty() {
            ui::INLINE_BLOCK_HORIZONTAL.repeat(8)
        } else {
            item.title.clone()
        };
        return ListItem::new(vec![Line::from(Span::styled(divider, styles.divider))]);
    }

    let indent = "  ".repeat(item.indent as usize);

    let mut primary_spans = Vec::new();

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

#[cfg(test)]
mod tests {
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect::<String>()
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
}
