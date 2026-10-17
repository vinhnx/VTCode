//! File-opener configuration and link/table helpers.

use crate::ui::tui::InlineMessageKind;
use std::sync::{Mutex, OnceLock};
use unicode_width::UnicodeWidthStr;
use url::Url;
use vtcode_commons::{parse_editor_target, resolve_editor_path};

static FILE_OPENER: OnceLock<Mutex<vtcode_config::FileOpener>> = OnceLock::new();

pub fn apply_file_opener_config(file_opener: vtcode_config::FileOpener) {
    let cell = FILE_OPENER.get_or_init(|| Mutex::new(vtcode_config::FileOpener::None));
    if let Ok(mut guard) = cell.lock() {
        *guard = file_opener;
    }
}

pub(super) fn current_file_opener() -> vtcode_config::FileOpener {
    FILE_OPENER
        .get()
        .map(|cell| *cell.lock().unwrap_or_else(|e| e.into_inner()))
        .unwrap_or(vtcode_config::FileOpener::None)
}

pub(super) fn make_clickable_target(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_remote_link_target(trimmed) {
        return Some(trimmed.to_string());
    }

    let opener = current_file_opener();
    let scheme = opener.scheme()?;
    let target = parse_editor_target(trimmed)?;
    let cwd = std::env::current_dir().ok()?;
    let file_url = Url::from_file_path(resolve_editor_path(target.path(), &cwd)).ok()?;
    let suffix = target.location_suffix().unwrap_or("");
    Some(format!("{scheme}://file{}{}", file_url.as_str().trim_start_matches("file://"), suffix))
}

pub(super) fn should_strip_inline_local_link_underline(target: &str) -> bool {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_remote_link_target(trimmed) {
        return false;
    }
    match Url::parse(trimmed) {
        Ok(url) => url.scheme() == "file",
        Err(_) => true,
    }
}

fn is_remote_link_target(target: &str) -> bool {
    target.starts_with("http://") || target.starts_with("https://")
}

pub(super) fn terminal_table_content_width(indent: &str) -> Option<usize> {
    crossterm::terminal::size()
        .ok()
        .map(|(width, _)| usize::from(width).saturating_sub(UnicodeWidthStr::width(indent)))
}

pub(super) fn transcript_table_frame_width(kind: InlineMessageKind, agent_label_frame_width: usize) -> usize {
    match kind {
        // Agent messages receive ` •` plus one content padding cell during
        // transcript reflow. Other table-bearing paths use their block prefix;
        // their rendered table lines are body/detail lines without a right edge.
        InlineMessageKind::Agent => UnicodeWidthStr::width(" • ") + agent_label_frame_width,
        InlineMessageKind::Tool => UnicodeWidthStr::width("    "),
        InlineMessageKind::Pty => UnicodeWidthStr::width("  "),
        InlineMessageKind::Policy
        | InlineMessageKind::User
        | InlineMessageKind::Info
        | InlineMessageKind::Error
        | InlineMessageKind::Warning => 0,
    }
}
