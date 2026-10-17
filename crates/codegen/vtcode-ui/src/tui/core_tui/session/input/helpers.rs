//! Pure text-measurement, label, and large-paste preview helpers for the composer input.
//
//Extracted from the session input module; all functions here are stateless
//and take only plain data, so they can be unit-tested and reused without a
//live `Session`.

use ratatui::{
    buffer::Buffer,
    style::{Modifier, Style},
    text::Text,
};
use regex::Regex;
use std::fmt::Write;
use std::path::Path;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthChar;
use vtcode_commons::formatting::contains_ignore_ascii_case;
use vtcode_commons::fs::{is_image_path, trim_trailing_image_path_str, unescape_whitespace};

use crate::tui::config::constants::ui;
use crate::tui::core_tui::blocked_status::is_git_status;

use super::CompactInputPreview;
use super::tokenizer::{InputTokenKind, tokenize_input};

pub(super) fn inline_prompt_suggestion_suffix(current: &str, suggestion: &str) -> Option<String> {
    if current.trim().is_empty() {
        return Some(suggestion.to_string());
    }

    let suggestion_lower = suggestion.to_lowercase();
    let current_lower = current.to_lowercase();
    if !suggestion_lower.starts_with(&current_lower) {
        return None;
    }

    Some(suggestion.chars().skip(current.chars().count()).collect())
}

pub(super) fn compact_image_label(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| trimmed.strip_prefix('\'').and_then(|value| value.strip_suffix('\'')))
        .unwrap_or(trimmed);

    if unquoted.starts_with("data:image/") {
        return Some("inline image".to_string());
    }

    let windows_drive = unquoted.as_bytes().get(1).is_some_and(|ch| *ch == b':')
        && unquoted.as_bytes().get(2).is_some_and(|ch| *ch == b'\\' || *ch == b'/');
    let starts_like_path = unquoted.starts_with('@')
        || unquoted.starts_with("file://")
        || unquoted.starts_with('/')
        || unquoted.starts_with("./")
        || unquoted.starts_with("../")
        || unquoted.starts_with("~/")
        || windows_drive;
    if !starts_like_path {
        return None;
    }

    let without_at = unquoted.strip_prefix('@').unwrap_or(unquoted);

    // Skip npm scoped package patterns like @scope/package@version
    if without_at.contains('/')
        && !without_at.starts_with('.')
        && !without_at.starts_with('/')
        && !without_at.starts_with("~/")
    {
        // Check if this looks like @scope/package (npm package)
        let parts: Vec<&str> = without_at.split('/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() {
            // Reject if it looks like a package name (no extension on second component)
            if !parts[parts.len() - 1].contains('.') {
                return None;
            }
        }
    }

    let without_scheme = without_at.strip_prefix("file://").unwrap_or(without_at);
    let path = Path::new(without_scheme);
    if !is_image_path(path) {
        return None;
    }

    let label = path.file_name().and_then(|name| name.to_str()).unwrap_or(without_scheme);
    Some(label.to_string())
}

