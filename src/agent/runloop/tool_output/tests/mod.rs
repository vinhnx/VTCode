pub(super) use serde_json::json;
pub(super) use vtcode_commons::ui_protocol::TaskItemStatus;
pub(super) use vtcode_core::config::ToolDisplayMode;
pub(super) use vtcode_core::ui::InlineHandle;
pub(super) use vtcode_core::utils::ansi::AnsiRenderer;

pub(super) use super::{
    TRACKER_ROW_DESCRIPTION_MAX_BYTES, TRACKER_TRANSCRIPT_MAX_ROWS, TrackerLine, collect_inline_output,
    humanize_tracker_title, is_tracker_current_row, preferred_follow_up_rendered_body, render_tool_output,
    should_render_command_session_terminal_panel, spooled_output_hint, tracker_current_tree_row,
    tracker_panel_metadata, tracker_panel_rows, tracker_progress_lines, tracker_row_text, tracker_summary_lines,
    tracker_transcript_lines, tracker_tree_body_lines,
};

fn transcript_texts(lines: &[TrackerLine]) -> Vec<&str> {
    lines.iter().map(|line| line.text.as_str()).collect()
}

fn transcript_statuses(lines: &[TrackerLine]) -> Vec<Option<TaskItemStatus>> {
    lines.iter().map(|line| line.status).collect()
}

mod render_basics;
mod render_edits;
mod render_exec;
mod render_hints;
mod render_mcp;
mod tracker_current_expanded;
mod tracker_header;
mod tracker_summary;
