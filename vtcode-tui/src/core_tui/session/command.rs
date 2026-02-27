use tui_popup::PopupState;
use unicode_segmentation::UnicodeSegmentation;

use super::super::types::{
    InlineCommand, InlineListSearchConfig, InlineListSelection, InlineMessageKind, InlineTextStyle,
    SecurePromptConfig,
};
use super::{
    Session,
    file_palette::{FilePalette, extract_file_reference},
    modal::{ModalListState, ModalSearchState, ModalState},
};

#[allow(dead_code)]
const USER_PREFIX: &str = "";

#[allow(dead_code)]
pub fn handle_command(session: &mut Session, command: InlineCommand) {
    match command {
        InlineCommand::AppendLine { kind, segments } => {
            session.clear_thinking_spinner_if_active(kind);
            session.push_line(kind, segments);
            session.transcript_content_changed = true;
        }
        InlineCommand::AppendPastedMessage {
            kind,
            text,
            line_count,
        } => {
            session.clear_thinking_spinner_if_active(kind);
            session.append_pasted_message(kind, text, line_count);
            session.transcript_content_changed = true;
        }
        InlineCommand::Inline { kind, segment } => {
            session.clear_thinking_spinner_if_active(kind);
            session.append_inline(kind, segment);
            session.transcript_content_changed = true;
        }
        InlineCommand::ReplaceLast { count, kind, lines } => {
            session.replace_last(count, kind, lines);
            session.transcript_content_changed = true;
        }
        InlineCommand::SetPrompt { prefix, style } => {
            session.prompt_prefix = prefix;
            session.prompt_style = style;
            ensure_prompt_style_color(session);
        }
        InlineCommand::SetPlaceholder { hint, style } => {
            session.placeholder = hint;
            session.placeholder_style = style;
        }
        InlineCommand::SetMessageLabels { agent, user } => {
            session.labels.agent = agent.filter(|label| !label.is_empty());
            session.labels.user = user.filter(|label| !label.is_empty());
            session.invalidate_scroll_metrics();
        }
        InlineCommand::SetHeaderContext { context } => {
            session.header_context = context;
            session.needs_redraw = true;
        }
        InlineCommand::SetInputStatus { left, right } => {
            session.input_status_left = left;
            session.input_status_right = right;
            session.needs_redraw = true;
        }
        InlineCommand::SetTheme { theme } => {
            session.theme = theme;
            ensure_prompt_style_color(session);
            session.invalidate_transcript_cache();
        }
        InlineCommand::SetQueuedInputs { entries } => {
            session.set_queued_inputs_entries(entries);
            mark_dirty(session);
        }
        InlineCommand::SetCursorVisible(value) => {
            session.cursor_visible = value;
        }
        InlineCommand::SetInputEnabled(value) => {
            session.input_enabled = value;
            super::slash::update_slash_suggestions(session);
        }
        InlineCommand::SetInput(content) => {
            // Check if the content appears to be an error message
            // If it looks like an error, redirect to transcript instead
            if is_error_content(&content) {
                // Add error to transcript instead of input field
                crate::utils::transcript::display_error(&content);
            } else {
                session.input_manager.set_content(content);
                session.scroll_manager.set_offset(0);
                super::slash::update_slash_suggestions(session);
            }
        }
        InlineCommand::ClearInput => {
            clear_input(session);
        }
        InlineCommand::ForceRedraw => {
            mark_dirty(session);
        }
        InlineCommand::ShowModal {
            title,
            lines,
            secure_prompt,
        } => {
            show_modal(session, title, lines, secure_prompt);
        }
        InlineCommand::ShowListModal {
            title,
            lines,
            items,
            selected,
            search,
        } => {
            show_list_modal(session, title, lines, items, selected, search);
        }
        InlineCommand::ShowWizardModal {
            title,
            steps,
            current_step,
            search,
            mode,
        } => {
            // Note: Wizard modal handling is done through show_wizard_modal in state.rs
            // This command path is for the session-based handling
            let wizard =
                super::modal::WizardModalState::new(title, steps, current_step, search, mode);
            session.wizard_modal = Some(wizard);
            session.input_enabled = false;
            session.cursor_visible = false;
            mark_dirty(session);
        }
        InlineCommand::CloseModal => {
            close_modal(session);
        }
        InlineCommand::LoadFilePalette { files, workspace } => {
            load_file_palette(session, files, workspace);
        }
        InlineCommand::ClearScreen => {
            clear_screen(session);
        }
        InlineCommand::SuspendEventLoop
        | InlineCommand::ResumeEventLoop
        | InlineCommand::ClearInputQueue => {
            // Handled by drive_terminal
        }
        InlineCommand::SetEditingMode(mode) => {
            session.header_context.editing_mode = mode;
            session.needs_redraw = true;
        }
        InlineCommand::SetAutonomousMode(enabled) => {
            session.header_context.autonomous_mode = enabled;
            session.needs_redraw = true;
        }
        InlineCommand::ShowPlanConfirmation { plan } => {
            show_plan_confirmation_modal(session, *plan);
        }
        InlineCommand::ShowDiffPreview {
            file_path,
            before,
            after,
            hunks,
            current_hunk,
        } => {
            show_diff_preview(session, file_path, before, after, hunks, current_hunk);
        }
        InlineCommand::SetSkipConfirmations(skip) => {
            session.skip_confirmations = skip;
            if skip {
                close_modal(session);
            }
        }
        InlineCommand::Shutdown => {
            request_exit(session);
        }
        InlineCommand::SetReasoningStage(stage) => {
            session.header_context.reasoning_stage = stage;
            session.invalidate_header_cache();
        }
    }
    session.needs_redraw = true;
}

