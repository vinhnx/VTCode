use super::*;
use crate::tui::config::constants::ui;
use crate::tui::core_tui::session::list_panel::input_styles_from_theme;
use crate::tui::core_tui::session::transcript_links::decorate_detected_link_lines;
use crate::tui::core_tui::style::ratatui_color_from_ansi;
use crate::tui::core_tui::types::InlineMessageKind;
use crate::tui::ui::tui::session::modal::{
    ModalBodyContext, ModalListState, ModalRenderStyles, render_modal_body, render_wizard_modal_body,
};
use anstyle::{Ansi256Color, Color as AnsiColorEnum};
use ratatui::widgets::{Block, Clear, Fill, Paragraph, Wrap};
use tracing::warn;

const MAX_INLINE_MODAL_HEIGHT: u16 = 20;
const MAX_INLINE_MODAL_HEIGHT_MULTILINE: u16 = 32;
/// Instruction viewport rows shared with `render_modal_body`: the plan
/// approval header (prompt + full-text summary + steps + overflow) budgets
/// up to eight wrapped visual rows so wrapped summaries stay visible.
const MAX_INLINE_INSTRUCTION_ROWS: usize = 8;
const MODAL_TITLE_CHROME_ROWS: usize = 2;

fn modal_base_style(session: &Session) -> Style {
    session.styles.default_style()
}

fn modal_heading_style(session: &Session) -> Style {
    modal_base_style(session)
        .fg(ratatui_color_from_ansi(resolve_modal_chrome_ansi_color(session)))
        .add_modifier(Modifier::BOLD)
}

/// Estimate wrapped instruction rows with the same greedy word-wrap the
/// modal body uses, so the claimed modal height hugs the painted content
/// instead of leaving a blank gap (raw line count) or clipping wrapped
/// summaries (under-count). `## ` section headers reserve their blank
/// separator row, matching `modal_instruction_lines`.
fn estimated_modal_instruction_rows(lines: &[String], content_width: usize) -> usize {
    if lines.iter().all(|line| line.trim().is_empty()) {
        return 1;
    }
    let width = content_width.max(1);
    let mut rows = 0usize;
    let mut first_content_seen = false;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            rows = rows.saturating_add(1);
            continue;
        }
        if trimmed.strip_prefix("## ").is_some() {
            if first_content_seen {
                rows = rows.saturating_add(1);
            }
            first_content_seen = true;
            rows = rows.saturating_add(1);
            continue;
        }
        first_content_seen = true;
        let mut current_width = 0usize;
        let mut line_rows = 1usize;
        for word in trimmed.split_whitespace() {
            let word_width = unicode_width::UnicodeWidthStr::width(word);
            if current_width == 0 {
                current_width = word_width;
            } else if current_width.saturating_add(1).saturating_add(word_width) > width {
                line_rows = line_rows.saturating_add(1);
                current_width = word_width;
            } else {
                current_width = current_width.saturating_add(1).saturating_add(word_width);
            }
        }
        rows = rows.saturating_add(line_rows);
    }
    rows.clamp(1, MAX_INLINE_INSTRUCTION_ROWS)
}

fn list_has_multiline_items(list: &ModalListState) -> bool {
    list.visible_indices.iter().any(|&index| {
        list.items
            .get(index)
            .is_some_and(|item| item.subtitle.as_ref().is_some_and(|subtitle| !subtitle.trim().is_empty()))
    })
}

fn list_row_cap(list: &ModalListState) -> usize {
    if list_has_multiline_items(list) {
        ui::INLINE_LIST_MAX_ROWS_MULTILINE
    } else {
        ui::INLINE_LIST_MAX_ROWS
    }
}

