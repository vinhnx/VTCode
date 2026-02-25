#![allow(dead_code)]

use anstyle::Color as AnsiColorEnum;
use ratatui::{
    prelude::*,
    widgets::{Block, Clear, List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use super::super::style::{measure_text_width, ratatui_style_from_inline};
use super::super::types::{InlineMessageKind, InlineTextStyle};
use super::terminal_capabilities;
use super::{
    Session,
    config_palette::ConfigItemKind,
    file_palette::FilePalette,
    message::MessageLine,
    modal::{
        ModalBodyContext, ModalListLayout, ModalRenderStyles, compute_modal_area,
        modal_content_width, render_modal_body, render_wizard_modal_body,
    },
    text_utils,
};
use crate::config::constants::ui;

const USER_PREFIX: &str = "";

#[allow(dead_code)]
pub fn render(session: &mut Session, frame: &mut Frame<'_>) {
    let size = frame.area();
    if size.width == 0 || size.height == 0 {
        return;
    }

    // Clear entire frame if modal was just closed to remove artifacts
    if session.needs_full_clear {
        frame.render_widget(Clear, size);
        session.needs_full_clear = false;
    }

    // Handle deferred file browser trigger (after slash modal dismisses)
    if session.deferred_file_browser_trigger {
        session.deferred_file_browser_trigger = false;
        if session.input_enabled && session.modal.is_none() {
            // Insert @ to trigger file browser now that slash modal is gone
            session.input_manager.insert_char('@');
            session.check_file_reference_trigger();
            session.mark_dirty(); // Ensure UI updates
        }
    }

    // Pull any newly forwarded log entries before layout calculations
    session.poll_log_entries();

    let header_lines = session.header_lines();
    let header_height = session.header_height_from_lines(size.width, &header_lines);
    if header_height != session.header_rows {
        session.header_rows = header_height;
        recalculate_transcript_rows(session);
    }

    let status_height = if size.width > 0 && has_input_status(session) {
        1
    } else {
        0
    };
    let inner_width = size
        .width
        .saturating_sub(ui::INLINE_INPUT_PADDING_HORIZONTAL.saturating_mul(2));
    let desired_lines = session.desired_input_lines(inner_width);
    let block_height = Session::input_block_height_for_lines(desired_lines);
    let input_height = block_height.saturating_add(status_height);
    session.apply_input_height(input_height);

    let chunks = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(1),
        Constraint::Length(input_height),
    ])
    .split(size);

    let (header_area, transcript_area, input_area) = (chunks[0], chunks[1], chunks[2]);

    // Calculate available height for transcript
    apply_view_rows(session, transcript_area.height);

    // Render components
    session.render_header(frame, header_area, &header_lines);
    if session.show_logs {
        let split = Layout::vertical([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(transcript_area);
        render_transcript(session, frame, split[0]);
        render_log_view(session, frame, split[1]);
    } else {
        render_transcript(session, frame, transcript_area);
    }
    session.render_input(frame, input_area);
    render_modal(session, frame, size);
    super::slash::render_slash_palette(session, frame, size);
    render_file_palette(session, frame, size);
    render_config_palette(session, frame, size);
}

fn render_log_view(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    let block = Block::bordered()
        .title("Logs")
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let paragraph = Paragraph::new((*session.log_text()).clone()).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn modal_list_highlight_style(session: &Session) -> Style {
    session.styles.modal_list_highlight_style()
}

pub fn apply_view_rows(session: &mut Session, rows: u16) {
    let resolved = rows.max(2);
    if session.view_rows != resolved {
        session.view_rows = resolved;
        invalidate_scroll_metrics(session);
    }
    recalculate_transcript_rows(session);
    session.enforce_scroll_bounds();
}

pub fn apply_transcript_rows(session: &mut Session, rows: u16) {
    let resolved = rows.max(1);
    if session.transcript_rows != resolved {
        session.transcript_rows = resolved;
        invalidate_scroll_metrics(session);
    }
}

pub fn apply_transcript_width(session: &mut Session, width: u16) {
    if session.transcript_width != width {
        session.transcript_width = width;
        invalidate_scroll_metrics(session);
    }
}

fn render_transcript(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }
    let block = Block::new()
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    apply_transcript_rows(session, inner.height);

    let content_width = inner.width;
    if content_width == 0 {
        return;
    }
    apply_transcript_width(session, content_width);

    let viewport_rows = inner.height as usize;
    let padding = usize::from(ui::INLINE_TRANSCRIPT_BOTTOM_PADDING);
    let effective_padding = padding.min(viewport_rows.saturating_sub(1));

    // Skip expensive total_rows calculation if only scrolling (no content change)
    // This optimization saves ~30-50% CPU on viewport-only scrolls
    let total_rows = if session.transcript_content_changed {
        session.total_transcript_rows(content_width) + effective_padding
    } else {
        // Reuse last known total if content unchanged
        session
            .scroll_manager
            .last_known_total()
            .unwrap_or_else(|| session.total_transcript_rows(content_width) + effective_padding)
    };
    let (top_offset, _clamped_total_rows) =
        session.prepare_transcript_scroll(total_rows, viewport_rows);
    let vertical_offset = top_offset.min(session.scroll_manager.max_offset());
    session.transcript_view_top = vertical_offset;

    let visible_start = vertical_offset;
    let scroll_area = inner;

    // Use cached visible lines to avoid re-cloning on viewport-only scrolls
    let cached_lines =
        session.collect_transcript_window_cached(content_width, visible_start, viewport_rows);

    // Only clone if we need to mutate (fill or overlay)
    let fill_count = viewport_rows.saturating_sub(cached_lines.len());
    let visible_lines = if fill_count > 0 || !session.queued_inputs.is_empty() {
        // Need to mutate, so clone from Arc
        let mut lines = (*cached_lines).clone();
        if fill_count > 0 {
            let target_len = lines.len() + fill_count;
            lines.resize_with(target_len, Line::default);
        }
        session.overlay_queue_lines(&mut lines, content_width);
        lines
    } else {
        // No mutation needed, use Arc directly
        (*cached_lines).clone()
    };

    let paragraph = Paragraph::new(visible_lines)
        .style(default_style(session))
        .wrap(Wrap { trim: false });

    // Only clear if content actually changed, not on viewport-only scroll
    // This is a significant optimization: avoids expensive Clear operation on most scrolls
    // Combined with layout skip above, this reduces render CPU by ~50% during scrolling
    if session.transcript_content_changed {
        frame.render_widget(Clear, scroll_area);
        session.transcript_content_changed = false;
    }
    frame.render_widget(paragraph, scroll_area);
}

fn header_reserved_rows(session: &Session) -> u16 {
    session.header_rows.max(ui::INLINE_HEADER_HEIGHT)
}

fn input_reserved_rows(session: &Session) -> u16 {
    header_reserved_rows(session) + session.input_height
}

pub fn recalculate_transcript_rows(session: &mut Session) {
    let reserved = input_reserved_rows(session).saturating_add(2); // account for transcript block borders
    let available = session.view_rows.saturating_sub(reserved).max(1);
    apply_transcript_rows(session, available);
}

/// Generic palette rendering helper to avoid duplication
struct PaletteRenderParams<F>
where
    F: for<'a> Fn(&Session, &'a str, bool) -> ListItem<'static>,
{
    is_active: bool,
    title: String,
    items: Vec<(usize, String, bool)>, // (index, display_text, is_selected)
    instructions: Vec<Line<'static>>,
    has_more: bool,
    more_text: String,
    render_item: F,
}

fn render_palette_generic<F>(
    session: &mut Session,
    frame: &mut Frame<'_>,
    viewport: Rect,
    params: PaletteRenderParams<F>,
) where
    F: for<'a> Fn(&Session, &'a str, bool) -> ListItem<'static>,
{
    if !params.is_active || viewport.height == 0 || viewport.width == 0 || session.modal.is_some() {
        return;
    }

    if params.items.is_empty() {
        return;
    }

    // Calculate width hint
    let mut width_hint = 40u16;
    for (_, display_text, _) in &params.items {
        width_hint = width_hint.max(measure_text_width(display_text) + 4);
    }

    let modal_height =
        params.items.len() + params.instructions.len() + 2 + if params.has_more { 1 } else { 0 };
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(params.title)
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let layout = ModalListLayout::new(inner, params.instructions.len());
    if let Some(text_area) = layout.text_area {
        let paragraph = Paragraph::new(params.instructions).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, text_area);
    }

    let mut list_items: Vec<ListItem> = params
        .items
        .iter()
        .map(|(_, display_text, is_selected)| {
            (params.render_item)(session, display_text.as_str(), *is_selected)
        })
        .collect();

    if params.has_more {
        let continuation_style =
            default_style(session).add_modifier(Modifier::DIM | Modifier::ITALIC);
        list_items.push(ListItem::new(Line::from(Span::styled(
            params.more_text,
            continuation_style,
        ))));
    }

    let list = List::new(list_items)
        .style(default_style(session))
        .highlight_symbol(ui::MODAL_LIST_HIGHLIGHT_FULL)
        .repeat_highlight_symbol(true);
    frame.render_widget(list, layout.list_area);
}

