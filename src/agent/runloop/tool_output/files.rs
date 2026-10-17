use anyhow::Result;
use serde_json::Value;
use vtcode_commons::diff_paths::language_hint_from_path;
use vtcode_commons::preview;
use vtcode_core::config::ToolOutputMode;
use vtcode_core::config::constants::tools;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::render_tree_detail;
use super::streams::{diff_language_hint_from_content, render_diff_content_block_with_language, strip_ansi_codes};
use super::styles::{GitStyles, LsStyles};
use vtcode_core::tools::file_ops::{canonical_diff_previews, diff_preview_user_message};
pub(crate) use vtcode_diff::format_numbered_unified_diff as format_diff_content_lines_with_numbers;

/// Constants for line and content limits (compact display)
const MAX_DISPLAYED_FILES: usize = 100; // Limit displayed files to reduce clutter

/// Helper to extract optional string from JSON value
fn get_string<'a>(val: &'a Value, key: &str) -> Option<&'a str> {
    val.get(key).and_then(|v| v.as_str())
}

/// Helper to extract optional boolean from JSON value
fn get_bool(val: &Value, key: &str) -> bool {
    val.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Helper to extract optional u64 from JSON value
fn get_u64(val: &Value, key: &str) -> Option<u64> {
    val.get(key).and_then(|v| v.as_u64())
}

fn compact_file_glance_enabled(renderer: &AnsiRenderer) -> bool {
    renderer.supports_inline_ui() && renderer.is_compact_display()
}

fn render_file_heading(renderer: &mut AnsiRenderer, heading: &str) -> Result<()> {
    // File-op summaries are always `•` Info rows so single-file, multi-file,
    // compact, and expanded output share one intuitive hierarchy. Each Info
    // row is a transcript boundary (see vtcode-ui gotchas), which keeps
    // `• Edited …` scannable and prevents the heading from merging into the
    // surrounding tool-detail group.
    renderer.line(MessageStyle::Info, &format!("• {heading}"))
}

fn diff_action(diff: &Value) -> &'static str {
    match get_string(diff, "operation") {
        Some("created") => "Created",
        Some("deleted") => "Deleted",
        _ => "Edited",
    }
}

fn diff_path(diff: &Value) -> &str {
    get_string(diff, "path").filter(|path| !path.is_empty()).unwrap_or("file")
}

fn diff_counts(diff: &Value) -> (Option<u64>, Option<u64>) {
    let additions = get_u64(diff, "additions").or_else(|| {
        diff.get("summary")
            .and_then(|summary| summary.get("additions"))
            .and_then(Value::as_u64)
    });
    let deletions = get_u64(diff, "deletions").or_else(|| {
        diff.get("summary")
            .and_then(|summary| summary.get("deletions"))
            .and_then(Value::as_u64)
    });
    (additions, deletions)
}

/// Paint `text` bold when the renderer supports color, otherwise plain.
///
/// Uses the design-system foreground-only convention for file paths: bold
/// without a background so headers stay legible on all themes. The caller
/// passes `renderer.capabilities().supports_color()` so headings and diff
/// bodies share one color gate (see M5).
fn paint_bold(text: &str, color_enabled: bool) -> String {
    if text.is_empty() || !color_enabled {
        return text.to_string();
    }
    let style = anstyle::Style::new().bold();
    format!("{style}{text}{}", anstyle::Reset)
}

/// Paint `text` with a diff foreground when the renderer supports color.
fn paint_diff_count(text: &str, color: anstyle::Color, color_enabled: bool) -> String {
    if text.is_empty() || !color_enabled {
        return text.to_string();
    }
    let style = anstyle::Style::new().fg_color(Some(color));
    format!("{style}{text}{}", anstyle::Reset)
}

fn styled_count_suffix(
    additions: Option<u64>,
    deletions: Option<u64>,
    git_styles: &GitStyles,
    color_enabled: bool,
) -> String {
    if additions.is_none() && deletions.is_none() {
        return String::new();
    }
    let additions = additions.unwrap_or_default();
    let deletions = deletions.unwrap_or_default();
    let added = paint_diff_count(&format!("+{additions}"), git_styles.addition_fg, color_enabled);
    let removed = paint_diff_count(&format!("-{deletions}"), git_styles.deletion_fg, color_enabled);
    format!(" ({added} {removed})")
}

