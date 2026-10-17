//! Tracker transcript rows, panel metadata, and rendering.
use anyhow::Result;
use serde_json::Value;
use vtcode_commons::ui_protocol::TaskItemStatus;
use vtcode_core::tools::handlers::task_tracking::{compact_task_tree_view_from_items, short_task_description};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::TaskPanelMetadata;

/// User-facing transcript surface for a tracker payload.
///
/// Successful checklists collapse to a header line (`• Release 2/5`) when
/// progress counts exist (explicit or derived from items), even if the tree
/// body is empty. Errors and empty trackers keep diagnostic lines. Row-level
/// detail travels through [`tracker_transcript_lines`] with typed statuses.
pub(crate) fn tracker_progress_lines(val: &Value) -> Vec<String> {
    if tracker_response_is_successful(val)
        && (!tracker_visible_tree_rows(val).is_empty() || tracker_progress_counts(val).is_some())
    {
        return vec![tracker_progress_header(val)];
    }
    let diagnostics = tracker_summary_lines(val);
    if diagnostics.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::with_capacity(diagnostics.len() + 1);
    lines.push("• Tasks".to_string());
    lines.extend(diagnostics);
    lines
}

/// Max tree rows shown inline in expanded transcript mode; overflow collapses
/// to a single `  … N more` row so large checklists stay bounded.
pub(crate) const TRACKER_TRANSCRIPT_MAX_ROWS: usize = 30;

/// Max bytes of one visible tracker row. A plan step can carry a long
/// `Action -> files: [...] -> verify: [...]` body; the row keeps only its
/// leading description so the TODO panel and the compact current row stay
/// scannable. The full step text remains in the structured checklist payload.
pub(crate) const TRACKER_ROW_DESCRIPTION_MAX_BYTES: usize = 96;

/// One user-facing tracker row: glyphless display text plus its typed status.
///
/// `status` is `None` for headers, diagnostics, and truncation rows, which
/// render with the default style. Carrying status alongside text (instead of
/// re-parsing glyphs) keeps styling exact after glyph removal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackerLine {
    pub(crate) text: String,
    pub(crate) status: Option<TaskItemStatus>,
}

impl TrackerLine {
    pub(crate) fn plain(text: String) -> Self {
        Self { text, status: None }
    }

    pub(crate) fn row(text: String, status: TaskItemStatus) -> Self {
        Self { text, status: Some(status) }
    }
}

/// One glyphless tree row with its typed status and leaf flag.
///
/// Parents summarize children and carry no status glyph; `leaf` distinguishes
/// them so the current-task picker prefers actionable leaf rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrackerRow {
    pub(crate) text: String,
    pub(crate) status: TaskItemStatus,
    pub(crate) leaf: bool,
}

/// Current-task marker shared by plan and todo tracker surfaces.
///
/// `»` replaces the old geometric `▶` so the focused row stays readable
/// on plain terminals while keeping a distinct single-glyph visual.
pub(crate) const TRACKER_CURRENT_MARKER: &str = "»";
/// Full current-task prefix (`marker + space`) for `strip_prefix`/`starts_with`.
pub(crate) const TRACKER_CURRENT_PREFIX: &str = "» ";