pub fn render_config_palette(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if !session.config_palette_active {
        return;
    }

    // Extract styles first to avoid borrow checker issues with session
    let modal_hl_style = modal_list_highlight_style(session);
    let def_style = default_style(session);
    let acc_style = accent_style(session);
    let b_style = border_style(session);
    let b_type = terminal_capabilities::get_border_type();

    let Some(palette) = session.config_palette.as_mut() else {
        return;
    };

    let items: Vec<ListItem> = palette
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = palette.list_state.selected() == Some(i);
            let value_str = match &item.kind {
                ConfigItemKind::Bool { value } => if *value { "[X]" } else { "[ ]" }.to_string(),
                ConfigItemKind::Enum { value, .. } => format!("< {} >", value),
                ConfigItemKind::Number { value, .. } => value.to_string(),
                ConfigItemKind::Display { value } => value.clone(),
            };

            let style = if is_selected {
                modal_hl_style
            } else {
                def_style
            };

            let label_span = Span::styled(format!("{:<30}", item.label), style);
            let value_span = Span::styled(value_str, if is_selected { style } else { acc_style });

            let mut label_line = vec![label_span, Span::raw(" "), value_span];
            if is_selected {
                label_line.insert(0, Span::raw("> "));
            } else {
                label_line.insert(0, Span::raw("  "));
            }

            let mut lines = vec![Line::from(label_line)];
            if let Some(desc) = &item.description {
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled(desc.to_owned(), def_style.add_modifier(Modifier::DIM)),
                ]));
            }

            ListItem::new(lines)
        })
        .collect();

    let width_hint = 70u16;
    let max_height = viewport.height.saturating_sub(4) as usize;
    let modal_height = (items.len() * 2 + 2).min(max_height);
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);

    let title = "Configuration".to_string();

    let instructions = " ↑↓ Navigate · Space/Enter Toggle · ←→ Adjust · Esc Close/Save ";
    let block = Block::bordered()
        .title(title)
        .title_bottom(Span::styled(
            instructions,
            def_style.add_modifier(Modifier::DIM),
        ))
        .border_type(b_type)
        .style(def_style)
        .border_style(b_style);

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let list = List::new(items).highlight_style(modal_hl_style);

    frame.render_stateful_widget(list, inner, &mut palette.list_state);
}