static IMAGE_PATH_INLINE_REGEX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ix)
        (?:^|[\s\(\[\{<\"'`])
        (
            @?
            (?:file://)?
            (?:
                ~/(?:[^\n/]+/)+
              | /(?:[^\n/]+/)+
              | [A-Za-z]:[\\/](?:[^\n\\\/]+[\\/])+
            )
            [^\n]+?
            \.(?:png|jpe?g|gif|webp)
        )"#,
    )
    .expect("Failed to compile inline image path regex")
});

pub(super) fn compact_image_placeholders(content: &str) -> Option<String> {
    // Hotpath: this regex scans the full composer content on every keystroke
    // (via `refresh_input_edit_state`). Every alternative requires a literal
    // `.ext` suffix, so content without `.` can never match — skip the engine
    // entirely for plain prose (same dot fast-reject lesson as link-candidate
    // detection).
    if !content.contains('.') {
        return None;
    }
    let mut matches = Vec::new();
    for capture in IMAGE_PATH_INLINE_REGEX.captures_iter(content) {
        let Some(path_match) = capture.get(1) else {
            continue;
        };
        let raw = path_match.as_str();
        // The regex may consume trailing text after the image extension.
        // Try progressively shorter suffixes to find the actual image path.
        let trimmed_raw = trim_trailing_image_path_str(raw);
        let Some(label) = image_label_for_path(trimmed_raw) else {
            continue;
        };
        let end = path_match.start() + trimmed_raw.len();
        matches.push((path_match.start(), end, label));
    }

    if matches.is_empty() {
        return None;
    }

    let mut result = String::with_capacity(content.len());
    let mut last_end = 0usize;
    for (start, end, label) in matches {
        if start < last_end {
            continue;
        }
        result.push_str(&content[last_end..start]);
        let _ = write!(result, "[Image: {label}]");
        last_end = end;
    }
    if last_end < content.len() {
        result.push_str(&content[last_end..]);
    }

    Some(result)
}

pub(super) fn image_label_for_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim_matches(|ch: char| matches!(ch, '"' | '\'')).trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);
    let without_scheme = without_at.strip_prefix("file://").unwrap_or(without_at);
    let unescaped = unescape_whitespace(without_scheme);
    let path = Path::new(unescaped.as_str());
    if !is_image_path(path) {
        return None;
    }

    let label = path.file_name().and_then(|name| name.to_str()).unwrap_or(unescaped.as_str());
    Some(label.to_string())
}

pub(super) fn is_spinner_frame(indicator: &str) -> bool {
    matches!(indicator, "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧" | "⠇" | "⠏" | "-" | "\\" | "|" | "/" | ".")
}

pub(crate) fn status_requires_shimmer(text: &str) -> bool {
    // Case-insensitive contains without allocating a lowercased String.
    // This function is called up to 3× per TUI tick (4 Hz upkeep when idle,
    // 60 Hz while interacting or animating) from is_running_activity /
    // has_status_spinner / is_shimmer_active, so
    // avoiding the per-call String allocation matters.
    let trimmed = text.trim();
    // Git branch names may contain activity words. Recognize the existing
    // labelled and compact Git formats before interpreting free-form status.
    if is_git_status(trimmed) {
        return false;
    }
    let needles = [
        "running command:",
        "running tool:",
        "running:",
        "running ",
        "executing ",
        "drafting plan",
        "validating plan",
        "persisting plan",
        "preparing approval",
        "approval required",
        "permission required",
        "action required",
        "input required",
        "waiting for approval",
        "waiting for input",
        "blocked",
        "[blocked]",
        "recovery: tools disabled",
        "tools disabled",
        "ctrl+c",
        "/stop to stop",
    ];
    if needles.iter().any(|needle| contains_ignore_ascii_case(trimmed, needle)) {
        return true;
    }
    let Some((indicator, rest)) = trimmed.split_once(' ') else {
        return false;
    };
    if indicator.chars().count() != 1 || rest.trim().is_empty() {
        return false;
    }
    is_spinner_frame(indicator)
}

/// Data structure for input widget rendering
#[derive(Clone, Debug)]
pub struct InputWidgetData {
    pub text: Text<'static>,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub cursor_should_be_visible: bool,
    pub use_fake_cursor: bool,
    pub background_style: Style,
    pub default_style: Style,
}

pub(super) fn render_fake_cursor(buf: &mut Buffer, cursor_x: u16, cursor_y: u16) {
    if let Some(cell) = buf.cell_mut((cursor_x, cursor_y)) {
        let mut style = cell.style();
        style = style.add_modifier(Modifier::REVERSED);
        cell.set_style(style);
        if cell.symbol().is_empty() {
            cell.set_symbol(" ");
        }
    }
}

pub(super) fn char_index_to_byte_index(content: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }

    content
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(content.len())
}

pub(super) fn byte_index_to_char_index(content: &str, byte_index: usize) -> usize {
    // Clamp to the nearest char boundary so a mid-multi-byte `byte_index`
    // (e.g. from an out-of-sync selection range) never panics the slice.
    let mut safe = byte_index.min(content.len());
    while safe > 0 && !content.is_char_boundary(safe) {
        safe -= 1;
    }
    content[..safe].chars().count()
}

pub(super) fn compact_inline_segment(content: &str) -> String {
    content
        .chars()
        .map(|ch| if ch == '\n' || ch == '\r' { ' ' } else { ch })
        .collect()
}

/// Number of image tokens visible in composer text.
///
/// Counts clipboard `[Image #N]` placeholders, inline `data:image/…` payloads,
/// and image file-path matches. Used with the attachment count to decide when
/// the composer should collapse to a summary instead of showing full text.
pub(super) fn count_input_image_tokens(content: &str) -> usize {
    let placeholder_count = content.matches("[Image #").count();
    let inline_data_count = content.matches("data:image/").count();
    // Hotpath: the path regex requires a literal `.ext` suffix, so content
    // without `.` can never match — skip the engine for plain typing.
    let path_count = if content.contains('.') {
        IMAGE_PATH_INLINE_REGEX.captures_iter(content).count()
    } else {
        0
    };
    placeholder_count.saturating_add(inline_data_count).saturating_add(path_count)
}