/// Check if the content appears to be an error message that should go to transcript instead of input field
#[allow(dead_code)]
fn is_error_content(content: &str) -> bool {
    // Check if message contains common error indicators
    let lower_content = content.to_lowercase();
    let error_indicators = [
        "error:",
        "error ",
        "error\n",
        "failed",
        "failure",
        "exception",
        "invalid",
        "not found",
        "couldn't",
        "can't",
        "cannot",
        "denied",
        "forbidden",
        "unauthorized",
        "timeout",
        "connection refused",
        "no such",
        "does not exist",
    ];

    error_indicators
        .iter()
        .any(|indicator| lower_content.contains(indicator))
}

#[allow(dead_code)]
fn ensure_prompt_style_color(session: &mut Session) {
    if session.prompt_style.color.is_none() {
        session.prompt_style.color = session.theme.primary.or(session.theme.foreground);
    }
}

#[allow(dead_code)]
pub fn mark_dirty(session: &mut Session) {
    session.needs_redraw = true;
}

#[allow(dead_code)]
fn show_modal(
    session: &mut Session,
    title: String,
    lines: Vec<String>,
    secure_prompt: Option<SecurePromptConfig>,
) {
    let state = ModalState {
        title,
        lines,
        footer_hint: None,
        list: None,
        secure_prompt,
        is_plan_confirmation: false,
        popup_state: PopupState::default(),
        restore_input: session.input_enabled,
        restore_cursor: session.cursor_visible,
        search: None,
    };
    if state.secure_prompt.is_none() {
        session.input_enabled = false;
    }
    session.cursor_visible = false;
    session.modal = Some(state);
    mark_dirty(session);
}

#[allow(dead_code)]
fn show_list_modal(
    session: &mut Session,
    title: String,
    lines: Vec<String>,
    items: Vec<crate::ui::tui::types::InlineListItem>,
    selected: Option<InlineListSelection>,
    search: Option<InlineListSearchConfig>,
) {
    let mut list_state = ModalListState::new(items, selected);
    let search_state = search.map(ModalSearchState::from);
    if let Some(search) = &search_state {
        list_state.apply_search(&search.query);
    }
    let state = ModalState {
        title,
        lines,
        footer_hint: None,
        list: Some(list_state),
        secure_prompt: None,
        is_plan_confirmation: false,
        popup_state: PopupState::default(),
        restore_input: session.input_enabled,
        restore_cursor: session.cursor_visible,
        search: search_state,
    };
    session.input_enabled = false;
    session.cursor_visible = false;
    session.modal = Some(state);
    mark_dirty(session);
}