pub fn render_file_palette(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if !session.file_palette_active {
        return;
    }

    let Some(palette) = session.file_palette.as_ref() else {
        return;
    };

    if viewport.height == 0 || viewport.width == 0 || session.modal.is_some() {
        return;
    }

    // Show loading state if no files loaded yet
    if !palette.has_files() {
        render_file_palette_loading(session, frame, viewport);
        return;
    }

    let items = palette.current_page_items();
    if items.is_empty() && palette.filter_query().is_empty() {
        return;
    }

    // Convert items to generic format
    let generic_items: Vec<(usize, String, bool)> = items
        .iter()
        .map(|(idx, entry, selected)| {
            let display = if entry.is_dir {
                format!("{}/ ", entry.display_name)
            } else {
                entry.display_name.clone()
            };
            (*idx, display, *selected)
        })
        .collect();

    let title = format!(
        "File Browser (Page {}/{})",
        palette.current_page_number(),
        palette.total_pages()
    );

    let instructions = file_palette_instructions(session, palette);
    let has_more = palette.has_more_items();
    let more_text = format!(
        "  ... ({} more items)",
        palette
            .total_items()
            .saturating_sub(palette.current_page_number() * 20)
    );

    // Render using generic helper
    render_palette_generic(
        session,
        frame,
        viewport,
        PaletteRenderParams {
            is_active: true, // is_active already checked above
            title,
            items: generic_items,
            instructions,
            has_more,
            more_text,
            render_item: |session, display_text: &str, is_selected| {
                let base_style = if is_selected {
                    modal_list_highlight_style(session)
                } else {
                    default_style(session)
                };

                // Apply file-specific styling
                let mut style = base_style;

                // Add icon prefix based on file type
                let (prefix, is_dir) = if display_text.ends_with("/ ") {
                    ("↳  ", true)
                } else {
                    ("  · ", false)
                };

                if is_dir {
                    style = style.add_modifier(Modifier::BOLD);
                }

                let display = format!("{}{}", prefix, display_text.trim_end_matches("/ "));
                ListItem::new(Line::from(display).style(style))
            },
        },
    );
}