/// Number of `@file` reference tokens in composer text.
pub(super) fn count_input_file_tokens(content: &str) -> usize {
    // Hotpath: `FileReference` is only assigned to `@`-led tokens, so content
    // without `@` needs no tokenization for this counter.
    if !content.contains('@') {
        return 0;
    }
    tokenize_input(content)
        .iter()
        .filter(|token| token.kind == InputTokenKind::FileReference)
        .count()
}

/// Effective image count for collapse decisions: the larger of visible text
/// tokens and live attachments, so orphaned-attachment payloads still collapse
/// while deleted placeholders do not double-count.
pub(super) fn effective_image_count(content: &str, attachment_count: usize) -> usize {
    count_input_image_tokens(content).max(attachment_count)
}

/// Measured size of composer (or pasted) text for collapse decisions.
///
/// Dimension key: `char_count` is Unicode scalar count, `line_count` is
/// logical `\n` lines, `image_count` covers `[Image #N]` + `data:image/` +
/// image paths (+ attachments via [`InputSizeMetrics::of_content`]),
/// `file_count` is `@file` tokens. Single source of truth so the paste gate,
/// the generic preview gate, and the paste-tracking gate cannot drift apart.
struct InputSizeMetrics {
    char_count: usize,
    line_count: usize,
    image_count: usize,
    file_count: usize,
}

impl InputSizeMetrics {
    fn of_text(text: &str) -> Self {
        Self {
            char_count: text.chars().count(),
            line_count: text.split('\n').count(),
            image_count: count_input_image_tokens(text),
            file_count: count_input_file_tokens(text),
        }
    }

    fn of_content(content: &str, attachment_count: usize) -> Self {
        Self {
            char_count: content.chars().count(),
            line_count: content.split('\n').count(),
            image_count: effective_image_count(content, attachment_count),
            file_count: count_input_file_tokens(content),
        }
    }

    fn should_collapse(&self) -> bool {
        self.line_count >= ui::INLINE_PASTE_COLLAPSE_LINE_THRESHOLD
            || self.char_count >= ui::INLINE_INPUT_COMPACT_CHAR_THRESHOLD
            || self.image_count >= ui::INLINE_INPUT_COMPACT_IMAGE_THRESHOLD
            || self.file_count >= ui::INLINE_INPUT_COMPACT_FILE_TOKEN_THRESHOLD
    }

    fn placeholder(&self) -> String {
        format_large_input_placeholder(self.char_count, self.line_count, self.image_count, self.file_count)
    }
}

/// Summary placeholder for large composer content.
///
/// Dimension key: `char_count` is Unicode scalar count, `line_count` is
/// logical `\n` lines, `image_count` covers `[Image #N]` + `data:image/` +
/// image paths + attachments, `file_count` is `@file` tokens. Keeps the
/// legacy `[Pasted Content N chars]` prefix so existing transcript/status
/// matching keeps working, appending line/image/file details when present.
pub(super) fn format_large_input_placeholder(
    char_count: usize,
    line_count: usize,
    image_count: usize,
    file_count: usize,
) -> String {
    let mut placeholder = format!("[Pasted Content {char_count} chars");
    if line_count > 1 {
        let _ = write!(placeholder, ", {line_count} lines");
    }
    if image_count > 0 {
        let label = if image_count == 1 { "image" } else { "images" };
        let _ = write!(placeholder, ", {image_count} {label}");
    }
    if file_count > 0 {
        let label = if file_count == 1 { "file" } else { "files" };
        let _ = write!(placeholder, ", {file_count} {label}");
    }
    placeholder.push(']');
    placeholder
}

/// Paste-range preview, extended beyond the legacy line-count gate.
///
/// Collapses when the pasted slice itself is large by lines, chars, images,
/// or file tokens. `before` keeps the last head-chars of the previous content
/// and each `after` line keeps its first head-chars, so surrounding context
/// stays visible but bounded while the pasted block becomes one marker.
pub(super) fn compact_paste_range_preview(
    content: &str,
    range: Option<std::ops::Range<usize>>,
) -> Option<CompactInputPreview> {
    let range = range?;
    if range.start >= range.end
        || range.end > content.len()
        || !content.is_char_boundary(range.start)
        || !content.is_char_boundary(range.end)
    {
        return None;
    }

    let pasted = &content[range.clone()];
    let pasted_metrics = InputSizeMetrics::of_text(pasted);
    if !pasted_metrics.should_collapse() {
        return None;
    }

    let head_chars = ui::INLINE_INPUT_COMPACT_PREVIEW_HEAD_CHARS;
    let before_src = &content[..range.start];
    let before = compact_inline_segment(&before_src[last_n_chars_start(before_src, head_chars)..]);
    let after_lines = content[range.end..]
        .split('\n')
        .map(|line| compact_inline_segment(&line[..first_n_chars_end(line, head_chars)]))
        .collect();
    Some(CompactInputPreview {
        before,
        placeholder: pasted_metrics.placeholder(),
        after_lines,
    })
}