fn styled_diff_heading(diff: &Value, git_styles: &GitStyles, color_enabled: bool) -> String {
    let (additions, deletions) = diff_counts(diff);
    let path = paint_bold(diff_path(diff), color_enabled);
    format!(
        "{} {}{}",
        diff_action(diff),
        path,
        styled_count_suffix(additions, deletions, git_styles, color_enabled)
    )
}

fn styled_aggregate_summary(diffs: &[Value], git_styles: &GitStyles, color_enabled: bool) -> String {
    let action = diffs
        .first()
        .map(diff_action)
        .filter(|first| diffs.iter().all(|diff| diff_action(diff) == *first))
        .unwrap_or("Edited");
    let (additions, deletions, has_counts) = diffs.iter().fold((0_u64, 0_u64, false), |summary, diff| {
        let (diff_additions, diff_deletions) = diff_counts(diff);
        (
            summary.0.saturating_add(diff_additions.unwrap_or_default()),
            summary.1.saturating_add(diff_deletions.unwrap_or_default()),
            summary.2 || diff_additions.is_some() || diff_deletions.is_some(),
        )
    });
    let file_label = if diffs.len() == 1 { "file" } else { "files" };
    let suffix = if has_counts {
        styled_count_suffix(Some(additions), Some(deletions), git_styles, color_enabled)
    } else {
        String::new()
    };
    format!("{action} {} {file_label}{suffix}", diffs.len())
}

fn styled_child_row(diff: &Value, branch: &str, git_styles: &GitStyles, color_enabled: bool) -> String {
    let (additions, deletions) = diff_counts(diff);
    let path = paint_bold(diff_path(diff), color_enabled);
    format!("  {branch} {path}{}", styled_count_suffix(additions, deletions, git_styles, color_enabled))
}