fn render_file_palette_loading(session: &Session, frame: &mut Frame<'_>, viewport: Rect) {
    let width_hint = 40u16;
    let modal_height = 3;
    let area = compute_modal_area(viewport, width_hint, modal_height, 0, 0, true);

    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title("File Browser")
        .border_type(terminal_capabilities::get_border_type())
        .style(default_style(session))
        .border_style(border_style(session));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.height > 0 && inner.width > 0 {
        let loading_text = vec![Line::from(Span::styled(
            "Loading workspace files...".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        ))];
        let paragraph = Paragraph::new(loading_text).wrap(Wrap { trim: true });
        frame.render_widget(paragraph, inner);
    }
}

fn file_palette_instructions(session: &Session, palette: &FilePalette) -> Vec<Line<'static>> {
    let mut lines = vec![];

    if palette.is_empty() {
        lines.push(Line::from(Span::styled(
            "No files found matching filter".to_owned(),
            default_style(session).add_modifier(Modifier::DIM),
        )));
    } else {
        let total = palette.total_items();
        let count_text = if total == 1 {
            "1 file".to_owned()
        } else {
            format!("{} files", total)
        };

        let nav_text = "↑↓ Navigate · PgUp/PgDn Page · Tab/Enter Select";

        lines.push(Line::from(vec![Span::styled(
            format!("{} · Esc Close", nav_text),
            default_style(session),
        )]));

        lines.push(Line::from(vec![
            Span::styled(
                format!("Showing {}", count_text),
                default_style(session).add_modifier(Modifier::DIM),
            ),
            Span::styled(
                if !palette.filter_query().is_empty() {
                    format!(" matching '{}'", palette.filter_query())
                } else {
                    String::new()
                },
                accent_style(session),
            ),
        ]));
    }

    lines
}

fn has_input_status(session: &Session) -> bool {
    let left_present = session
        .input_status_left
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    if left_present {
        return true;
    }
    session
        .input_status_right
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
}

pub(super) fn render_message_spans(session: &Session, index: usize) -> Vec<Span<'static>> {
    let Some(line) = session.lines.get(index) else {
        return vec![Span::raw(String::new())];
    };
    let mut spans = Vec::new();
    if line.kind == InlineMessageKind::Agent {
        spans.extend(agent_prefix_spans(session, line));
    } else if let Some(prefix) = prefix_text(session, line.kind) {
        let style = prefix_style(session, line);
        spans.push(Span::styled(
            prefix,
            ratatui_style_from_inline(&style, session.theme.foreground),
        ));
    }

    if line.kind == InlineMessageKind::Agent {
        spans.push(Span::raw(ui::INLINE_AGENT_MESSAGE_LEFT_PADDING));
    }

    if line.segments.is_empty() {
        if spans.is_empty() {
            spans.push(Span::raw(String::new()));
        }
        return spans;
    }

    if line.kind == InlineMessageKind::Tool {
        let tool_spans = render_tool_segments(session, line);
        if tool_spans.is_empty() {
            spans.push(Span::raw(String::new()));
        } else {
            spans.extend(tool_spans);
        }
        return spans;
    }

    if line.kind == InlineMessageKind::Pty {
        // Render PTY content directly without header decoration
        let fallback = text_fallback(session, line.kind).or(session.theme.foreground);
        for segment in &line.segments {
            let style = ratatui_style_from_inline(&segment.style, fallback);
            spans.push(Span::styled(segment.text.clone(), style));
        }
        if !spans.is_empty() {
            return spans;
        }
    }

    let fallback = text_fallback(session, line.kind).or(session.theme.foreground);
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, fallback);
        spans.push(Span::styled(segment.text.clone(), style));
    }

    if spans.is_empty() {
        spans.push(Span::raw(String::new()));
    }

    spans
}