/// Transcript lines for a tracker payload, honoring display mode.
///
/// Status surfaces through text styling only (no leaf glyphs): done rows
/// render struck-through, the current row renders bold in the theme accent,
/// blocked rows use the warning token. Compact (`expanded = false`) shows
/// header plus the single current task row (`  » …`); expanded appends the
/// glyphless tree body (truncated). Errors and diagnostics never expand.
pub(crate) fn tracker_transcript_lines(val: &Value, expanded: bool) -> Vec<TrackerLine> {
    if !expanded {
        let progress = tracker_progress_lines(val);
        if progress.len() != 1 || !tracker_response_is_successful(val) {
            return progress.into_iter().map(TrackerLine::plain).collect();
        }
        let Some(current) = tracker_current_tree_row(val) else {
            return progress.into_iter().map(TrackerLine::plain).collect();
        };
        let header = progress.into_iter().next().unwrap_or_else(|| "• Tasks".to_string());
        return vec![TrackerLine::plain(header), current];
    }
    if !tracker_response_is_successful(val) {
        return tracker_progress_lines(val).into_iter().map(TrackerLine::plain).collect();
    }
    let progress = tracker_progress_lines(val);
    if progress.len() != 1 {
        return progress.into_iter().map(TrackerLine::plain).collect();
    }
    let tree = tracker_rich_tree_rows(val);
    if tree.is_empty() {
        return progress.into_iter().map(TrackerLine::plain).collect();
    }
    let header = progress.into_iter().next().unwrap_or_else(|| "• Tasks".to_string());
    let mut lines = Vec::with_capacity(1 + TRACKER_TRANSCRIPT_MAX_ROWS + 1);
    lines.push(TrackerLine::plain(header));
    if tree.len() > TRACKER_TRANSCRIPT_MAX_ROWS {
        let overflow = tree.len() - TRACKER_TRANSCRIPT_MAX_ROWS;
        lines.extend(
            tree.into_iter()
                .take(TRACKER_TRANSCRIPT_MAX_ROWS)
                .map(|row| TrackerLine::row(row.text, row.status)),
        );
        lines.push(TrackerLine::plain(format!("  … {overflow} more")));
    } else {
        lines.extend(tree.into_iter().map(|row| TrackerLine::row(row.text, row.status)));
    }
    lines
}

/// Panel body rows for a tracker payload: glyphless tree text only (no header).
///
/// Statuses travel separately via [`tracker_tree_body_statuses`]; the panel
/// maps them to theme styles per row.
pub(crate) fn tracker_tree_body_lines(val: &Value) -> Vec<String> {
    tracker_rich_tree_rows(val).into_iter().map(|row| row.text).collect()
}

/// Panel rows with typed statuses plus the focused-row index.
///
/// Returns `(texts, statuses, current)`: display lines for the panel body,
/// parallel statuses for per-row styling, and the index of the focused
/// current task (leaf-aware pick, same priority as the transcript) for accent
/// emphasis. The focused index is `None` when nothing is actionable.
pub(crate) fn tracker_panel_rows(val: &Value) -> (Vec<String>, Vec<TaskItemStatus>, Option<usize>) {
    let rows = tracker_rich_tree_rows(val);
    let current = tracker_current_row_index(&rows);
    let (texts, statuses) = rows.into_iter().map(|row| (row.text, row.status)).unzip();
    (texts, statuses, current)
}

/// Prefer actionable leaves, then status priority and original tree order.
fn tracker_current_row_index(rows: &[TrackerRow]) -> Option<usize> {
    for want_leaf in [true, false] {
        for status in [
            TaskItemStatus::InProgress,
            TaskItemStatus::Pending,
            TaskItemStatus::Blocked,
        ] {
            if let Some(index) = rows.iter().position(|row| row.status == status && row.leaf == want_leaf) {
                return Some(index);
            }
        }
    }
    None
}