/// Remove leading unified-diff file headers so the transcript never repeats
/// the path shown in the surrounding `• Edited path` heading.
///
/// Canonical per-file previews carry one file's unified diff. Rendering both
/// the heading and the raw `diff --git` / `index` / `--- a/path` / `+++ b/path`
/// lines repeats the same path three times and reads as blank duplication.
/// Hunk headers (`@@`), `+/-` bodies, context lines, and `\ No newline`
/// markers are always preserved.
///
/// Only the leading header block (before the first `@@` hunk or `+/-` body)
/// is stripped. Headers are matched on ANSI-stripped text at column 0 so
/// colored previews strip identically to plain ones, and every known header
/// shape (`diff --git`, `index`, `---`/`+++`, mode lines, rename/copy lines,
/// `Binary … differs`, and `*** … File:` apply-patch markers) is removed
/// unconditionally — the heading already identifies the file, so keeping any
/// of them would reintroduce duplication on absolute/quoted paths.
/// Context lines such as `" index = 0"` (leading space) are body content and
/// are never removed.
fn strip_redundant_file_headers(content: &str) -> String {
    // Cheap ANSI scan: only strip when an escape is present so plain diffs
    // avoid an extra allocation.
    fn plain_line(line: &str) -> std::borrow::Cow<'_, str> {
        if line.contains('\x1b') {
            strip_ansi_codes(line)
        } else {
            std::borrow::Cow::Borrowed(line)
        }
    }

    fn is_file_header(plain: &str) -> bool {
        plain.starts_with("diff --git ")
            || plain.starts_with("diff --combined ")
            || plain.starts_with("index ")
            || plain.starts_with("--- ")
            || plain.starts_with("+++ ")
            || plain.starts_with("new file mode ")
            || plain.starts_with("deleted file mode ")
            || plain.starts_with("old mode ")
            || plain.starts_with("new mode ")
            || plain.starts_with("similarity index ")
            || plain.starts_with("dissimilarity index ")
            || plain.starts_with("rename from ")
            || plain.starts_with("rename to ")
            || plain.starts_with("copy from ")
            || plain.starts_with("copy to ")
            || plain.starts_with("Binary ")
            || plain.starts_with("*** Update File:")
            || plain.starts_with("*** Add File:")
            || plain.starts_with("*** Delete File:")
            || plain.starts_with("*** Begin Patch")
            || plain.starts_with("*** End Patch")
    }

    let mut kept = Vec::new();
    let mut in_header = true;
    for line in content.lines() {
        if in_header {
            let plain = plain_line(line);
            // Hunk header ends the file-header block; everything after is body.
            if plain.starts_with("@@") {
                in_header = false;
                kept.push(line);
                continue;
            }
            // `+/-` bodies (excluding `---`/`+++` markers handled above) also
            // end the header block. Context lines start with a space and
            // `\ No newline` markers are body trailers.
            if plain.starts_with('\\')
                || (plain.starts_with('+') && !plain.starts_with("+++ "))
                || (plain.starts_with('-') && !plain.starts_with("--- "))
                || plain.starts_with(' ')
            {
                in_header = false;
                kept.push(line);
                continue;
            }
            if plain.trim().is_empty() || is_file_header(&plain) {
                continue;
            }
            kept.push(line);
            continue;
        }
        kept.push(line);
    }
    // Drop leading blank lines left behind by header stripping so the first
    // visible row is the hunk header, not empty whitespace.
    let first_content = kept.iter().position(|line| !line.trim().is_empty()).unwrap_or(kept.len());
    let mut result = kept[first_content..].join("\n");
    // Preserve the trailing newline convention of unified diffs.
    if content.ends_with('\n') && !result.is_empty() && !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn render_diff_entry_details(
    renderer: &mut AnsiRenderer,
    diff: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    // Stable `reason` codes are never surfaced directly; render the friendly
    // `diff_preview_user_message` instead (see `diff_preview.rs`).
    if get_bool(diff, "skipped") {
        let reason = diff_preview_user_message(diff);
        if let Some(detail) = get_string(diff, "detail") {
            render_tree_detail(renderer, &format!("preview: {reason} ({detail})"))?;
        } else {
            render_tree_detail(renderer, &format!("preview: {reason}"))?;
        }
        return Ok(());
    }

    let diff_content = get_string(diff, "content").unwrap_or("");
    if diff_content.is_empty() {
        if get_bool(diff, "is_empty") {
            render_tree_detail(renderer, "no changes")?;
            return Ok(());
        }
        // Empty non-`is_empty` previews render no body here, but fall through
        // to the truncation notice below so omission metadata is preserved.
    } else {
        let path = diff_path(diff);
        let trimmed = strip_redundant_file_headers(diff_content);
        // Header-only previews (no hunks/bodies) carry no visible changes.
        // Render a friendly row instead of echoing the duplicated `---`/`+++`
        // markers that the heading already shows.
        if trimmed.trim().is_empty() {
            render_tree_detail(renderer, "no changes")?;
        } else {
            render_diff_content(renderer, trimmed.as_str(), path, git_styles, ls_styles)?;
        }
    }

    if get_bool(diff, "truncated") {
        let omitted = get_u64(diff, "omitted_line_count").unwrap_or(0);
        let inline_tui = renderer.prefers_untruncated_output();
        // Registry previews are already head/tail excerpts when truncated.
        // Never advertise full-diff review from an excerpt body.
        if !inline_tui {
            if omitted > 0 {
                render_tree_detail(renderer, &format!("… +{omitted} lines (use exec_command with sed for full view)"))?;
            } else {
                render_tree_detail(renderer, "… diff truncated")?;
            }
            return Ok(());
        }
        if omitted > 0 {
            render_tree_detail(renderer, &format!("… +{omitted} lines omitted (preview excerpt retained)"))?;
        } else {
            render_tree_detail(renderer, "… diff truncated")?;
        }
        return Ok(());
    }
    Ok(())
}

pub(crate) fn render_write_file_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let diffs = canonical_diff_previews(payload);

    // Created files without a diff still get the shared `•` summary row so
    // create/write/edit/patch headings stay scannable as one hierarchy.
    if get_bool(payload, "created") && diffs.is_empty() {
        let heading =
            get_string(payload, "path").map_or_else(|| "File created".to_string(), |path| format!("Created {path}"));
        render_file_heading(renderer, &heading)?;
    }

    if let Some(encoding) = get_string(payload, "encoding") {
        render_tree_detail(renderer, &format!("encoding: {encoding}"))?;
    }

    if diffs.is_empty() {
        if get_bool(payload, "skipped") {
            let reason = get_string(payload, "reason").unwrap_or("already exists");
            render_tree_detail(renderer, &format!("write skipped: {reason}"))?;
        } else if get_bool(payload, "conflict") {
            render_tree_detail(renderer, "write blocked: file conflict")?;
        } else if let Some(error) = get_string(payload, "error") {
            renderer.line(MessageStyle::ToolError, error)?;
        }
        return Ok(());
    }

    render_diff_preview_entries(renderer, &diffs, git_styles, ls_styles)
}