pub(super) fn agent_prefix_spans(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let prefix_style_inline = prefix_style(session, line);
    let prefix_style_ratatui =
        ratatui_style_from_inline(&prefix_style_inline, session.theme.foreground);
    let has_label = session
        .labels
        .agent
        .as_ref()
        .is_some_and(|label| !label.is_empty());
    let prefix_has_trailing_space = ui::INLINE_AGENT_QUOTE_PREFIX
        .chars()
        .last()
        .is_some_and(|ch| ch.is_whitespace());
    if !ui::INLINE_AGENT_QUOTE_PREFIX.is_empty() {
        spans.push(Span::styled(
            ui::INLINE_AGENT_QUOTE_PREFIX.to_owned(),
            prefix_style_ratatui,
        ));
        if has_label && !prefix_has_trailing_space {
            spans.push(Span::styled(" ".to_owned(), prefix_style_ratatui));
        }
    }

    if let Some(label) = &session.labels.agent
        && !label.is_empty()
    {
        let label_style = ratatui_style_from_inline(&prefix_style_inline, session.theme.foreground);
        spans.push(Span::styled(label.clone(), label_style));
    }

    spans
}

/// Strips ANSI escape codes from text to ensure plain text output
pub(super) fn strip_ansi_codes(text: &str) -> std::borrow::Cow<'_, str> {
    text_utils::strip_ansi_codes(text)
}

pub(super) fn render_tool_segments(session: &Session, line: &MessageLine) -> Vec<Span<'static>> {
    // Render tool output without header decorations - just display segments directly
    let mut spans = Vec::new();
    for segment in &line.segments {
        let style = ratatui_style_from_inline(&segment.style, session.theme.foreground);
        spans.push(Span::styled(segment.text.clone(), style));
    }
    spans
}

/// Simplify tool call display text for better human readability
#[allow(dead_code)]
fn simplify_tool_display(text: &str) -> String {
    text_utils::simplify_tool_display(text)
}

#[allow(dead_code)]
fn tool_inline_style(session: &Session, tool_name: &str) -> InlineTextStyle {
    session.styles.tool_inline_style(tool_name)
}

fn tool_border_style(session: &Session) -> InlineTextStyle {
    session.styles.tool_border_style()
}

fn default_style(session: &Session) -> Style {
    session.styles.default_style()
}

#[allow(dead_code)]
fn accent_inline_style(session: &Session) -> InlineTextStyle {
    session.styles.accent_inline_style()
}

fn accent_style(session: &Session) -> Style {
    session.styles.accent_style()
}

#[allow(dead_code)]
fn border_inline_style(session: &Session) -> InlineTextStyle {
    session.styles.border_inline_style()
}

fn border_style(session: &Session) -> Style {
    session.styles.border_style()
}

fn prefix_text(session: &Session, kind: InlineMessageKind) -> Option<String> {
    match kind {
        InlineMessageKind::User => Some(
            session
                .labels
                .user
                .clone()
                .unwrap_or_else(|| USER_PREFIX.to_owned()),
        ),
        InlineMessageKind::Agent => None,
        InlineMessageKind::Policy => session.labels.agent.clone(),
        InlineMessageKind::Tool | InlineMessageKind::Pty | InlineMessageKind::Error => None,
        InlineMessageKind::Info | InlineMessageKind::Warning => None,
    }
}

fn prefix_style(session: &Session, line: &MessageLine) -> InlineTextStyle {
    session.styles.prefix_style(line)
}

fn text_fallback(session: &Session, kind: InlineMessageKind) -> Option<AnsiColorEnum> {
    session.styles.text_fallback(kind)
}

fn viewport_height(session: &Session) -> usize {
    session.viewport_height()
}

pub(super) fn invalidate_scroll_metrics(session: &mut Session) {
    session.invalidate_scroll_metrics();
}