fn list_desired_rows(list: &ModalListState) -> usize {
    // Count rendered rows, not items: each title costs one row, each subtitle
    // costs a second row, headers reserve a blank separator above, and every
    // selectable row reserves a trailing blank. Capped so large
    // pickers still scroll instead of claiming the full viewport.
    let mut rows = 0usize;
    for (visible_index, &item_index) in list.visible_indices.iter().enumerate() {
        let Some(item) = list.items.get(item_index) else {
            continue;
        };
        if item.is_divider {
            rows = rows.saturating_add(1);
            continue;
        }
        if item.is_header() && visible_index > 0 {
            rows = rows.saturating_add(1);
        }
        rows = rows.saturating_add(1);
        if item.subtitle.as_ref().is_some_and(|subtitle| !subtitle.trim().is_empty()) {
            rows = rows.saturating_add(1);
        }
        if item.selection.is_some() {
            rows = rows.saturating_add(1);
        }
    }
    rows.clamp(1, list_row_cap(list))
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

fn resolve_modal_chrome_ansi_color(session: &Session) -> AnsiColorEnum {
    session
        .theme
        .tool_accent
        .or(session.theme.primary)
        .or(session.theme.secondary)
        .unwrap_or(AnsiColorEnum::Ansi256(Ansi256Color(ui::SAFE_ANSI_BRIGHT_CYAN)))
}

fn modal_chrome_style(session: &Session) -> Style {
    modal_heading_style(session)
}

fn render_modal_background(frame: &mut Frame<'_>, area: Rect, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().style(style), area);
}

fn render_modal_divider(frame: &mut Frame<'_>, area: Rect, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Fill::new(ui::INLINE_BLOCK_HORIZONTAL).style(style), area);
}

fn wizard_step_has_inline_custom_editor(wizard: &crate::tui::ui::tui::session::modal::WizardModalState) -> bool {
    // Single predicate shared with render and hit-testing: the editor is
    // visible exactly when the selected item is an unanswered custom-note
    // answer. See `modal::inline_editor_for_step`.
    let Some(step) = wizard.steps.get(wizard.current_step) else {
        return false;
    };
    crate::tui::core_tui::session::modal::inline_editor_for_step(step).is_some()
}

pub fn split_inline_modal_area(session: &Session, area: Rect) -> (Rect, Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return (area, None);
    }

    let title_chrome_rows = if modal_has_title(session) {
        MODAL_TITLE_CHROME_ROWS as u16
    } else {
        0
    };
    let multiline_list_present = if let Some(wizard) = session.wizard_overlay() {
        wizard
            .steps
            .get(wizard.current_step)
            .is_some_and(|step| list_has_multiline_items(&step.list))
    } else if let Some(modal) = session.modal_state() {
        modal.list.as_ref().is_some_and(list_has_multiline_items)
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
        if wizard.search.is_some() {
            lines = lines.saturating_add(1); // divider before list
        }
        let (list_rows, summary_rows) = wizard
            .steps
            .get(wizard.current_step)
            .map(|step| (list_desired_rows(&step.list), step.list.summary_line_rows(None, false)))
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
        lines = lines.saturating_add(wizard.instruction_lines().len().min(MAX_INLINE_INSTRUCTION_ROWS));
        if title_chrome_rows > 0 {
            lines = lines.saturating_add(1 + usize::from(title_chrome_rows)); // title row + dividers
        }
        lines
    } else if let Some(modal) = session.modal_state() {
        // Size instructions by wrapped visual rows (not raw line count) so
        // the modal hugs the painted header: no blank gap when lines are
        // short, no clipped summary when they wrap. Matches the clamping in
        // `render_modal_body`.
        let content_width = area.width.saturating_sub(2) as usize;
        let mut lines = estimated_modal_instruction_rows(&modal.lines, content_width);
        if let Some(search) = modal.search.as_ref() {
            // Match `render_modal_body`: prompt-only search costs 1 row, a
            // titled search field costs 2.
            lines = lines.saturating_add(if search.label.is_empty() { 1 } else { 2 });
        }
        if modal.secure_prompt.is_some() {
            lines = lines.saturating_add(2);
        }
        if modal.list.is_some() {
            // Match `render_modal_body`: the instructions→list divider renders
            // whenever a list is present, with or without a search field.
            lines = lines.saturating_add(1); // divider before list
        }
        if let Some(list) = modal.list.as_ref() {
            lines = lines.saturating_add(list_desired_rows(list));
            lines = lines.saturating_add(list.summary_line_rows(modal.footer_hint.as_deref(), modal.status.is_some()));
        } else {
            lines = lines.saturating_add(1);
        }
        if title_chrome_rows > 0 {
            lines = lines.saturating_add(1 + usize::from(title_chrome_rows)); // title row + dividers
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
    }
    .saturating_add(title_chrome_rows);
    let capped_max = modal_height_cap.min(max_panel_height).max(min_height);
    let desired_height = (desired_lines.min(u16::MAX as usize) as u16).max(min_height).min(capped_max);

    let [transcript_area, modal_area] = area
        .try_layout(&Layout::vertical([Constraint::Min(1), Constraint::Length(desired_height)]))
        // Unreachable: `desired_height <= max_panel_height = height - 1`, so
        // `Min(1) + Length(desired)` always fits. Fail closed (empty modal)
        // instead of double-claiming `area` for both halves.
        .unwrap_or([area, Rect::ZERO]);
    (transcript_area, Some(modal_area))
}

pub(crate) fn floating_modal_area(area: Rect) -> Rect {
    if area.width == 0 || area.height == 0 {
        return area;
    }

    let height = (area.height / 2).max(1);
    let y = area.y.saturating_add(area.height.saturating_sub(height));
    Rect::new(area.x, y, area.width, height)
}

pub(crate) fn clip_transcript_area(transcript_area: Rect, modal_area: Rect) -> Rect {
    if transcript_area.width == 0 || transcript_area.height == 0 || modal_area.width == 0 || modal_area.height == 0 {
        return transcript_area;
    }

    let transcript_right = transcript_area.x.saturating_add(transcript_area.width);
    let transcript_bottom = transcript_area.y.saturating_add(transcript_area.height);
    let modal_right = modal_area.x.saturating_add(modal_area.width);
    let modal_bottom = modal_area.y.saturating_add(modal_area.height);
    let overlaps_horizontally = transcript_area.x < modal_right && modal_area.x < transcript_right;
    let overlaps_vertically = transcript_area.y < modal_bottom && modal_area.y < transcript_bottom;

    if !overlaps_horizontally || !overlaps_vertically {
        return transcript_area;
    }

    let clipped_height = modal_area.y.saturating_sub(transcript_area.y).min(transcript_area.height);
    Rect::new(transcript_area.x, transcript_area.y, transcript_area.width, clipped_height)
}

pub fn render_modal(session: &mut Session, frame: &mut Frame<'_>, area: Rect) {
    if !session.has_active_overlay() || area.width == 0 || area.height == 0 {
        session.set_modal_list_area(None);
        session.set_modal_text_areas(Vec::new());
        session.set_modal_link_targets(Vec::new());
        return;
    }

    let styles = modal_render_styles(session);
    let input_styles = input_styles_from_theme(&session.theme);
    session.occlude_progress_feedback(area);
    render_modal_background(frame, area, styles.background);
    let link_style = session.styles.transcript_link_style().add_modifier(Modifier::UNDERLINED);
    let hovered_link_style = link_style.add_modifier(Modifier::BOLD);
    let workspace_root = session.workspace_root.clone();
    let last_mouse_position = session.last_mouse_position;
    let title = modal_title_text(session).trim().to_owned();
    let mut title_link_targets = Vec::new();
    let (body_area, title_area) = if title.is_empty() {
        (area, None)
    } else {
        let [title_area, top_divider_area, body_area, bottom_divider_area] = area
            .try_layout(&Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(0),
                Constraint::Length(1),
            ]))
            .unwrap_or_else(|_| {
                warn!(target: "vtcode::tui", height = area.height, "modal layout fallback to zero rects");
                [Rect::ZERO; 4]
            });
        let title_line = Line::from(Span::styled(title, styles.title));
        let (decorated_title, link_targets) = decorate_detected_link_lines(
            vec![title_line],
            title_area,
            workspace_root.as_deref(),
            last_mouse_position,
            link_style,
            hovered_link_style,
        );
        title_link_targets = link_targets;
        render_modal_background(frame, title_area, styles.background);
        frame.render_widget(Paragraph::new(decorated_title).style(styles.title).wrap(Wrap { trim: true }), title_area);
        render_modal_divider(frame, top_divider_area, styles.border);
        render_modal_divider(frame, bottom_divider_area, styles.border);
        (body_area, Some(title_area))
    };

    if let Some(wizard) = session.wizard_overlay_mut() {
        render_modal_background(frame, body_area, styles.background);
        if body_area.width == 0 || body_area.height == 0 {
            session.set_modal_list_area(None);
            session.set_modal_text_areas(Vec::new());
            session.set_modal_link_targets(Vec::new());
            return;
        }
        let mut outcome = render_wizard_modal_body(
            frame,
            body_area,
            wizard,
            &styles,
            &input_styles,
            workspace_root.as_deref(),
            last_mouse_position,
            link_style,
            hovered_link_style,
        );
        if let Some(title_area) = title_area {
            outcome.push_text_area(title_area);
            outcome.link_targets.extend(title_link_targets.clone());
        }
        session.set_modal_list_area(outcome.list_area);
        session.set_modal_text_areas(outcome.text_areas);
        session.set_modal_link_targets(outcome.link_targets);
        return;
    }

    let input = session.input_manager.content().to_owned();
    let cursor = session.input_manager.cursor();
    let Some(modal) = session.modal_state_mut() else {
        session.set_modal_list_area(None);
        session.set_modal_text_areas(Vec::new());
        session.set_modal_link_targets(Vec::new());
        return;
    };

    render_modal_background(frame, body_area, styles.background);
    if body_area.width == 0 || body_area.height == 0 {
        session.set_modal_list_area(None);
        session.set_modal_text_areas(Vec::new());
        session.set_modal_link_targets(Vec::new());
        return;
    }

    if modal.is_help_modal {
        use ratatui_cheese::help::{Binding, Help, HelpStyles};
        let help = Help::default()
            .show_all(true)
            .styles(HelpStyles::from_palette(&ratatui_cheese::theme::Palette::dark()))
            .bindings(vec![
                Binding::new("?", "help"),
                Binding::new("Enter", "submit"),
                Binding::new("Ctrl+C", "interrupt"),
                Binding::new("Esc Esc", "clear / open rewind"),
                Binding::new("Tab", "queue for next turn"),
                Binding::new("Shift+Tab", "switch agent"),
            ])
            .binding_groups(vec![
                vec![
                    Binding::new("!cmd", "shell mode"),
                    Binding::new("@path", "file reference"),
                    Binding::new("Enter", "run / queue busy turn"),
                    Binding::new("Ctrl+Enter", "queue for next turn"),
                    Binding::new("Tab", "queue for next turn"),
                    Binding::new("Shift+Enter", "new line"),
                    Binding::new("Esc Esc", "clear / open rewind"),
                    Binding::new("Ctrl+C", "interrupt/copy"),
                    Binding::new("Ctrl+D", "exit"),
                    Binding::new("PgUp/PgDn", "scroll"),
                    Binding::new("Alt+S", "subprocesses"),
                ],
                vec![
                    Binding::new("Ctrl+A/E", "line start/end"),
                    Binding::new("Cmd+Left/Right", "line start/end"),
                    Binding::new("Cmd+A/Backspace", "clear line"),
                    Binding::new("Ctrl+F/B", "char move"),
                    Binding::new("Alt+F/B", "word move"),
                    Binding::new("Alt+←/→", "word move"),
                    Binding::new("Ctrl+P/N", "history"),
                    Binding::new("Ctrl+R/S", "history search"),
                    Binding::new("Ctrl+W", "delete prev word"),
                    Binding::new("Alt+D", "delete next word"),
                    Binding::new("Ctrl+U/K", "clear line/delete end"),
                    Binding::new("Ctrl+T (unbound)", "transpose input"),
                    Binding::new("Alt+U/L/C", "case change"),
                ],
                vec![
                    Binding::new("/", "commands"),
                    Binding::new("?", "shortcuts"),
                    Binding::new("Shift+Tab", "switch agent"),
                    Binding::new("Ctrl+L", "clear screen"),
                    Binding::new("Ctrl+M", "model picker"),
                    Binding::new("Ctrl+O", "copy response"),
                    Binding::new("Alt+P", "prompt suggest"),
                    Binding::new("Alt+T", "toggle tool summaries"),
                    Binding::new("Alt+O", "review alias"),
                    Binding::new("Ctrl+T", "open/close review"),
                    Binding::new("r", "rich/raw review"),
                    Binding::new("Ctrl+I", "lists"),
                    Binding::new("Ctrl+G", "editor"),
                    Binding::new("Ctrl+Z/Y", "undo/redo"),
                ],
            ]);
        frame.render_widget(&help, body_area);
        session.set_modal_list_area(None);
        session.set_modal_text_areas(Vec::new());
        session.set_modal_link_targets(Vec::new());
        return;
    }
    let mut outcome = render_modal_body(
        frame,
        body_area,
        ModalBodyContext {
            instructions: &modal.lines,
            footer_hint: modal.footer_hint.as_deref(),
            status: modal.status.as_ref(),
            list: modal.list.as_mut(),
            styles: &styles,
            secure_prompt: modal.secure_prompt.as_ref(),
            search: modal.search.as_ref(),
            input: &input,
            cursor,
            input_styles: &input_styles,
        },
        workspace_root.as_deref(),
        last_mouse_position,
        link_style,
        hovered_link_style,
    );
    if let Some(title_area) = title_area {
        outcome.push_text_area(title_area);
        outcome.link_targets.extend(title_link_targets);
    }
    session.set_modal_list_area(outcome.list_area);
    session.set_modal_text_areas(outcome.text_areas);
    session.set_modal_link_targets(outcome.link_targets);
}

