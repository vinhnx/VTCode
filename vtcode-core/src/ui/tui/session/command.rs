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
    prompt_palette::{PromptPalette, extract_prompt_reference},
};
use crate::config::constants::prompts;
use crate::prompts::CustomPromptRegistry;

#[allow(dead_code)]
const USER_PREFIX: &str = "";
#[allow(dead_code)]
const PROMPT_COMMAND_PREFIX: &str = "/prompt:";

#[allow(dead_code)]
pub fn handle_command(session: &mut Session, command: InlineCommand) {
    match command {
        InlineCommand::AppendLine { kind, segments } => {
            // Remove spinner message when agent response arrives
            if kind == InlineMessageKind::Agent && session.thinking_spinner.is_active {
                if let Some(spinner_idx) = session.thinking_spinner.spinner_line_index {
                    if spinner_idx < session.lines.len() {
                        session.lines.remove(spinner_idx);
                    }
                }
                session.thinking_spinner.stop();
            }
            session.push_line(kind, segments);
            session.transcript_content_changed = true;
        }
        InlineCommand::Inline { kind, segment } => {
            // Remove spinner message when agent response arrives
            if kind == InlineMessageKind::Agent && session.thinking_spinner.is_active {
                if let Some(spinner_idx) = session.thinking_spinner.spinner_line_index {
                    if spinner_idx < session.lines.len() {
                        session.lines.remove(spinner_idx);
                    }
                }
                session.thinking_spinner.stop();
            }
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
        InlineCommand::SetPlan { plan } => {
            session.plan = plan;
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
        InlineCommand::CloseModal => {
            close_modal(session);
        }
        InlineCommand::SetCustomPrompts { registry } => {
            set_custom_prompts(session, registry);
        }
        InlineCommand::LoadFilePalette { files, workspace } => {
            load_file_palette(session, files, workspace);
        }
        InlineCommand::ClearScreen => {
            clear_screen(session);
        }
        InlineCommand::SuspendEventLoop | InlineCommand::ResumeEventLoop => {
            // Handled by drive_terminal
        }
        InlineCommand::Shutdown => {
            request_exit(session);
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
        list: None,
        secure_prompt,
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
        list: Some(list_state),
        secure_prompt: None,
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

#[allow(dead_code)]
fn close_modal(session: &mut Session) {
    if let Some(state) = session.modal.take() {
        session.input_enabled = state.restore_input;
        session.cursor_visible = state.restore_cursor;
        // Force full screen clear on next render to remove modal artifacts
        session.needs_full_clear = true;
        // Force transcript cache invalidation to ensure full redraw
        session.transcript_cache = None;
        mark_dirty(session);
    }
}

#[allow(dead_code)]
pub fn set_custom_prompts(session: &mut Session, custom_prompts: CustomPromptRegistry) {
    // Initialize prompt palette when custom prompts are loaded
    if custom_prompts.enabled() && !custom_prompts.is_empty() {
        let mut palette = PromptPalette::new();
        palette.load_prompts(custom_prompts.iter());
        session.prompt_palette = Some(palette);
    }

    session.custom_prompts = Some(custom_prompts);
    // Update slash palette if we're currently viewing slash commands
    if session.input_manager.content().starts_with('/') {
        super::slash::update_slash_suggestions(session);
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
        let content = session.input_manager.content().to_string();
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
pub(super) fn check_prompt_reference_trigger(session: &mut Session) {
    // Initialize prompt palette on-demand if it doesn't exist
    if session.prompt_palette.is_none() {
        let mut palette = PromptPalette::new();

        // Try loading from custom_prompts first
        let loaded = if let Some(ref custom_prompts) = session.custom_prompts {
            if custom_prompts.enabled() && !custom_prompts.is_empty() {
                palette.load_prompts(custom_prompts.iter());
                true
            } else {
                false
            }
        } else {
            false
        };

        // Fallback: load directly from filesystem if custom_prompts not available
        if !loaded {
            // Try default .vtcode/prompts directory
            if let Ok(current_dir) = std::env::current_dir() {
                let prompts_dir = current_dir.join(".vtcode").join("prompts");
                palette.load_from_directory(&prompts_dir);
            }
        }

        if let Ok(current_dir) = std::env::current_dir() {
            let core_dir = current_dir.join(prompts::CORE_BUILTIN_PROMPTS_DIR);
            palette.load_from_directory(&core_dir);
        }

        let builtin_prompts = CustomPromptRegistry::builtin_prompts();
        if !builtin_prompts.is_empty() {
            palette.append_custom_prompts(builtin_prompts.iter());
        }

        session.prompt_palette = Some(palette);
    }

    if let Some(palette) = session.prompt_palette.as_mut() {
        if let Some((_, _, query)) = extract_prompt_reference(
            session.input_manager.content(),
            session.input_manager.cursor(),
        ) {
            // Reset selection and clear previous state when opening
            palette.reset();
            palette.set_filter(query);
            session.prompt_palette_active = true;
        } else {
            session.prompt_palette_active = false;
        }
    }
}

#[allow(dead_code)]
pub fn close_prompt_palette(session: &mut Session) {
    session.prompt_palette_active = false;

    // Clean up resources when closing to free memory
    if let Some(palette) = session.prompt_palette.as_mut() {
        palette.cleanup();
    }
}

#[allow(dead_code)]
pub fn insert_prompt_reference(session: &mut Session, prompt_name: &str) {
    let mut command = String::from(PROMPT_COMMAND_PREFIX);
    command.push_str(prompt_name);
    command.push(' ');

    session.input_manager.set_content(command);
    session.input_manager.move_cursor_to_end();
    super::slash::update_slash_suggestions(session);
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
        .filter_map(|ch| {
            if matches!(ch, '\r' | '\u{7f}') {
                return None;
            }
            if ch == '\n' {
                if remaining_newlines == 0 {
                    return None;
                }
                remaining_newlines = remaining_newlines.saturating_sub(1);
            }
            Some(ch)
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
        let content = session.input_manager.content().to_string();
        let mut new_content = String::new();
        new_content.push_str(&content[..delete_start]);
        new_content.push_str(&content[session.input_manager.cursor()..]);
        session.input_manager.set_content(new_content);
        session.input_manager.set_cursor(delete_start);
        super::slash::update_slash_suggestions(session);
    }
}

#[allow(dead_code)]
pub(super) fn delete_sentence_backward(session: &mut Session) {
    if session.input_manager.cursor() == 0 {
        return;
    }

    let input_before_cursor = &session.input_manager.content()[..session.input_manager.cursor()];
    let chars: Vec<(usize, char)> = input_before_cursor.char_indices().collect();

    if chars.is_empty() {
        return;
    }

    // Look backwards from cursor for the most recent sentence ending followed by whitespace
    // A sentence typically ends with ., !, ? followed by space, tab, newline or end of input
    let mut delete_start = 0;

    // Search backwards to find the most recent sentence boundary
    for i in (0..chars.len()).rev() {
        let (pos, ch) = chars[i];

        if matches!(ch, '.' | '!' | '?') {
            // Check if this punctuation is followed by whitespace or we're at the end
            // Since we're looking at input before cursor, we check the original full input
            if pos + ch.len_utf8() < session.input_manager.content().len() {
                // Check the character after the punctuation in the full input string
                let after_punct = &session.input_manager.content()
                    [pos + ch.len_utf8()..session.input_manager.cursor()];
                if !after_punct.is_empty() {
                    let next_char = after_punct.chars().next().unwrap();
                    if next_char.is_whitespace() {
                        // Found sentence ending punctuation followed by whitespace
                        delete_start = pos + ch.len_utf8();
                        break;
                    }
                } else {
                    // At the end of the text being considered (before cursor)
                    // This might be a sentence boundary if there's whitespace after cursor
                    delete_start = pos + ch.len_utf8();
                    break;
                }
            } else {
                // At the end of the entire input string
                delete_start = pos + ch.len_utf8();
                break;
            }
        } else if matches!(ch, '\n' | '\r') {
            // Newlines can also separate sentences
            delete_start = pos + ch.len_utf8();
            break;
        }
    }

    // Delete from delete_start to cursor
    if delete_start < session.input_manager.cursor() {
        let content = session.input_manager.content().to_string();
        let mut new_content = String::new();
        new_content.push_str(&content[..delete_start]);
        new_content.push_str(&content[session.input_manager.cursor()..]);
        session.input_manager.set_content(new_content);
        session.input_manager.set_cursor(delete_start);
        super::slash::update_slash_suggestions(session);
    }
}

#[allow(dead_code)]
pub(super) fn remember_submitted_input(session: &mut Session, submitted: &str) {
    session.input_manager.add_to_history(submitted.to_string());
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
    let previous_max_offset = super::render::current_max_scroll_offset(session);
    let revision = session.next_revision();
    session.lines.push(super::message::MessageLine {
        kind,
        segments,
        revision,
    });
    super::render::invalidate_scroll_metrics(session);
    super::render::adjust_scroll_after_change(session, previous_max_offset);
}

#[allow(dead_code)]
pub(super) fn append_inline(
    session: &mut Session,
    kind: InlineMessageKind,
    segment: crate::ui::tui::types::InlineSegment,
) {
    let previous_max_offset = super::render::current_max_scroll_offset(session);

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

    super::render::invalidate_scroll_metrics(session);
    super::render::adjust_scroll_after_change(session, previous_max_offset);
}

#[allow(dead_code)]
pub(super) fn replace_last(
    session: &mut Session,
    count: usize,
    kind: InlineMessageKind,
    lines: Vec<Vec<crate::ui::tui::types::InlineSegment>>,
) {
    let previous_max_offset = super::render::current_max_scroll_offset(session);
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
    super::render::invalidate_scroll_metrics(session);
    super::render::adjust_scroll_after_change(session, previous_max_offset);
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

    if kind == InlineMessageKind::Tool
        && super::render::handle_tool_code_fence_marker(session, text)
    {
        return;
    }

    let mut appended = false;

    let mut mark_revision = false;
    {
        if let Some(line) = session.lines.last_mut() {
            if line.kind == kind {
                if let Some(last) = line.segments.last_mut() {
                    if last.style == *style {
                        last.text.push_str(text);
                        appended = true;
                        mark_revision = true;
                    }
                }
                if !appended {
                    line.segments.push(crate::ui::tui::types::InlineSegment {
                        text: text.to_string(),
                        style: style.clone(),
                    });
                    appended = true;
                    mark_revision = true;
                }
            }
        }
    }

    if mark_revision {
        let revision = session.next_revision();
        if let Some(line) = session.lines.last_mut() {
            if line.kind == kind {
                line.revision = revision;
            }
        }
    }

    if appended {
        super::render::invalidate_scroll_metrics(session);
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
                text: text.to_string(),
                style: style.clone(),
            });
            line.revision = revision;
        }
        super::render::invalidate_scroll_metrics(session);
        return;
    }

    let revision = session.next_revision();
    session.lines.push(super::message::MessageLine {
        kind,
        segments: vec![crate::ui::tui::types::InlineSegment {
            text: text.to_string(),
            style: style.clone(),
        }],
        revision,
    });

    super::render::invalidate_scroll_metrics(session);
}

#[allow(dead_code)]
fn start_line(session: &mut Session, kind: InlineMessageKind) {
    push_line(session, kind, Vec::new());
}

#[allow(dead_code)]
fn reset_line(session: &mut Session, kind: InlineMessageKind) {
    let mut cleared = false;
    {
        if let Some(line) = session.lines.last_mut() {
            if line.kind == kind {
                line.segments.clear();
                cleared = true;
            }
        }
    }
    if cleared {
        let revision = session.next_revision();
        if let Some(line) = session.lines.last_mut() {
            if line.kind == kind {
                line.revision = revision;
            }
        }
        super::render::invalidate_scroll_metrics(session);
        return;
    }
    start_line(session, kind);
}