fn wrap_block_lines(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
) -> Vec<Line<'static>> {
    wrap_block_lines_with_options(
        session,
        first_prefix,
        continuation_prefix,
        content,
        max_width,
        border_style,
        true,
    )
}

fn wrap_block_lines_no_right_border(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
) -> Vec<Line<'static>> {
    wrap_block_lines_with_options(
        session,
        first_prefix,
        continuation_prefix,
        content,
        max_width,
        border_style,
        false,
    )
}

fn wrap_block_lines_with_options(
    session: &Session,
    first_prefix: &str,
    continuation_prefix: &str,
    content: Vec<Span<'static>>,
    max_width: usize,
    border_style: Style,
    show_right_border: bool,
) -> Vec<Line<'static>> {
    if max_width < 2 {
        let fallback = if show_right_border {
            format!("{}││", first_prefix)
        } else {
            format!("{}│", first_prefix)
        };
        return vec![Line::from(fallback).style(border_style)];
    }

    let right_border = if show_right_border {
        ui::INLINE_BLOCK_BODY_RIGHT
    } else {
        ""
    };
    let first_prefix_width = first_prefix.chars().count();
    let continuation_prefix_width = continuation_prefix.chars().count();
    let prefix_width = first_prefix_width.max(continuation_prefix_width);
    let border_width = right_border.chars().count();
    let consumed_width = prefix_width.saturating_add(border_width);
    let content_width = max_width.saturating_sub(consumed_width);

    if max_width == usize::MAX {
        let mut spans = vec![Span::styled(first_prefix.to_owned(), border_style)];
        spans.extend(content);
        if show_right_border {
            spans.push(Span::styled(right_border.to_owned(), border_style));
        }
        return vec![Line::from(spans)];
    }

    let mut wrapped = wrap_line(session, Line::from(content), content_width);
    if wrapped.is_empty() {
        wrapped.push(Line::default());
    }

    // Add borders to each wrapped line
    for (idx, line) in wrapped.iter_mut().enumerate() {
        let line_width = line.spans.iter().map(|s| s.width()).sum::<usize>();
        let padding = if show_right_border {
            content_width.saturating_sub(line_width)
        } else {
            0
        };

        let active_prefix = if idx == 0 {
            first_prefix
        } else {
            continuation_prefix
        };
        let mut new_spans = vec![Span::styled(active_prefix.to_owned(), border_style)];
        new_spans.append(&mut line.spans);
        if padding > 0 {
            new_spans.push(Span::styled(" ".repeat(padding), Style::default()));
        }
        if show_right_border {
            new_spans.push(Span::styled(right_border.to_owned(), border_style));
        }
        line.spans = new_spans;
    }

    wrapped
}

fn pty_block_has_content(session: &Session, index: usize) -> bool {
    if session.lines.is_empty() {
        return false;
    }

    let mut start = index;
    while start > 0 {
        let Some(previous) = session.lines.get(start - 1) else {
            break;
        };
        if previous.kind != InlineMessageKind::Pty {
            break;
        }
        start -= 1;
    }

    let mut end = index;
    while end + 1 < session.lines.len() {
        let Some(next) = session.lines.get(end + 1) else {
            break;
        };
        if next.kind != InlineMessageKind::Pty {
            break;
        }
        end += 1;
    }

    if start > end || end >= session.lines.len() {
        tracing::warn!(
            "invalid range: start={}, end={}, len={}",
            start,
            end,
            session.lines.len()
        );
        return false;
    }

    for line in &session.lines[start..=end] {
        if line
            .segments
            .iter()
            .any(|segment| !segment.text.trim().is_empty())
        {
            return true;
        }
    }

    false
}