pub(crate) fn render_apply_patch_diff_preview(
    renderer: &mut AnsiRenderer,
    payload: &Value,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let diffs = canonical_diff_previews(payload);
    if diffs.is_empty() {
        if get_bool(payload, "conflict") {
            render_tree_detail(renderer, "patch blocked: file conflict")?;
        } else if let Some(error) = get_string(payload, "error") {
            renderer.line(MessageStyle::ToolError, error)?;
        }
        return Ok(());
    }

    render_diff_preview_entries(renderer, &diffs, git_styles, ls_styles)
}

fn render_diff_preview_entries(
    renderer: &mut AnsiRenderer,
    diffs: &[Value],
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let color_enabled = renderer.capabilities().supports_color();
    let visible_diffs = &diffs[..diffs.len().min(MAX_DISPLAYED_FILES)];
    if compact_file_glance_enabled(renderer) && visible_diffs.len() > 1 {
        render_file_heading(renderer, &styled_aggregate_summary(visible_diffs, git_styles, color_enabled))?;
        for (index, diff) in visible_diffs.iter().enumerate() {
            let branch = if index + 1 == visible_diffs.len() { "└" } else { "├" };
            renderer.line(MessageStyle::Info, &styled_child_row(diff, branch, git_styles, color_enabled))?;
            render_diff_entry_details(renderer, diff, git_styles, ls_styles)?;
        }
    } else {
        for diff in visible_diffs {
            render_file_heading(renderer, &styled_diff_heading(diff, git_styles, color_enabled))?;
            render_diff_entry_details(renderer, diff, git_styles, ls_styles)?;
        }
    }

    let omitted = diffs.len().saturating_sub(visible_diffs.len());
    if omitted > 0 {
        render_tree_detail(renderer, &format!("… +{omitted} more files not shown"))?;
    }

    Ok(())
}