/// Generic preview for large composer content without a qualifying paste range.
///
/// Covers typed large inputs, single-line floods (minified JSON/base64), and
/// token floods (many `[Image #N]` / `@file`). For char-large content shows a
/// truncated head, the summary placeholder, and a truncated tail so previous
/// content is summarized rather than rendered in full. For token-only floods
/// (short text but many images/files) shows just the placeholder to avoid
/// echoing the token list twice.
pub(super) fn generic_large_input_preview(content: &str, attachment_count: usize) -> Option<CompactInputPreview> {
    let metrics = InputSizeMetrics::of_content(content, attachment_count);
    if !metrics.should_collapse() {
        return None;
    }

    let head_chars = ui::INLINE_INPUT_COMPACT_PREVIEW_HEAD_CHARS;
    let tail_chars = ui::INLINE_INPUT_COMPACT_PREVIEW_TAIL_CHARS;
    let char_large = metrics.char_count > head_chars.saturating_add(tail_chars);
    let (before, after_lines) = if char_large {
        let before = compact_inline_segment(&content[..first_n_chars_end(content, head_chars)]);
        let tail = &content[last_n_chars_start(content, tail_chars)..];
        (before, vec![compact_inline_segment(tail)])
    } else {
        (String::new(), Vec::new())
    };
    Some(CompactInputPreview {
        before,
        placeholder: metrics.placeholder(),
        after_lines,
    })
}

/// Whether pasted text should be tracked as a collapsible block.
///
/// Mirrors the preview gate so char-large, image-heavy, and file-heavy pastes
/// collapse the same way line-heavy pastes already do.
pub(crate) fn should_track_compact_paste(pasted: &str) -> bool {
    InputSizeMetrics::of_text(pasted).should_collapse()
}

/// End byte index of the first `n` chars (char-boundary safe, no allocation).
pub(super) fn first_n_chars_end(content: &str, n: usize) -> usize {
    content.char_indices().nth(n).map_or(content.len(), |(idx, _)| idx)
}

/// Start byte index of the last `n` chars (char-boundary safe, no allocation).
pub(super) fn last_n_chars_start(content: &str, n: usize) -> usize {
    if n == 0 {
        return content.len();
    }
    match content.char_indices().rev().nth(n) {
        Some((idx, ch)) => idx + ch.len_utf8(),
        None => 0,
    }
}

pub(super) fn display_width_for_char_range(content: &str, char_count: usize) -> u16 {
    content
        .chars()
        .take(char_count)
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0) as u16)
        .fold(0_u16, u16::saturating_add)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_count_skips_regex_without_dot_but_keeps_memchr_branches() {
        // Arrange (asymmetric): plain typing has neither markers nor a dot;
        // the data-URI branch has a marker but no dot; uppercase ext has both.
        assert_eq!(count_input_image_tokens("just plain typing here"), 0);
        assert_eq!(count_input_image_tokens("data:image/png;base64,AAA"), 1);
        assert_eq!(count_input_image_tokens("see /tmp/photo.PNG here"), 1);
        assert_eq!(count_input_image_tokens("see /tmp/photo.png here"), 1);
    }

    #[test]
    fn file_count_skips_tokenize_without_at() {
        // Arrange (asymmetric): dot without `@` cannot yield FileReference;
        // `@` at a word boundary can; mid-word `@` cannot.
        assert_eq!(count_input_file_tokens("plain words with dot here"), 0);
        assert_eq!(count_input_file_tokens("open @src/main.rs now"), 1);
        assert_eq!(count_input_file_tokens("mail foo@bar here"), 0);
    }

    #[test]
    fn placeholders_skip_regex_without_dot_but_keep_real_paths() {
        // Arrange: no dot means no `.ext` suffix is possible; a dotted
        // non-image stays None through the post-gate regex path.
        assert_eq!(compact_image_placeholders("just plain typing"), None);
        assert_eq!(compact_image_placeholders("read notes.txt today"), None);
        assert!(compact_image_placeholders("see /tmp/photo.png here").is_some());
    }
}