fn reflow_pty_lines(session: &Session, index: usize, width: u16) -> Vec<Line<'static>> {
    let Some(line) = session.lines.get(index) else {
        return vec![Line::default()];
    };

    let max_width = if width == 0 {
        usize::MAX
    } else {
        width as usize
    };

    if !pty_block_has_content(session, index) {
        return Vec::new();
    }

    let mut border_style = ratatui_style_from_inline(
        &session.styles.tool_border_style(),
        session.theme.foreground,
    );
    border_style = border_style.add_modifier(Modifier::DIM);

    let prev_is_pty = index
        .checked_sub(1)
        .and_then(|prev| session.lines.get(prev))
        .map(|prev| prev.kind == InlineMessageKind::Pty)
        .unwrap_or(false);

    let is_start = !prev_is_pty;

    let mut lines = Vec::new();

    let mut combined = String::new();
    for segment in &line.segments {
        combined.push_str(segment.text.as_str());
    }
    if is_start && combined.trim().is_empty() {
        return Vec::new();
    }

    // Render body content - strip ANSI codes to ensure plain text output
    let fallback = text_fallback(session, InlineMessageKind::Pty).or(session.theme.foreground);
    let mut body_spans = Vec::new();
    for segment in &line.segments {
        let stripped_text = strip_ansi_codes(&segment.text);
        let mut style = ratatui_style_from_inline(&segment.style, fallback);
        style = style.add_modifier(Modifier::DIM);
        body_spans.push(Span::styled(stripped_text.into_owned(), style));
    }

    // Check if this is a thinking spinner line (skip border rendering)
    let is_thinking_spinner = combined.contains("Thinking...");

    if is_start {
        // Render body without borders - just indent with spaces for visual separation
        if is_thinking_spinner {
            // Render thinking spinner without borders
            lines.extend(wrap_block_lines_no_right_border(
                session,
                "",
                "",
                body_spans,
                max_width,
                border_style,
            ));
        } else {
            let body_prefix = "  ";
            let continuation_prefix =
                text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
            lines.extend(wrap_block_lines_no_right_border(
                session,
                body_prefix,
                continuation_prefix.as_str(),
                body_spans,
                max_width,
                border_style,
            ));
        }
    } else {
        let body_prefix = "  ";
        let continuation_prefix =
            text_utils::pty_wrapped_continuation_prefix(body_prefix, combined.as_str());
        lines.extend(wrap_block_lines_no_right_border(
            session,
            body_prefix,
            continuation_prefix.as_str(),
            body_spans,
            max_width,
            border_style,
        ));
    }

    if lines.is_empty() {
        lines.push(Line::default());
    }

    lines
}

fn message_divider_line(session: &Session, width: usize, kind: InlineMessageKind) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }

    let content = ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(width);
    let style = message_divider_style(session, kind);
    Line::from(content).style(style)
}

fn message_divider_style(session: &Session, kind: InlineMessageKind) -> Style {
    session.styles.message_divider_style(kind)
}

fn wrap_line(_session: &Session, line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    text_utils::wrap_line(line, max_width)
}

fn justify_wrapped_lines(
    session: &Session,
    lines: Vec<Line<'static>>,
    max_width: usize,
    kind: InlineMessageKind,
) -> Vec<Line<'static>> {
    if max_width == 0 {
        return lines;
    }

    let total = lines.len();
    let mut justified = Vec::with_capacity(total);
    let mut in_fenced_block = false;
    for (index, line) in lines.into_iter().enumerate() {
        let is_last = index + 1 == total;
        let mut next_in_fenced_block = in_fenced_block;
        let is_fence_line = {
            let line_text_storage: std::borrow::Cow<'_, str> = if line.spans.len() == 1 {
                std::borrow::Cow::Borrowed(&*line.spans[0].content)
            } else {
                std::borrow::Cow::Owned(
                    line.spans
                        .iter()
                        .map(|span| &*span.content)
                        .collect::<String>(),
                )
            };
            let line_text: &str = &*line_text_storage;
            let trimmed_start = line_text.trim_start();
            trimmed_start.starts_with("```") || trimmed_start.starts_with("~~~")
        };
        if is_fence_line {
            next_in_fenced_block = !in_fenced_block;
        }

        // Extend diff line backgrounds to full width
        let processed_line = if is_diff_line(session, &line) {
            pad_diff_line(session, &line, max_width)
        } else if kind == InlineMessageKind::Agent
            && !in_fenced_block
            && !is_fence_line
            && should_justify_message_line(session, &line, max_width, is_last)
        {
            justify_message_line(session, &line, max_width)
        } else {
            line
        };

        justified.push(processed_line);
        in_fenced_block = next_in_fenced_block;
    }

    justified
}