/// Glyphless tree rows with typed statuses, in tree order.
///
/// Checklist items render through the shared compact tree formatter; the
/// leading status glyph is stripped so status surfaces through text styling
/// only. The legacy `view.lines` fallback derives status from its glyphs.
fn tracker_rich_tree_rows(val: &Value) -> Vec<TrackerRow> {
    let checklist_items = val
        .get("checklist")
        .and_then(Value::as_object)
        .and_then(|checklist| checklist.get("items"))
        .and_then(Value::as_array);
    let compact_rows = checklist_items
        .filter(|items| !items.is_empty())
        .map(|items| compact_task_tree_view_from_items(items))
        .unwrap_or_default();
    if compact_rows.is_empty() {
        return val
            .get("view")
            .and_then(Value::as_object)
            .and_then(|view| view.get("lines"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(visible_tracker_view_row)
                    .map(|display| match tracker_tree_row_glyph(&display) {
                        Some((glyph, _)) => TrackerRow {
                            text: tracker_row_text(strip_tracker_status_glyph(&display)),
                            status: task_status_from_glyph(glyph),
                            leaf: true,
                        },
                        // Glyph-free context rows are preserved (never dropped)
                        // with the neutral pending style, matching the legacy
                        // plain rendering.
                        None => TrackerRow {
                            text: tracker_row_text(display),
                            status: TaskItemStatus::Pending,
                            leaf: false,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
    }
    let mut rows = Vec::with_capacity(compact_rows.len());
    for row in &compact_rows {
        let Some(display) = visible_tracker_view_row(row) else {
            continue;
        };
        let status = row
            .get("status")
            .and_then(Value::as_str)
            .and_then(|raw| raw.parse::<TaskItemStatus>().ok())
            .unwrap_or(TaskItemStatus::Pending);
        let leaf = tracker_tree_row_glyph(&display).is_some();
        rows.push(TrackerRow {
            text: tracker_row_text(strip_tracker_status_glyph(&display)),
            status,
            leaf,
        });
    }
    rows
}

fn tracker_visible_tree_rows(val: &Value) -> Vec<String> {
    tracker_tree_body_lines(val)
}

/// Whether a transcript line is the compact-mode focused TODO row.
pub(crate) fn is_tracker_current_row(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with(TRACKER_CURRENT_PREFIX) || trimmed.starts_with("▶ ")
}

/// Compact-mode focused row: the single most actionable task.
///
/// Reuses the leaf-aware pick from [`tracker_panel_rows`] so transcript and
/// panel always agree on which task is focused. All-completed and empty
/// checklists return `None` so compact stays header-only. The returned row
/// uses the distinct `  » ` visual without a status glyph; styling carries
/// the status.
pub(crate) fn tracker_current_tree_row(val: &Value) -> Option<TrackerLine> {
    let rows = tracker_rich_tree_rows(val);
    let row = rows.get(tracker_current_row_index(&rows)?)?;
    Some(TrackerLine::row(format_tracker_current_row(&row.text), row.status))
}

/// Map a status glyph token to its typed status.
fn task_status_from_glyph(glyph: &str) -> TaskItemStatus {
    match glyph {
        "[x] " => TaskItemStatus::Completed,
        "[-] " => TaskItemStatus::InProgress,
        "[!] " => TaskItemStatus::Blocked,
        _ => TaskItemStatus::Pending,
    }
}

/// Split a tree display row into its status glyph and body, if it is a leaf.
///
/// Only the leading glyph token is inspected, so descriptions containing
/// bracket text mid-string (e.g. `Fix [-] flag handling`) are never mangled.
fn tracker_tree_row_glyph(display: &str) -> Option<(&str, &str)> {
    let mut rest = display.trim_start();
    loop {
        if let Some(after) = rest
            .strip_prefix("├ ")
            .or_else(|| rest.strip_prefix("└ "))
            .or_else(|| rest.strip_prefix("│ "))
        {
            rest = after;
            continue;
        }
        break;
    }
    for glyph in ["□ ", "[x] ", "[-] ", "[!] "] {
        if let Some(body) = rest.strip_prefix(glyph) {
            return Some((glyph, body));
        }
    }
    None
}

/// Strip the leading status glyph from a tree row, keeping branch structure.
///
/// `  ├ [-] Defer setup` → `  ├ Defer setup`. Parent rows and glyph-free
/// rows pass through unchanged.
/// Bound one visible tracker row to a single short description.
///
/// Applied after glyph stripping so the tree prefix and row structure survive;
/// only the trailing description is shortened to its leading clause and then
/// trimmed to the byte budget. Never lets a row wrap into multiple lines in
/// the panel or the compact current-task row.
pub(super) fn tracker_row_text(text: String) -> String {
    let (prefix, body) = split_tree_prefix_for_shortening(&text);
    let short = short_task_description(body);
    let base = if short.trim().is_empty() {
        body.trim()
    } else {
        short.trim()
    };
    let recombined = format!("{prefix}{base}");
    vtcode_commons::formatting::truncate_byte_budget(&recombined, TRACKER_ROW_DESCRIPTION_MAX_BYTES, "…")
}

/// Split a glyphless tree row into its branch prefix and description body so
/// shortening preserves `  ├ `/`  └ `/`  │ ` structure.
fn split_tree_prefix_for_shortening(text: &str) -> (&str, &str) {
    let mut index = 0;
    while index < text.len() && text.as_bytes()[index] == b' ' {
        index += 1;
    }
    loop {
        let rest = &text[index..];
        if let Some(after) = rest
            .strip_prefix("├ ")
            .or_else(|| rest.strip_prefix("└ "))
            .or_else(|| rest.strip_prefix("│ "))
        {
            index = text.len() - after.len();
            continue;
        }
        break;
    }
    text.split_at(index)
}

fn strip_tracker_status_glyph(display: &str) -> String {
    let Some((_, body)) = tracker_tree_row_glyph(display) else {
        return display.to_string();
    };
    let prefix_len = display.len() - body.len();
    let (prefix_with_glyph, _) = display.split_at(prefix_len);
    let glyph_len = ["□ ", "[x] ", "[-] ", "[!] "]
        .iter()
        .find_map(|glyph| prefix_with_glyph.strip_suffix(glyph).map(|_| glyph.len()))
        .unwrap_or(0);
    format!("{}{}", &prefix_with_glyph[..prefix_with_glyph.len() - glyph_len], body.trim_start())
}

/// Reframe a glyphless tree row with the distinct current-task visual.
fn format_tracker_current_row(glyphless_text: &str) -> String {
    let mut rest = glyphless_text.trim_start();
    loop {
        if let Some(after) = rest
            .strip_prefix("├ ")
            .or_else(|| rest.strip_prefix("└ "))
            .or_else(|| rest.strip_prefix("│ "))
        {
            rest = after;
            continue;
        }
        break;
    }
    format!("  {TRACKER_CURRENT_MARKER} {}", rest.trim_start())
}

/// Title + progress only — no next-step snippet, no tree rows.
fn tracker_progress_header(val: &Value) -> String {
    let Some((completed, total)) = tracker_progress_counts(val) else {
        return "• Tasks".to_string();
    };
    let label = resolve_tracker_title(val, "Tasks");
    format!("• {label} {completed}/{total}")
}

fn tracker_progress_counts(val: &Value) -> Option<(usize, usize)> {
    let checklist = val.get("checklist")?.as_object()?;
    if let (Some(completed), Some(total)) =
        (checklist.get("completed").and_then(Value::as_u64), checklist.get("total").and_then(Value::as_u64))
    {
        return Some((usize::try_from(completed).unwrap_or(usize::MAX), usize::try_from(total).unwrap_or(usize::MAX)));
    }
    let items = checklist.get("items")?.as_array()?;
    // Only count renderable tasks (non-empty description/text). Malformed
    // entries without descriptions must not produce misleading `0/N` counts.
    let renderable = items
        .iter()
        .filter(|item| {
            item.get("description")
                .and_then(Value::as_str)
                .or_else(|| item.get("text").and_then(Value::as_str))
                .is_some_and(|desc| !desc.trim().is_empty())
        })
        .collect::<Vec<_>>();
    if renderable.is_empty() {
        return None;
    }
    let total = renderable.len();
    let completed = renderable
        .iter()
        .filter(|item| item.get("status").and_then(Value::as_str) == Some("completed"))
        .count();
    Some((completed, total))
}

/// Humanize generated tracker titles (`1789108823046-kind-lagoon` → `Kind
/// Lagoon`) so the transcript header reads as a name instead of a file-stem
/// ID. Only millisecond-timestamp-prefixed slugs are rewritten; user titles
/// (`Release`, paths, sentences) pass through verbatim.
pub(crate) fn humanize_tracker_title(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Task tracker".to_string();
    }
    let mut parts = trimmed.splitn(2, ['-', '_']);
    let prefix = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();
    let is_generated_id = prefix.len() >= 10
        && prefix.chars().all(|character| character.is_ascii_digit())
        && !slug.is_empty()
        && slug
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if !is_generated_id {
        return trimmed.to_string();
    }
    let humanized = slug
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if humanized.is_empty() {
        trimmed.to_string()
    } else {
        humanized
    }
}

fn visible_tracker_view_row(value: &Value) -> Option<String> {
    let display = value.get("display").and_then(Value::as_str).or_else(|| value.as_str())?;
    let trimmed = display.trim_start();
    if trimmed.starts_with("files:") || trimmed.starts_with("outcome:") || trimmed.starts_with("verify:") {
        return None;
    }
    Some(display.to_string())
}

pub(crate) fn tracker_panel_metadata(val: &Value) -> Option<TaskPanelMetadata> {
    val.get("checklist").and_then(Value::as_object)?;
    let title = resolve_tracker_title(val, "Task tracker");
    let (completed, total) = tracker_progress_counts(val)?;
    Some(TaskPanelMetadata { title, completed, total })
}

/// Descriptive header title: never surface a generated plan codename.
///
/// Checklist titles created from approved plans already carry the plan-summary
/// clause, but older/manual payloads may still carry a timestamp slug
/// (`1789108823046-jolly-forest`) or its humanized form (`Jolly Forest`).
/// Those read as random names and say nothing about the work, so derive a
/// purpose-indicating title from the first checklist item instead. User titles
/// pass through verbatim (humanized only for timestamp slugs).
fn resolve_tracker_title(val: &Value, fallback_default: &str) -> String {
    let raw = val
        .get("checklist")
        .and_then(Value::as_object)
        .and_then(|checklist| checklist.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty());
    let Some(raw) = raw else {
        return first_item_descriptive_title(val).unwrap_or_else(|| fallback_default.to_string());
    };
    if is_generated_tracker_title(raw) {
        return first_item_descriptive_title(val).unwrap_or_else(|| fallback_default.to_string());
    }
    humanize_tracker_title(raw)
}

fn is_generated_tracker_title(raw: &str) -> bool {
    let trimmed = raw.trim();
    let mut parts = trimmed.splitn(2, ['-', '_']);
    let prefix = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();
    let is_timestamped =
        prefix.len() >= 10 && prefix.chars().all(|character| character.is_ascii_digit()) && !slug.is_empty();
    if is_timestamped {
        return true;
    }
    vtcode_commons::slug::is_humanized_codename(&humanize_tracker_title(trimmed))
}

/// Fallback descriptive title from the first actionable checklist item.
/// Bounds to the same 60-byte header budget used at tracker creation.
fn first_item_descriptive_title(val: &Value) -> Option<String> {
    let items = val.get("checklist")?.get("items")?.as_array()?;
    for item in items {
        let description = item
            .get("description")
            .and_then(Value::as_str)
            .or_else(|| item.get("text").and_then(Value::as_str))?;
        let short = short_task_description(description);
        let short = short.trim();
        if !short.is_empty() {
            return Some(vtcode_commons::formatting::truncate_byte_budget(short, 60, "…"));
        }
    }
    None
}

pub(super) fn render_tracker_view(renderer: &mut AnsiRenderer, val: &Value) -> Result<bool> {
    // Non-inline fallback honors display mode: compact shows header plus the
    // current task, expanded appends the truncated tree. Glyphless plain text;
    // status styling applies on the inline surface.
    let expanded = !renderer.is_compact_display();
    let lines: Vec<String> = tracker_transcript_lines(val, expanded)
        .into_iter()
        .map(|line| line.text)
        .collect();
    if lines.is_empty() {
        return Ok(false);
    }

    // Render through the markdown pipeline so inline formatting displays styled
    // instead of raw source on the single progress/diagnostic line.
    for line in lines {
        renderer.render_markdown_output(MessageStyle::ToolDetail, &line)?;
    }

    Ok(true)
}

pub(super) fn tracker_summary_lines(val: &Value) -> Vec<String> {
    let has_valid_checklist_items = val
        .get("checklist")
        .and_then(Value::as_object)
        .and_then(|checklist| checklist.get("items"))
        .and_then(Value::as_array)
        .is_some_and(|items| !compact_task_tree_view_from_items(items).is_empty());
    if has_valid_checklist_items && tracker_response_is_successful(val) {
        return Vec::new();
    }

    tracker_diagnostic_lines(val)
}

fn tracker_response_is_successful(val: &Value) -> bool {
    if val.get("error").is_some() || val.get("error_type").is_some() {
        return false;
    }

    match val.get("status").and_then(Value::as_str) {
        Some("created" | "replaced" | "updated" | "unchanged" | "ok" | "added") => true,
        Some(_) => false,
        None => true,
    }
}

fn tracker_diagnostic_lines(val: &Value) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(status) = val.get("status").and_then(Value::as_str)
        && !status.trim().is_empty()
    {
        lines.push(format!("  Tracker status: {status}"));
    }

    if let Some(message) = val.get("message").and_then(Value::as_str)
        && !message.trim().is_empty()
    {
        lines.push(format!("  Update: {message}"));
    }

    lines
}