/// Show plan confirmation modal.
///
/// Displays the plan markdown and asks for confirmation.
/// User can choose from execute variants or return to plan editing.
pub(crate) fn show_plan_confirmation_modal(
    session: &mut Session,
    plan: crate::ui::tui::types::PlanContent,
) {
    use crate::ui::tui::types::{InlineListItem, InlineListSelection};

    let context_usage = match extract_context_usage(session.input_status_right.as_deref()) {
        Some(ContextUsageSummary {
            percent,
            is_left: true,
        }) => format!("{percent}% left"),
        Some(ContextUsageSummary {
            percent,
            is_left: false,
        }) => format!("{percent}% used"),
        None => "--".to_string(),
    };

    let mut lines: Vec<String> = plan
        .raw_content
        .lines()
        .map(|line| line.to_string())
        .collect();
    if lines.is_empty() && !plan.summary.is_empty() {
        lines.push(plan.summary.clone());
    }

    lines.insert(
        0,
        "A plan is ready to execute. Would you like to proceed?".to_string(),
    );

    let footer_hint = plan
        .file_path
        .as_ref()
        .map(|path| format!("ctrl-g to edit in VS Code · {path}"));

    // Four-option confirmation menu
    let items = vec![
        InlineListItem {
            title: format!("Yes, clear context ({context_usage}) and auto-accept edits"),
            subtitle: Some("Reset conversation history and execute immediately.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalClearContextAutoAccept),
            search_value: None,
        },
        InlineListItem {
            title: "Yes, auto-accept edits".to_string(),
            subtitle: Some("Keep context and execute with auto-approval.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalAutoAccept),
            search_value: None,
        },
        InlineListItem {
            title: "Yes, manually approve edits".to_string(),
            subtitle: Some("Keep context and confirm each edit before applying.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalExecute),
            search_value: None,
        },
        InlineListItem {
            title: "Type feedback to revise the plan".to_string(),
            subtitle: Some("Return to plan mode and refine the plan.".to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::PlanApprovalEditPlan),
            search_value: None,
        },
    ];

    let list_state = ModalListState::new(
        items,
        Some(InlineListSelection::PlanApprovalClearContextAutoAccept),
    );

    let state = ModalState {
        title: "Ready to code?".to_string(),
        lines,
        footer_hint,
        list: Some(list_state),
        secure_prompt: None,
        is_plan_confirmation: true,
        popup_state: PopupState::default(),
        restore_input: session.input_enabled,
        restore_cursor: session.cursor_visible,
        search: None,
    };
    session.input_enabled = false;
    session.cursor_visible = false;
    session.modal = Some(state);
    mark_dirty(session);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContextUsageSummary {
    percent: u8,
    is_left: bool,
}

fn extract_context_usage(status_line: Option<&str>) -> Option<ContextUsageSummary> {
    let status_line = status_line?;
    let words: Vec<&str> = status_line.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }

    for (index, pair) in words.windows(2).enumerate() {
        let candidate = pair[0].trim_end_matches('%');
        let next = pair[1].trim_matches(|ch: char| ch == ',' || ch == '.');
        if !next.eq_ignore_ascii_case("context") {
            continue;
        }

        if let Ok(percent) = candidate.parse::<u8>() {
            let is_left = words.get(index + 2).is_some_and(|value| {
                value
                    .trim_matches(|ch: char| ch == ',' || ch == '.')
                    .eq_ignore_ascii_case("left")
            });
            return Some(ContextUsageSummary {
                percent: percent.min(100),
                is_left,
            });
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{ContextUsageSummary, extract_context_usage};

    #[test]
    fn parses_legacy_context_usage_percent() {
        let usage = extract_context_usage(Some("model | 25% context | (low)"));
        assert_eq!(
            usage,
            Some(ContextUsageSummary {
                percent: 25,
                is_left: false,
            })
        );
    }

    #[test]
    fn parses_context_left_percent() {
        let usage = extract_context_usage(Some("model | 17% context left | (low)"));
        assert_eq!(
            usage,
            Some(ContextUsageSummary {
                percent: 17,
                is_left: true,
            })
        );
    }
}

#[allow(dead_code)]
fn close_modal(session: &mut Session) {
    if let Some(state) = session.modal.take() {
        session.input_enabled = state.restore_input;
        session.cursor_visible = state.restore_cursor;
        // Force full screen clear on next render to remove modal artifacts
        session.needs_full_clear = true;
        // Force transcript cache invalidation to ensure full redraw
        session.invalidate_transcript_cache();
        session.mark_line_dirty(0);
        mark_dirty(session);
        return;
    }

    if session.wizard_modal.take().is_some() {
        session.input_enabled = true;
        session.cursor_visible = true;
        session.needs_full_clear = true;
        session.invalidate_transcript_cache();
        session.mark_line_dirty(0);
        mark_dirty(session);
    }
}

#[allow(dead_code)]
fn load_file_palette(session: &mut Session, files: Vec<String>, workspace: std::path::PathBuf) {
    let mut palette = FilePalette::new(workspace);
    palette.load_files(files);
    session.file_palette = Some(palette);
    session.file_palette_active = false;
    check_file_reference_trigger(session);
}

#[allow(dead_code)]
pub(super) fn check_file_reference_trigger(session: &mut Session) {
    if let Some(palette) = session.file_palette.as_mut() {
        if let Some((_, _, query)) = extract_file_reference(
            session.input_manager.content(),
            session.input_manager.cursor(),
        ) {
            // Reset selection and clear previous state when opening
            palette.reset();
            palette.set_filter(query);
            session.file_palette_active = true;
        } else {
            session.file_palette_active = false;
        }
    }
}

#[allow(dead_code)]
pub fn close_file_palette(session: &mut Session) {
    session.file_palette_active = false;

    // Clean up resources when closing to free memory
    if let Some(palette) = session.file_palette.as_mut() {
        palette.cleanup();
    }
}

#[allow(dead_code)]
pub fn insert_file_reference(session: &mut Session, file_path: &str) {
    if let Some((start, end, _)) = extract_file_reference(
        session.input_manager.content(),
        session.input_manager.cursor(),
    ) {
        let replacement = format!("@{}", file_path);
        let content = session.input_manager.content().to_owned();
        let mut new_content = String::new();
        new_content.push_str(&content[..start]);
        new_content.push_str(&replacement);
        new_content.push_str(&content[end..]);
        session.input_manager.set_content(new_content);
        session.input_manager.set_cursor(start + replacement.len());
        session.input_manager.insert_char(' ');
    }
}

#[allow(dead_code)]
fn clear_screen(session: &mut Session) {
    session.lines.clear();
    session.scroll_manager.set_offset(0);
    session.invalidate_transcript_cache();
    session.invalidate_scroll_metrics();
    session.needs_full_clear = true;
    mark_dirty(session);
}

#[allow(dead_code)]
pub fn request_exit(session: &mut Session) {
    session.should_exit = true;
}

pub(super) fn clear_input(session: &mut Session) {
    session.input_manager.clear();
    session.input_compact_mode = false;
    session.scroll_manager.set_offset(0);
    super::slash::update_slash_suggestions(session);
    session.mark_dirty();
}

#[allow(dead_code)]
pub(super) fn insert_char(session: &mut Session, ch: char) {
    if ch == '\u{7f}' {
        return;
    }
    if ch == '\n' && !can_insert_newline(session) {
        return;
    }
    session.input_manager.insert_char(ch);
    super::slash::update_slash_suggestions(session);
}

#[allow(dead_code)]
pub(super) fn insert_text(session: &mut Session, text: &str) {
    let mut remaining_newlines = remaining_newline_capacity(session);
    let sanitized: String = text
        .chars()
        .filter(|ch| {
            if matches!(ch, '\r' | '\u{7f}') {
                return false;
            }
            if *ch == '\n' {
                if remaining_newlines == 0 {
                    return false;
                }
                remaining_newlines = remaining_newlines.saturating_sub(1);
            }
            true
        })
        .collect();
    if sanitized.is_empty() {
        return;
    }
    session.input_manager.insert_text(&sanitized);
    super::slash::update_slash_suggestions(session);
}

#[allow(dead_code)]
pub(super) fn delete_char(session: &mut Session) {
    session.input_manager.backspace();
    super::slash::update_slash_suggestions(session);
}

#[allow(dead_code)]
pub(super) fn delete_char_forward(session: &mut Session) {
    session.input_manager.delete();
    super::slash::update_slash_suggestions(session);
}

#[allow(dead_code)]
pub(super) fn delete_word_backward(session: &mut Session) {
    if session.input_manager.cursor() == 0 {
        return;
    }

    // Find the start of the current word by moving backward (same logic as move_left_word)
    let graphemes: Vec<(usize, &str)> = session.input_manager.content()
        [..session.input_manager.cursor()]
        .grapheme_indices(true)
        .collect();

    if graphemes.is_empty() {
        return;
    }

    let mut index = graphemes.len();

    // Skip any trailing whitespace
    while index > 0 {
        let (_, grapheme) = graphemes[index - 1];
        if grapheme.chars().all(char::is_whitespace) {
            index -= 1;
        } else {
            break;
        }
    }

    // Move backwards until we find whitespace (start of the word)
    while index > 0 {
        let (_, grapheme) = graphemes[index - 1];
        if grapheme.chars().all(char::is_whitespace) {
            break;
        }
        index -= 1;
    }

    // Calculate the position to delete from
    let delete_start = if index < graphemes.len() {
        graphemes[index].0
    } else {
        0
    };

    // Delete from delete_start to cursor
    if delete_start < session.input_manager.cursor() {
        let content = session.input_manager.content().to_owned();
        let mut new_content = String::new();
        new_content.push_str(&content[..delete_start]);
        new_content.push_str(&content[session.input_manager.cursor()..]);
        session.input_manager.set_content(new_content);
        session.input_manager.set_cursor(delete_start);
        super::slash::update_slash_suggestions(session);
    }
}

#[allow(dead_code)]
pub(super) fn delete_to_start_of_line(session: &mut Session) {
    let content = session.input_manager.content();
    let cursor = session.input_manager.cursor();

    let before = &content[..cursor];
    let delete_start = if let Some(newline_pos) = before.rfind('\n') {
        newline_pos + 1
    } else {
        0
    };

    if delete_start < cursor {
        let new_content = format!("{}{}", &content[..delete_start], &content[cursor..]);
        session.input_manager.set_content(new_content);
        session.input_manager.set_cursor(delete_start);
        super::slash::update_slash_suggestions(session);
    }
}

#[allow(dead_code)]
pub(super) fn delete_to_end_of_line(session: &mut Session) {
    let content = session.input_manager.content();
    let cursor = session.input_manager.cursor();

    let rest = &content[cursor..];
    let delete_len = if let Some(newline_pos) = rest.find('\n') {
        newline_pos
    } else {
        rest.len()
    };

    if delete_len > 0 {
        let new_content = format!("{}{}", &content[..cursor], &content[cursor + delete_len..]);
        session.input_manager.set_content(new_content);
        super::slash::update_slash_suggestions(session);
    }
}

#[allow(dead_code)]
pub(super) fn remember_submitted_input(
    session: &mut Session,
    submitted: super::input_manager::InputHistoryEntry,
) {
    session.input_manager.add_to_history(submitted);
}

#[allow(dead_code)]
fn remaining_newline_capacity(session: &Session) -> usize {
    crate::config::constants::ui::INLINE_INPUT_MAX_LINES
        .saturating_sub(1)
        .saturating_sub(session.input_manager.content().matches('\n').count())
}

#[allow(dead_code)]
fn can_insert_newline(session: &Session) -> bool {
    remaining_newline_capacity(session) > 0
}

#[allow(dead_code)]
pub(super) fn push_line(
    session: &mut Session,
    kind: InlineMessageKind,
    segments: Vec<crate::ui::tui::types::InlineSegment>,
) {
    let previous_max_offset = session.current_max_scroll_offset();
    let revision = session.next_revision();
    session.lines.push(super::message::MessageLine {
        kind,
        segments,
        revision,
    });
    session.invalidate_scroll_metrics();
    session.adjust_scroll_after_change(previous_max_offset);
}

#[allow(dead_code)]
pub(super) fn append_inline(
    session: &mut Session,
    kind: InlineMessageKind,
    segment: crate::ui::tui::types::InlineSegment,
) {
    let previous_max_offset = session.current_max_scroll_offset();

    // For Tool messages, process the entire text as one unit to avoid excessive line breaks
    // Newlines in tool output will be preserved as actual newline characters rather than
    // triggering new message lines
    if kind == InlineMessageKind::Tool {
        append_text(session, kind, &segment.text, &segment.style);
    } else {
        let mut remaining = segment.text.as_str();
        let style = segment.style.clone();

        while !remaining.is_empty() {
            if let Some((index, control)) = remaining
                .char_indices()
                .find(|(_, ch)| matches!(ch, '\n' | '\r'))
            {
                let (text, _) = remaining.split_at(index);
                if !text.is_empty() {
                    append_text(session, kind, text, &style);
                }

                let control_char = control;
                let next_index = index + control_char.len_utf8();
                remaining = &remaining[next_index..];

                match control_char {
                    '\n' => start_line(session, kind),
                    '\r' => {
                        if remaining.starts_with('\n') {
                            remaining = &remaining[1..];
                            start_line(session, kind);
                        } else {
                            reset_line(session, kind);
                        }
                    }
                    _ => {}
                }
            } else {
                if !remaining.is_empty() {
                    append_text(session, kind, remaining, &style);
                }
                break;
            }
        }
    }

    session.invalidate_scroll_metrics();
    session.adjust_scroll_after_change(previous_max_offset);
}

#[allow(dead_code)]
pub(super) fn replace_last(
    session: &mut Session,
    count: usize,
    kind: InlineMessageKind,
    lines: Vec<Vec<crate::ui::tui::types::InlineSegment>>,
) {
    let previous_max_offset = session.current_max_scroll_offset();
    let remove_count = std::cmp::min(count, session.lines.len());
    for _ in 0..remove_count {
        session.lines.pop();
    }
    for segments in lines {
        let revision = session.next_revision();
        session.lines.push(super::message::MessageLine {
            kind,
            segments,
            revision,
        });
    }
    session.invalidate_scroll_metrics();
    session.adjust_scroll_after_change(previous_max_offset);
}

#[allow(dead_code)]
fn append_text(
    session: &mut Session,
    kind: InlineMessageKind,
    text: &str,
    style: &InlineTextStyle,
) {
    if text.is_empty() {
        return;
    }

    if kind == InlineMessageKind::Tool && session.handle_tool_code_fence_marker(text) {
        return;
    }

    let mut appended = false;

    let mut mark_revision = false;
    {
        if let Some(line) = session.lines.last_mut()
            && line.kind == kind
        {
            if let Some(last) = line.segments.last_mut()
                && &*last.style == style
            {
                last.text.push_str(text);
                appended = true;
                mark_revision = true;
            }
            if !appended {
                line.segments.push(crate::ui::tui::types::InlineSegment {
                    text: text.to_owned(),
                    style: std::sync::Arc::new(style.clone()),
                });
                appended = true;
                mark_revision = true;
            }
        }
    }

    if mark_revision {
        let revision = session.next_revision();
        if let Some(line) = session.lines.last_mut()
            && line.kind == kind
        {
            line.revision = revision;
        }
    }

    if appended {
        session.invalidate_scroll_metrics();
        return;
    }

    let can_reuse_last = session
        .lines
        .last()
        .map(|line| line.kind == kind && line.segments.is_empty())
        .unwrap_or(false);
    if can_reuse_last {
        let revision = session.next_revision();
        if let Some(line) = session.lines.last_mut() {
            line.segments.push(crate::ui::tui::types::InlineSegment {
                text: text.to_owned(),
                style: std::sync::Arc::new(style.clone()),
            });
            line.revision = revision;
        }
        session.invalidate_scroll_metrics();
        return;
    }

    let revision = session.next_revision();
    session.lines.push(super::message::MessageLine {
        kind,
        segments: vec![crate::ui::tui::types::InlineSegment {
            text: text.to_owned(),
            style: std::sync::Arc::new(style.clone()),
        }],
        revision,
    });

    session.invalidate_scroll_metrics();
}

#[allow(dead_code)]
fn start_line(session: &mut Session, kind: InlineMessageKind) {
    push_line(session, kind, Vec::new());
}

#[allow(dead_code)]
fn reset_line(session: &mut Session, kind: InlineMessageKind) {
    let mut cleared = false;
    {
        if let Some(line) = session.lines.last_mut()
            && line.kind == kind
        {
            line.segments.clear();
            cleared = true;
        }
    }
    if cleared {
        let revision = session.next_revision();
        if let Some(line) = session.lines.last_mut()
            && line.kind == kind
        {
            line.revision = revision;
        }
        session.invalidate_scroll_metrics();
        return;
    }
    start_line(session, kind);
}

/// Show diff preview modal for file edit approval
pub(super) fn show_diff_preview(
    session: &mut Session,
    file_path: String,
    before: String,
    after: String,
    hunks: Vec<crate::ui::tui::types::DiffHunk>,
    current_hunk: usize,
) {
    use crate::ui::tui::types::DiffPreviewState;

    let mut state = DiffPreviewState::new(file_path, before, after, hunks);
    state.current_hunk = current_hunk;

    session.diff_preview = Some(state);
    session.input_enabled = false;
    session.cursor_visible = false;
    mark_dirty(session);
}