pub(crate) fn render_list_dir_output(renderer: &mut AnsiRenderer, val: &Value, _ls_styles: &LsStyles) -> Result<()> {
    // Get pagination info first
    let count = get_u64(val, "count").unwrap_or(0);
    let total = get_u64(val, "total").unwrap_or(0);
    let page = get_u64(val, "page").unwrap_or(1);
    let _has_more = get_bool(val, "has_more");
    let per_page = get_u64(val, "per_page").unwrap_or(20);

    // Show path - always display root directory for clarity
    if let Some(path) = get_string(val, "path") {
        let display_path = if path.is_empty() { "/" } else { path };
        renderer
            .line(MessageStyle::ToolDetail, &format!("{}{}", display_path, if !path.is_empty() { "/" } else { "" }))?;
    }

    // Show summary as a tree detail so list/read/edit summaries share one
    // `  └ …` hierarchy under the tool header.
    if count > 0 || total > 0 {
        let start_idx = (page - 1) * per_page + 1;
        let _end_idx = start_idx + count - 1;

        // Simplified summary without pagination details that confuse the agent
        let summary = if total > count {
            format!("Showing {count} of {total} items")
        } else {
            format!("{count} items total")
        };
        render_tree_detail(renderer, &summary)?;
    }

    // Render items grouped by type
    if let Some(items) = val.get("items").and_then(|v| v.as_array()) {
        if items.is_empty() {
            render_tree_detail(renderer, "empty")?;
        } else {
            let mut directories = Vec::new();
            let mut files = Vec::new();

            // Group items by type
            for item in items.iter().take(MAX_DISPLAYED_FILES) {
                if let Some(name) = get_string(item, "name") {
                    let item_type = get_string(item, "type").unwrap_or("file");
                    let size = get_u64(item, "size");

                    if item_type == "directory" {
                        directories.push((name.to_string(), size));
                    } else {
                        files.push((name.to_string(), size));
                    }
                }
            }

            // Get sort order from the JSON value, defaulting to alphabetical by name
            let sort_order = get_string(val, "sort").unwrap_or("name");

            // Sort each group based on the specified sort order
            match sort_order {
                "size" => {
                    // Sort by size (largest first), with None sizes treated as 0
                    directories.sort_by_key(|a| std::cmp::Reverse(a.1.unwrap_or(0)));
                    files.sort_by_key(|a| std::cmp::Reverse(a.1.unwrap_or(0)));
                }
                "name" => {
                    // Sort alphabetically (case-insensitive for natural sorting)
                    directories.sort_by_key(|a| a.0.to_lowercase());
                    files.sort_by_key(|a| a.0.to_lowercase());
                }
                "type" => {
                    // Sort by type/extension (files with extensions first, then by extension)
                    directories.sort_by_key(|a| a.0.to_lowercase());
                    files.sort_by(|a, b| {
                        let ext_a = std::path::Path::new(&a.0)
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();
                        let ext_b = std::path::Path::new(&b.0)
                            .extension()
                            .map(|e| e.to_string_lossy().to_lowercase())
                            .unwrap_or_default();

                        ext_a.cmp(&ext_b).then(a.0.to_lowercase().cmp(&b.0.to_lowercase()))
                    });
                }
                _ => {
                    // Default to alphabetical sorting
                    directories.sort_by_key(|a| a.0.to_lowercase());
                    files.sort_by_key(|a| a.0.to_lowercase());
                }
            }

            // Calculate max name width for directories (with trailing /) and files
            let max_name_width = if !directories.is_empty() || !files.is_empty() {
                let dir_max_width = directories
                    .iter()
                    .map(|(name, _)| preview::display_width(name) + 1) // +1 for trailing /
                    .max()
                    .unwrap_or(10)
                    .min(40);

                let file_max_width = files
                    .iter()
                    .map(|(name, _)| preview::display_width(name))
                    .max()
                    .unwrap_or(10)
                    .min(40);

                dir_max_width.max(file_max_width)
            } else {
                10 // Default width if no items
            };

            // Render directories first with section header
            if !directories.is_empty() {
                renderer.line(MessageStyle::ToolDetail, "[Directories]")?;
                for (name, _size) in &directories {
                    let name_with_slash = format!("{name}/");
                    let display = preview::pad_to_display_width(&name_with_slash, max_name_width, ' ');
                    renderer.line(MessageStyle::ToolDetail, &display)?;
                }

                // Add visual separation between directories and files
                if !files.is_empty() {
                    renderer.line(MessageStyle::ToolDetail, "")?; // Add blank line
                }
            }

            // Render files with section header
            if !files.is_empty() {
                renderer.line(MessageStyle::ToolDetail, "[Files]")?;
                for (name, _size) in &files {
                    // Simple file name display without size or emoji
                    let display = preview::pad_to_display_width(name, max_name_width, ' ');
                    renderer.line(MessageStyle::ToolDetail, &display)?;
                }
            }

            let omitted = items.len().saturating_sub(MAX_DISPLAYED_FILES);
            if omitted > 0 {
                render_tree_detail(renderer, &format!("… +{omitted} more items not shown"))?;
            }
        }
    }

    // Pagination navigation removed - agent should work with first page results
    // If more items exist, agent can call list_files again with specific page parameter

    Ok(())
}