pub(crate) fn modal_render_styles(session: &Session) -> ModalRenderStyles {
    let default_style = modal_base_style(session);
    let header_style = modal_heading_style(session);
    let chrome_style = modal_chrome_style(session);
    let muted_style = session.styles.muted_text_style();
    let chrome_border_style = session
        .styles
        .border_style()
        .fg(ratatui_color_from_ansi(resolve_modal_chrome_ansi_color(session)))
        .remove_modifier(Modifier::DIM)
        .add_modifier(Modifier::BOLD);
    // Secondary text (labels, subtitles, hints, dividers, badges) recedes via
    // an explicit muted color — never `Modifier::DIM`: ratatui's
    // `Cell::set_style` only inserts modifiers, so any DIM painted as an area
    // background clings to every glyph drawn on top (the whole popup used to
    // render dimmed). Option titles stay at full foreground so every choice
    // reads; the selected row pops through `highlight` (accent + bold).
    ModalRenderStyles {
        border: chrome_border_style,
        highlight: modal_list_highlight_style(session),
        badge: muted_style.add_modifier(Modifier::BOLD),
        header: header_style,
        selectable: default_style,
        detail: muted_style,
        search_match: header_style.add_modifier(Modifier::UNDERLINED),
        title: chrome_style,
        divider: muted_style,
        background: default_style,
        instruction_border: chrome_border_style,
        instruction_title: header_style,
        instruction_bullet: header_style,
        instruction_body: default_style,
        hint: muted_style.add_modifier(Modifier::ITALIC),
        success: session.styles.accent_style().add_modifier(Modifier::BOLD),
        warning: session.styles.warning_style().add_modifier(Modifier::BOLD),
        danger: {
            let color = session.styles.text_fallback(InlineMessageKind::Error);
            let mut style = session.styles.default_style();
            if let Some(color) = color {
                style = style.fg(ratatui_color_from_ansi(color));
            }
            style.add_modifier(Modifier::BOLD)
        },
        accent: session.styles.modal_list_highlight_style(),
    }
}

fn handle_tool_code_fence_marker(session: &mut Session, text: &str) -> bool {
    let trimmed = text.trim();
    let stripped = trimmed.strip_prefix("```").or_else(|| trimmed.strip_prefix("~~~"));

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

fn remove_trailing_empty_tool_line(session: &mut Session) {
    let should_remove = session
        .lines
        .last()
        .map(|line| line.kind == InlineMessageKind::Tool && line.segments.is_empty())
        .unwrap_or(false);
    if should_remove {
        session.lines.pop();
        session.invalidate_scroll_metrics();
    }
}

#[cfg(test)]
mod tests;