fn should_justify_message_line(
    _session: &Session,
    line: &Line<'static>,
    max_width: usize,
    is_last: bool,
) -> bool {
    if is_last || max_width == 0 {
        return false;
    }
    if line.spans.len() != 1 {
        return false;
    }
    let text: &str = &*line.spans[0].content;
    if text.trim().is_empty() {
        return false;
    }
    if text.starts_with(char::is_whitespace) {
        return false;
    }
    let trimmed = text.trim();
    if trimmed.starts_with(|ch: char| ['-', '*', '`', '>', '#'].contains(&ch)) {
        return false;
    }
    if trimmed.contains("```") {
        return false;
    }
    let width = UnicodeWidthStr::width(trimmed);
    if width >= max_width || width < max_width / 2 {
        return false;
    }

    justify_plain_text(text, max_width).is_some()
}

fn justify_message_line(
    _session: &Session,
    line: &Line<'static>,
    max_width: usize,
) -> Line<'static> {
    let span = &line.spans[0];
    if let Some(justified) = justify_plain_text(&*span.content, max_width) {
        Line::from(justified).style(span.style)
    } else {
        line.clone()
    }
}

fn is_diff_line(_session: &Session, line: &Line<'static>) -> bool {
    // Detect actual diff lines: must start with +, -, or space (diff markers)
    // AND have background color styling applied (from git diff coloring)
    // This avoids false positives from regular text that happens to start with these chars
    if line.spans.is_empty() {
        return false;
    }

    // Check if any span has background color (diff lines from render have colored backgrounds)
    let has_bg_color = line.spans.iter().any(|span| span.style.bg.is_some());
    if !has_bg_color {
        return false;
    }

    // Must start with a diff marker character in the first span
    let first_span_char = line.spans[0].content.chars().next();
    matches!(first_span_char, Some('+') | Some('-') | Some(' '))
}

fn pad_diff_line(_session: &Session, line: &Line<'static>, max_width: usize) -> Line<'static> {
    if max_width == 0 || line.spans.is_empty() {
        return line.clone();
    }

    // Calculate actual display width using Unicode width rules
    let line_width: usize = line
        .spans
        .iter()
        .map(|s| {
            s.content
                .chars()
                .map(|ch| unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1))
                .sum::<usize>()
        })
        .sum();

    let padding_needed = max_width.saturating_sub(line_width);

    if padding_needed == 0 {
        return line.clone();
    }

    let padding_style = line
        .spans
        .iter()
        .find_map(|span| span.style.bg)
        .map(|bg| Style::default().bg(bg))
        .unwrap_or_default();

    let mut new_spans = Vec::with_capacity(line.spans.len() + 1);
    new_spans.extend(line.spans.iter().cloned());
    new_spans.push(Span::styled(" ".repeat(padding_needed), padding_style));

    Line::from(new_spans)
}

fn prepare_transcript_scroll(
    session: &mut Session,
    total_rows: usize,
    viewport_rows: usize,
) -> (usize, usize) {
    let viewport = viewport_rows.max(1);
    let clamped_total = total_rows.max(1);
    session.scroll_manager.set_total_rows(clamped_total);
    session.scroll_manager.set_viewport_rows(viewport as u16);
    let max_offset = session.scroll_manager.max_offset();

    if session.scroll_manager.offset() > max_offset {
        session.scroll_manager.set_offset(max_offset);
    }

    let top_offset = max_offset.saturating_sub(session.scroll_manager.offset());
    (top_offset, clamped_total)
}

// Delegate to text_utils module
fn justify_plain_text(text: &str, max_width: usize) -> Option<String> {
    text_utils::justify_plain_text(text, max_width)
}

pub fn render_modal(session: &mut Session, frame: &mut Frame<'_>, viewport: Rect) {
    if viewport.width == 0 || viewport.height == 0 {
        return;
    }

    // Auto-approve modals when skip_confirmations is set (for tests and headless mode)
    if session.skip_confirmations {
        if let Some(mut modal) = session.modal.take() {
            if let Some(list) = &mut modal.list {
                if let Some(_selection) = list.current_selection() {
                    // Note: We can't easily emit an event from here without access to the sender.
                    // Instead, we just clear the modal and assume the tool execution logic
                    // or whatever triggered the modal will check skip_confirmations as well.
                    // This is handled in ensure_tool_permission.
                }
            }
            session.input_enabled = modal.restore_input;
            session.cursor_visible = modal.restore_cursor;
            session.needs_full_clear = true;
            session.needs_redraw = true;
            return;
        }
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
