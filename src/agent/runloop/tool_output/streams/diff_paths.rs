//! Split from streams.rs; see module docs there.

use super::*;

/// Infer a syntax language hint from unified diff file headers.
///
/// Scans `diff --git`, `---`/`+++` markers, and `*** Update/Add/Delete File:`
/// apply-patch headers so generic diff rendering still gets per-language
/// syntax colors. Returns the lowercase extension (`rs`, …). Quoted paths
/// (`"a/my file.rs"`) are unquoted before inference.
pub(crate) fn diff_language_hint_from_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        if let Some(path) = git_b_path(trimmed) {
            if let Some(hint) = vtcode_commons::diff_paths::language_hint_from_path(&path) {
                return Some(hint);
            }
        }
        if let Some(path) = marker_path(trimmed) {
            if let Some(hint) = vtcode_commons::diff_paths::language_hint_from_path(&path) {
                return Some(hint);
            }
        }
        if let Some(path) = parse_apply_patch_path(trimmed) {
            if let Some(hint) = vtcode_commons::diff_paths::language_hint_from_path(&path) {
                return Some(hint);
            }
        }
    }
    None
}

/// New (`b/`) path from a `diff --git` line, handling quoted paths with
/// spaces (`"a/my file.rs" "b/my file.rs"`) that whitespace splitting mangles.
fn git_b_path(line: &str) -> Option<String> {
    let quoted: Vec<&str> = line.split('"').collect();
    if quoted.len() >= 4 {
        let new_path = quoted[3].trim();
        if !new_path.is_empty() {
            return Some(new_path.trim_start_matches("b/").to_string());
        }
    }
    parse_diff_git_path(line).map(unquote_diff_path)
}

/// Path from a `---`/`+++` marker, handling quoted paths with spaces.
fn marker_path(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.contains('"') {
        let quoted: Vec<&str> = trimmed.split('"').collect();
        if quoted.len() >= 3 {
            let path = quoted[1].trim();
            if !path.is_empty() && path != "/dev/null" {
                return Some(path.trim_start_matches("a/").trim_start_matches("b/").to_string());
            }
        }
    }
    parse_diff_marker_path(trimmed).map(unquote_diff_path)
}

fn unquote_diff_path(path: String) -> String {
    let trimmed = path.trim();
    if trimmed.len() >= 2
        && trimmed.starts_with('"')
        && trimmed.ends_with('"')
        && let Some(inner) = trimmed.get(1..trimmed.len().saturating_sub(1))
    {
        return inner.to_string();
    }
    trimmed.to_string()
}

fn parse_apply_patch_path(line: &str) -> Option<String> {
    for prefix in ["*** Update File:", "*** Add File:", "*** Delete File:"] {
        if let Some(path) = line.strip_prefix(prefix).map(str::trim).filter(|path| !path.is_empty()) {
            return Some(unquote_diff_path(path.to_string()));
        }
    }
    None
}

pub(crate) fn language_hint_for_display_line(line: &DiffDisplayLine) -> Option<String> {
    let text = line.text.trim_start();
    if let Some(path) = git_b_path(text) {
        if let Some(hint) = vtcode_commons::diff_paths::language_hint_from_path(&path) {
            return Some(hint);
        }
    }
    if let Some(path) = marker_path(text) {
        if let Some(hint) = vtcode_commons::diff_paths::language_hint_from_path(&path) {
            return Some(hint);
        }
    }
    if let Some(path) = parse_apply_patch_path(text) {
        return vtcode_commons::diff_paths::language_hint_from_path(&path);
    }
    None
}

/// Workspace-visible path from unified/apply-patch headers, when present.
fn file_path_from_diff_content(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim_start();
        for path in [
            git_b_path(trimmed),
            marker_path(trimmed),
            parse_apply_patch_path(trimmed),
        ]
        .into_iter()
        .flatten()
        {
            let path = path.trim();
            if !path.is_empty() && path != "/dev/null" && path != "file" {
                return Some(path.to_owned());
            }
        }
    }
    None
}

/// Parse `... N lines omitted ...` / `… +N lines …` omission copy.
pub(crate) fn parse_omitted_line_count(text: &str) -> Option<u64> {
    let idx = text.find("lines omitted").or_else(|| text.find("lines —"))?;
    let before = text[..idx]
        .trim()
        .trim_end_matches('…')
        .trim_end_matches('.')
        .trim_end_matches(' ')
        .trim_start_matches('…')
        .trim_start_matches('.')
        .trim_start_matches(' ')
        .trim_start_matches('+');
    let digits: String = before.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

pub(crate) fn is_generic_diff_review_label(path: &str) -> bool {
    vtcode_commons::ui_protocol::is_generic_diff_review_path(path)
}

pub(crate) fn resolve_diff_review_path(diff_content: &str) -> String {
    file_path_from_diff_content(diff_content)
        .filter(|path| !is_generic_diff_review_label(path))
        .unwrap_or_else(|| "diff".to_owned())
}

/// True when `diff_content` is itself a pre-truncated excerpt (registry preview).
pub(crate) fn is_pretruncated_diff_content(diff_content: &str) -> bool {
    diff_content.lines().any(|line| {
        line.contains("lines omitted") || line.contains("preview excerpt retained") || line.contains("diff truncated")
    })
}

/// Whether any laid-out body/metadata row would exceed the reflow safety cap.
pub(crate) fn line_exceeds_wrap_safety_cap(
    line: &DiffDisplayLine,
    line_number_width: usize,
    show_gutter: bool,
) -> bool {
    match line.kind {
        DiffDisplayKind::Addition | DiffDisplayKind::Deletion | DiffDisplayKind::Context => {
            display_width(&line.text) > DIFF_WRAP_SOURCE_MAX_WIDTH
        }
        _ => {
            let text = diff_display_text(line, line_number_width, show_gutter);
            display_width(&text) > DIFF_WRAP_SOURCE_MAX_WIDTH
        }
    }
}