pub(crate) fn render_read_file_output(renderer: &mut AnsiRenderer, val: &Value) -> Result<()> {
    // Batch read: show compact per-file summary
    if let Some(items) = val.get("items").and_then(Value::as_array) {
        let files_read = get_u64(val, "files_read").unwrap_or(items.len() as u64);
        let files_ok = get_u64(val, "files_succeeded").unwrap_or(0);
        let failed = files_read.saturating_sub(files_ok);

        let mut summary = format!("{} file{} read", files_ok, if files_ok == 1 { "" } else { "s" });
        if failed > 0 {
            summary.push_str(&format!(", {failed} failed"));
        }
        render_tree_detail(renderer, &summary)?;

        for item in items.iter().take(MAX_BATCH_DISPLAY_FILES) {
            if let Some(fp) = item.get("file_path").and_then(Value::as_str) {
                let short = shorten_path(fp, 60);
                if item.get("error").is_some() {
                    renderer.line(MessageStyle::ToolError, &format!("  ✗ {short}"))?;
                } else {
                    let lines_info = item
                        .get("ranges")
                        .and_then(Value::as_array)
                        .map(|ranges| {
                            let total_lines: u64 =
                                ranges.iter().filter_map(|r| r.get("lines_read").and_then(Value::as_u64)).sum();
                            format!(" ({total_lines} lines)")
                        })
                        .unwrap_or_default();
                    renderer.line(MessageStyle::ToolDetail, &format!("  ✓ {short}{lines_info}"))?;
                }
            }
        }
        if items.len() > MAX_BATCH_DISPLAY_FILES {
            renderer.line(MessageStyle::ToolDetail, &format!("  … +{} more", items.len() - MAX_BATCH_DISPLAY_FILES))?;
        }
        return Ok(());
    }

    // Single file read: show summary line
    let lines_read = get_u64(val, "lines_read");
    let start_line = get_u64(val, "start_line");
    let end_line = get_u64(val, "end_line");
    let has_more = val.get("has_more").and_then(Value::as_bool).unwrap_or(false);

    let summary = if let Some(n) = lines_read {
        if has_more {
            format!("Read {n} lines (more available)")
        } else {
            format!("Read {n} lines")
        }
    } else if let (Some(start), Some(end)) = (start_line, end_line) {
        let count = end.saturating_sub(start) + 1;
        format!("Read lines {start}-{end} ({count} lines)")
    } else if let Some(content) = get_string(val, "content") {
        let count = content.lines().count();
        format!("Read {count} lines")
    } else {
        return Ok(());
    };
    render_tree_detail(renderer, &summary)?;

    Ok(())
}

const MAX_BATCH_DISPLAY_FILES: usize = 10;

fn shorten_path(path: &str, max_len: usize) -> String {
    if preview::display_width(path) <= max_len {
        return path.to_string();
    }
    if let Some(name) = std::path::Path::new(path).file_name() {
        let name_str = name.to_string_lossy();
        if let Some(parent) = std::path::Path::new(path).parent() {
            let parent_str = parent.to_string_lossy();
            let reserved = preview::display_width(name_str.as_ref()) + 2; // ellipsis + slash
            let budget = max_len.saturating_sub(reserved);
            if budget > 0 && preview::display_width(parent_str.as_ref()) > budget {
                let parent_tail = preview::suffix_for_display_width(parent_str.as_ref(), budget);
                return format!("…{parent_tail}/{name_str}");
            }
        }
        return name_str.to_string();
    }
    preview::truncate_to_display_width(path, max_len).to_string()
}

/// Render diff bodies with the design-system diff treatment.
///
/// `file_path` supplies the syntax language hint (`rs`, `ts`, …) so diff
/// bodies keep the soft add/delete row tint plus stronger intraline chips
/// while code tokens carry syntax foregrounds. Prose (`md`/`txt`) and unknown
/// extensions stay solid-tinted by design; ANSI16 and no-color remain
/// foreground-only. File headers are stripped by the caller, so this starts
/// at the first hunk — no duplicated `---`/`+++` rows. When the path carries
/// no extension (e.g. `Makefile`, fallback `"file"`), fall back to inferring
/// from the diff headers so pathless previews keep their syntax.
fn render_diff_content(
    renderer: &mut AnsiRenderer,
    diff_content: &str,
    file_path: &str,
    git_styles: &GitStyles,
    ls_styles: &LsStyles,
) -> Result<()> {
    let plain_diff = strip_ansi_codes(diff_content);
    let language = language_hint_from_path(file_path).or_else(|| diff_language_hint_from_content(plain_diff.as_ref()));
    render_diff_content_block_with_language(
        renderer,
        plain_diff.as_ref(),
        Some(tools::WRITE_FILE),
        git_styles,
        ls_styles,
        MessageStyle::ToolDetail,
        ToolOutputMode::Compact,
        usize::MAX,
        language.as_deref(),
    )
}

pub(super) fn colorize_diff_summary_line(line: &str, _supports_color: bool) -> Option<String> {
    let trimmed = line.trim_start();
    let is_summary = trimmed.contains(" file changed")
        || trimmed.contains(" files changed")
        || trimmed.contains(" insertion(+)")
        || trimmed.contains(" insertions(+)")
        || trimmed.contains(" deletion(-)")
        || trimmed.contains(" deletions(-)");
    if is_summary { Some(line.to_string()) } else { None }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;
