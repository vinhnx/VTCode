use crate::exec::events::{
    AgentMessageItem, ErrorItem, ItemCompletedEvent, ItemStartedEvent, ItemUpdatedEvent, ReasoningItem, ThreadEvent,
    ThreadItem, ThreadItemDetails, ToolCallStatus, ToolInvocationItem, ToolOutcome, ToolOutputItem,
    tool_outcome_from_status,
};
use crate::tools::file_ops::{canonical_diff_previews, diff_preview_user_message};
use serde_json::Value;
use std::collections::HashMap;
use std::fmt::Write;

#[derive(Debug, Clone, Default)]
struct StreamingTextState {
    item_id: Option<String>,
    text: String,
    started: bool,
}

#[derive(Debug, Clone)]
struct ToolCallStreamState {
    item_id: String,
    name: Option<String>,
    arguments: String,
    started: bool,
    /// Bytes of `arguments` included in the last emitted `item.updated` event.
    /// Used to throttle per-token delta emissions — see `MIN_TOOL_ARG_UPDATE_BYTES`.
    last_emitted_args_len: usize,
    /// Number of intermediate `item.updated` events emitted for this tool call.
    /// Capped at `MAX_TOOL_ARG_UPDATE_EVENTS` to bound log growth for large arguments.
    update_events: usize,
}

/// Minimum accumulated bytes of tool-call arguments between two `item.updated`
/// events. Without this, every streaming argument delta (one per token) emits a
/// full-arguments update, producing ~30+ events per tool call and bloating the
/// session log (observed 3069 `item.updated` events for 98 tool outputs in a
/// single 3-turn session). The final `complete_tool_call` always emits the full
/// arguments, so intermediate updates are progress hints only.
const MIN_TOOL_ARG_UPDATE_BYTES: usize = 512;

/// Maximum intermediate `item.updated` events per tool call. Once reached, no
/// further streaming updates are emitted until the tool call completes.
const MAX_TOOL_ARG_UPDATE_EVENTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Aggregated output from a tool execution, containing either inline text or a
/// spool file reference.
pub struct ToolOutputPayload {
    /// Combined output text from the tool execution.
    pub aggregated_output: String,
    /// Optional path to a spool file containing the full output.
    pub spool_path: Option<String>,
}

fn pluralize<'a>(count: u64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
}

/// Attribute only a successful file tool's recorded result, never current Git state.
pub fn file_change_completed_event(
    call_id: &str,
    tool_name: &str,
    args: &Value,
    output: &Value,
) -> Option<ThreadEvent> {
    use crate::exec::events::{FileChangeItem, FileUpdateChange, PatchApplyStatus, PatchChangeKind};
    let file_tool =
        matches!(tool_name, "apply_patch" | "write_file" | "edit_file" | "create_file" | "delete_file" | "file")
            || tool_name == crate::config::constants::tools::UNIFIED_FILE;
    if !file_tool
        || !crate::tools::tool_intent::classify_tool_intent(tool_name, args).mutating
        || output.get("success").and_then(Value::as_bool) == Some(false)
        || output.get("skipped").and_then(Value::as_bool) == Some(true)
        || output.get("conflict").and_then(Value::as_bool) == Some(true)
        || crate::tools::file_ops::diff_output_has_effective_change(output) == Some(false)
    {
        return None;
    }
    let previews = canonical_diff_previews(output);
    let mut changes = Vec::new();
    let mut diffs = Vec::new();
    let mut additions = 0u64;
    let mut deletions = 0u64;
    let mut counts_available = true;
    let mut diff_incomplete = previews.is_empty();
    for preview in &previews {
        if preview.get("is_empty").and_then(Value::as_bool) == Some(true)
            && !matches!(preview.get("operation").and_then(Value::as_str), Some("created" | "deleted"))
        {
            continue;
        }
        let Some(path) = preview.get("path").and_then(Value::as_str) else {
            continue;
        };
        let kind = match preview.get("operation").and_then(Value::as_str) {
            Some("created" | "added") => PatchChangeKind::Add,
            Some("deleted" | "removed") => PatchChangeKind::Delete,
            _ => PatchChangeKind::Update,
        };
        changes.push(FileUpdateChange { path: path.to_owned(), kind });
        diff_incomplete |= preview.get("truncated").and_then(Value::as_bool) == Some(true)
            || preview.get("skipped").and_then(Value::as_bool) == Some(true)
            || preview.get("content").and_then(Value::as_str).is_none();
        if let Some(content) = preview.get("content").and_then(Value::as_str) {
            diffs.push(content.to_owned());
        }
        if let (Some(a), Some(d)) = (
            preview
                .get("additions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("additions")))
                .and_then(Value::as_u64),
            preview
                .get("deletions")
                .or_else(|| preview.get("summary").and_then(|s| s.get("deletions")))
                .and_then(Value::as_u64),
        ) {
            additions = additions.saturating_add(a);
            deletions = deletions.saturating_add(d);
        } else {
            counts_available = false;
        }
    }
    if changes.is_empty()
        && let Some(paths) = output.get("modified_files").and_then(Value::as_array)
    {
        counts_available = false;
        for path in paths.iter().filter_map(Value::as_str) {
            changes.push(FileUpdateChange {
                path: path.to_owned(),
                kind: PatchChangeKind::Update,
            });
        }
    }
    if changes.is_empty() {
        return None;
    }
    Some(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            id: format!("{call_id}:changes"),
            context: None,
            details: ThreadItemDetails::FileChange(Box::new(FileChangeItem {
                diff_incomplete: diff_incomplete.then_some(true),
                changes,
                status: PatchApplyStatus::Completed,
                unified_diff: (!diffs.is_empty()).then(|| diffs.join("\n")),
                additions: counts_available.then_some(additions),
                deletions: counts_available.then_some(deletions),
            })),
        },
    }))
}

fn trimmed_string_field<'a>(output: &'a Value, key: &str) -> Option<&'a str> {
    output
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

fn diff_preview_output(output: &Value) -> Option<String> {
    let diffs = canonical_diff_previews(output);
    if diffs.is_empty() {
        return None;
    }
    let mut sections = Vec::new();

    for diff in &diffs {
        let path = diff.get("path").and_then(Value::as_str).unwrap_or("file");
        let operation = diff
            .get("operation")
            .and_then(Value::as_str)
            .map(|operation| format!(" ({operation})"))
            .unwrap_or_default();
        let counts =
            match (diff.get("additions").and_then(Value::as_u64), diff.get("deletions").and_then(Value::as_u64)) {
                (Some(additions), Some(deletions)) if additions > 0 || deletions > 0 => {
                    format!(" (+{additions} -{deletions})")
                }
                _ => String::new(),
            };
        if let Some(content) = diff
            .get("content")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|content| !content.is_empty())
        {
            sections.push(format!("diff preview for {path}{operation}{counts}:\n{content}"));
            continue;
        }

        if diff.get("skipped").and_then(Value::as_bool) == Some(true) {
            let reason = diff_preview_user_message(diff);
            sections.push(format!("diff preview for {path}{operation}{counts}: {reason}"));
            continue;
        }

        if diff.get("is_empty").and_then(Value::as_bool) == Some(true) {
            sections.push(format!("diff preview for {path}{operation}: no changes"));
        }
    }

    (!sections.is_empty()).then(|| sections.join("\n"))
}

#[cold]
fn trimmed_error_message(output: &Value) -> Option<&str> {
    match output.get("error") {
        Some(Value::String(message)) => Some(message.as_str()),
        Some(Value::Object(error)) => error.get("message").and_then(Value::as_str),
        _ => None,
    }
    .map(str::trim)
    .filter(|text| !text.is_empty())
}

fn sample_strings_from_objects(items: &[Value], keys: &[&str], limit: usize) -> Vec<String> {
    let mut samples = Vec::new();

    for item in items {
        let Some(value) = keys
            .iter()
            .find_map(|key| item.get(*key).and_then(Value::as_str))
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            continue;
        };

        if samples.iter().any(|sample| sample == value) {
            continue;
        }

        samples.push(value.to_string());
        if samples.len() >= limit {
            break;
        }
    }

    samples
}

fn match_path_text(item: &Value) -> Option<&str> {
    item.get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .or_else(|| {
            item.get("file")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
        .or_else(|| {
            item.get("data")
                .and_then(Value::as_object)
                .and_then(|data| data.get("path"))
                .and_then(Value::as_object)
                .and_then(|path| path.get("text"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
        })
}

fn sample_match_paths(matches: &[Value], limit: usize) -> Vec<String> {
    let mut samples = Vec::new();

    for item in matches {
        let Some(path) = match_path_text(item) else {
            continue;
        };

        if samples.iter().any(|sample| sample == path) {
            continue;
        }

        samples.push(path.to_string());
        if samples.len() >= limit {
            break;
        }
    }

    samples
}

fn summarize_list_items(output: &Value, items: &[Value]) -> String {
    let total = output
        .get("total")
        .or_else(|| output.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(items.len() as u64);

    let (files, directories) =
        items
            .iter()
            .fold((0u64, 0u64), |(files, directories), item| match item.get("type").and_then(Value::as_str) {
                Some("file") => (files + 1, directories),
                Some("directory") => (files, directories + 1),
                _ => (files, directories),
            });

    let mut summary = format!("Listed {total} {}", pluralize(total, "item", "items"));
    if files > 0 || directories > 0 {
        let _ = write!(
            summary,
            " ({} {}, {} {})",
            files,
            pluralize(files, "file", "files"),
            directories,
            pluralize(directories, "directory", "directories"),
        );
    }

    let samples = sample_strings_from_objects(items, &["path", "name"], 3);
    if !samples.is_empty() {
        let _ = write!(summary, ": {}", samples.join(", "));
    }

    summary
}

fn summarize_file_list(output: &Value, files: &[Value]) -> String {
    let total = output.get("total").and_then(Value::as_u64).unwrap_or(files.len() as u64);
    let mut summary = format!("Listed {total} {}", pluralize(total, "file", "files"));

    let samples = files
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .take(3)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !samples.is_empty() {
        let _ = write!(summary, ": {}", samples.join(", "));
    }

    summary
}

fn summarize_search_results(output: &Value, results: &[Value]) -> String {
    let returned = output
        .get("returned")
        .or_else(|| output.get("total"))
        .or_else(|| output.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(results.len() as u64);

    let mut summary = if returned == 0 {
        "No results found".to_string()
    } else {
        format!("Returned {returned} {}", pluralize(returned, "result", "results"))
    };
    if let Some(query) = trimmed_string_field(output, "query") {
        let _ = write!(summary, " for `{query}`");
    }

    // Sample a few hits as `path:line` so archives stay legible without
    // embedding full snippets.
    let mut samples = Vec::new();
    for item in results {
        let Some(path) = match_path_text(item) else { continue };
        let sample = match item.get("line").and_then(Value::as_u64) {
            Some(line) => format!("{path}:{line}"),
            None => path.to_string(),
        };
        if samples.iter().any(|existing: &String| existing == &sample) {
            continue;
        }
        samples.push(sample);
        if samples.len() >= 3 {
            break;
        }
    }
    if !samples.is_empty() {
        let _ = write!(summary, ": {}", samples.join(", "));
    }

    summary
}

fn summarize_matches(output: &Value, matches: &[Value]) -> String {
    let total = output
        .get("total_match_count")
        .or_else(|| output.get("matched_count"))
        .or_else(|| output.get("count"))
        .and_then(Value::as_u64)
        .unwrap_or(matches.len() as u64);

    if total == 0 {
        return "No matches found".to_string();
    }

    let mut summary = format!("Found {total} {}", pluralize(total, "match", "matches"));

    let samples = sample_match_paths(matches, 3);
    if !samples.is_empty() {
        let _ = write!(summary, " in {}", samples.join(", "));
    } else if let Some(path) = trimmed_string_field(output, "path") {
        let _ = write!(summary, " in {path}");
    }

    summary
}

fn append_unique_line(lines: &mut Vec<String>, line: &str) {
    if !lines.iter().any(|existing| existing == line) {
        lines.push(line.to_string());
    }
}

fn contains_line_block(container: &str, candidate: &str) -> bool {
    !line_block_ranges(container, candidate).is_empty()
}

fn line_block_ranges(container: &str, candidate: &str) -> Vec<(usize, usize)> {
    let container_lines = container.lines().collect::<Vec<_>>();
    let candidate_lines = candidate.lines().collect::<Vec<_>>();
    if candidate_lines.is_empty() || candidate_lines.len() > container_lines.len() {
        return Vec::new();
    }

    container_lines
        .windows(candidate_lines.len())
        .enumerate()
        .filter_map(|(start, window)| {
            (window == candidate_lines.as_slice()).then_some((start, start + candidate_lines.len()))
        })
        .collect()
}

fn contains_distinct_line_blocks(container: &str, first: &str, second: &str) -> bool {
    let first_ranges = line_block_ranges(container, first);
    let second_ranges = line_block_ranges(container, second);
    first_ranges.iter().any(|&(first_start, first_end)| {
        second_ranges
            .iter()
            .any(|&(second_start, second_end)| first_end <= second_start || second_end <= first_start)
    })
}

fn append_labeled_stream_text(lines: &mut Vec<String>, label: &str, text: &str) {
    lines.push(format!("[{label}]\n{text}"));
}

fn append_unrepresented_stream_text(lines: &mut Vec<String>, text: &str) {
    if !lines.iter().any(|existing| contains_line_block(existing, text)) {
        lines.push(text.to_string());
    }
}

fn aggregate_primary_streams(output: &Value) -> Vec<String> {
    let merged = trimmed_string_field(output, "output");
    let stdout = trimmed_string_field(output, "stdout");
    let stderr = trimmed_string_field(output, "stderr");
    let mut parts = Vec::new();

    match (merged, stdout, stderr) {
        (None, Some(stdout), Some(stderr)) => {
            // Without an authoritative merged field, stdout and stderr are
            // separate pipes. Keep both labels even when their bytes happen
            // to be identical or one is contained by the other.
            append_labeled_stream_text(&mut parts, "stdout", stdout);
            append_labeled_stream_text(&mut parts, "stderr", stderr);
        }
        (None, Some(stdout), None) | (None, None, Some(stdout)) => parts.push(stdout.to_string()),
        (None, None, None) => {}
        (Some(merged), Some(stdout), Some(stderr)) if contains_distinct_line_blocks(merged, stdout, stderr) => {
            // The merged field proves that it contains both named streams as
            // distinct blocks, so it is safe to emit once without duplicates.
            parts.push(merged.to_string());
        }
        (Some(merged), Some(stdout), Some(stderr)) => {
            let stdout_is_in_merged = contains_line_block(merged, stdout);
            let stderr_is_in_merged = contains_line_block(merged, stderr);
            let stdout_contains_merged = contains_line_block(stdout, merged);
            let stderr_contains_merged = contains_line_block(stderr, merged);

            if stdout_is_in_merged && !stderr_is_in_merged {
                parts.push(merged.to_string());
                append_labeled_stream_text(&mut parts, "stderr", stderr);
            } else if stderr_is_in_merged && !stdout_is_in_merged {
                parts.push(merged.to_string());
                append_labeled_stream_text(&mut parts, "stdout", stdout);
            } else {
                // Equal or overlapping named streams cannot be deduplicated
                // from one occurrence in `output`; retain both labels. Keep
                // the merged value too when neither named stream covers it
                // completely.
                append_labeled_stream_text(&mut parts, "stdout", stdout);
                append_labeled_stream_text(&mut parts, "stderr", stderr);
                if !stdout_contains_merged && !stderr_contains_merged {
                    append_unrepresented_stream_text(&mut parts, merged);
                }
            }
        }
        (Some(merged), Some(named), None) | (Some(merged), None, Some(named)) => {
            if contains_line_block(merged, named) {
                // The merged value contains the complete named stream and is
                // the least lossy representation of the result.
                parts.push(merged.to_string());
            } else if contains_line_block(named, merged) {
                // The named stream is the complete value and `output` is a
                // bounded preview.
                parts.push(named.to_string());
            } else {
                parts.push(merged.to_string());
                append_unrepresented_stream_text(&mut parts, named);
            }
        }
        (Some(merged), None, None) => parts.push(merged.to_string()),
    }

    if let Some(content) = trimmed_string_field(output, "content") {
        // `content` is an independent payload field. Never replace a labeled
        // pipe stream with it merely because one contains the other.
        append_unrepresented_stream_text(&mut parts, content);
    }

    parts
}

const STREAM_OUTPUT_METADATA_FIELDS: &[&str] = &[
    "output",
    "stdout",
    "stderr",
    "content",
    "success",
    "status",
    "exit_code",
    "command",
    "working_directory",
    "session_id",
    "process_id",
    "id",
    "is_exited",
    "rows",
    "cols",
    "wall_time",
    "duration_ms",
    "spool_path",
    "critical_note",
    "hint",
    "message",
    "next_action",
    "error_message",
];

fn structured_stream_metadata(output: &Value, additional_excluded_fields: &[&str]) -> Option<String> {
    let object = output.as_object()?;
    let metadata = object
        .iter()
        .filter(|(key, value)| {
            // A string error is rendered below as its user-facing text;
            // retain structured errors so codes, retry state, and other
            // diagnostic fields remain available to event consumers.
            !(STREAM_OUTPUT_METADATA_FIELDS.contains(&key.as_str())
                || additional_excluded_fields.contains(&key.as_str())
                || matches!(value, Value::Null)
                || (key.as_str() == "error" && value.is_string()))
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<serde_json::Map<_, _>>();

    if metadata.is_empty() {
        return None;
    }

    serde_json::to_string_pretty(&Value::Object(metadata))
        .ok()
        .map(|metadata| format!("Structured output:\n{metadata}"))
}

fn append_primary_output_context_with_exclusions(
    parts: &mut Vec<String>,
    output: &Value,
    additional_excluded_fields: &[&str],
) {
    if let Some(metadata) = structured_stream_metadata(output, additional_excluded_fields) {
        append_unique_line(parts, &metadata);
    }
    if let Some(text) = trimmed_error_message(output) {
        append_unique_line(parts, text);
    }
    for key in ["message", "critical_note", "hint", "next_action", "error_message"] {
        if let Some(text) = trimmed_string_field(output, key) {
            append_unique_line(parts, text);
        }
    }
}

fn append_primary_output_context(parts: &mut Vec<String>, output: &Value) {
    append_primary_output_context_with_exclusions(parts, output, &[]);
}

const LIST_RESULT_SUMMARY_FIELDS: &[&str] = &["items", "count", "total"];
const FILE_RESULT_SUMMARY_FIELDS: &[&str] = &["files", "total"];
const MATCH_RESULT_SUMMARY_FIELDS: &[&str] = &["matches", "total_match_count", "matched_count", "count", "path"];
const SEARCH_RESULT_SUMMARY_FIELDS: &[&str] = &["results", "returned", "total", "count", "query"];
const EMPTY_RESULT_SUMMARY_FIELDS: &[&str] = &[];

fn append_structured_result_metadata(parts: &mut Vec<String>, output: &Value, summary_fields: &[&str]) {
    if let Some(metadata) = structured_stream_metadata(output, summary_fields) {
        append_unique_line(parts, &metadata);
    }
}

fn append_diff_stream_context(parts: &mut Vec<String>, output: &Value) {
    for stream in aggregate_primary_streams(output) {
        let stream_text = stream
            .strip_prefix("[stdout]\n")
            .or_else(|| stream.strip_prefix("[stderr]\n"))
            .unwrap_or(&stream);
        let already_in_diff = canonical_diff_previews(output).iter().any(|diff| {
            diff.get("content")
                .and_then(Value::as_str)
                .is_some_and(|content| content.trim() == stream_text.trim())
        });
        if !already_in_diff {
            append_unique_line(parts, &stream);
        }
    }
}

/// Extract a [`ToolOutputPayload`] from a tool result JSON value, preserving
/// bounded metadata alongside a spool reference and falling back to inline
/// text aggregation otherwise.
pub fn tool_output_payload_from_value(output: &Value) -> ToolOutputPayload {
    if !output.is_object() {
        let aggregated_output = match output {
            Value::Null => String::new(),
            Value::String(text) => text.to_string(),
            _ => serde_json::to_string(output).unwrap_or_default(),
        };
        return ToolOutputPayload { aggregated_output, spool_path: None };
    }

    if let Some(spool_path) = output.get("spool_path").and_then(Value::as_str) {
        let mut parts = Vec::new();
        append_primary_output_context(&mut parts, output);
        return ToolOutputPayload {
            aggregated_output: parts.join("\n"),
            spool_path: Some(spool_path.to_string()),
        };
    }

    if let Some(diff) = diff_preview_output(output) {
        let mut parts = vec![diff];
        append_diff_stream_context(&mut parts, output);
        append_primary_output_context_with_exclusions(&mut parts, output, &["diff", "diff_preview"]);
        return ToolOutputPayload {
            aggregated_output: parts.join("\n"),
            spool_path: None,
        };
    }

    let mut primary_text = aggregate_primary_streams(output);

    if !primary_text.is_empty() {
        append_primary_output_context(&mut primary_text, output);
        return ToolOutputPayload {
            aggregated_output: primary_text.join("\n"),
            spool_path: None,
        };
    }

    let (structured_summary, summary_fields) = if let Some(items) = output.get("items").and_then(Value::as_array) {
        (Some(summarize_list_items(output, items)), LIST_RESULT_SUMMARY_FIELDS)
    } else if let Some(files) = output.get("files").and_then(Value::as_array) {
        (Some(summarize_file_list(output, files)), FILE_RESULT_SUMMARY_FIELDS)
    } else if let Some(matches) = output.get("matches").and_then(Value::as_array) {
        (Some(summarize_matches(output, matches)), MATCH_RESULT_SUMMARY_FIELDS)
    } else if let Some(results) = output.get("results").and_then(Value::as_array) {
        // `code_search`-style outputs: without this branch the archive only
        // records "Structured result with fields: query, filters, results,
        // returned", which made the turn_912/913 planning trajectories (54
        // searches) illegible in session archives and ATIF exports.
        (Some(summarize_search_results(output, results)), SEARCH_RESULT_SUMMARY_FIELDS)
    } else {
        (
            output
                .as_object()
                .map(|obj| {
                    obj.keys()
                        .filter(|key| key.as_str() != "success")
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .filter(|keys| !keys.is_empty())
                .map(|keys| format!("Structured result with fields: {}", keys.join(", "))),
            EMPTY_RESULT_SUMMARY_FIELDS,
        )
    };

    let mut parts = Vec::new();
    if let Some(summary) = structured_summary.as_deref() {
        append_unique_line(&mut parts, summary);
    }
    if let Some(text) = trimmed_error_message(output) {
        append_unique_line(&mut parts, text);
    }
    for key in ["message", "critical_note", "hint", "next_action", "error_message"] {
        if let Some(text) = trimmed_string_field(output, key) {
            append_unique_line(&mut parts, text);
        }
    }
    append_structured_result_metadata(&mut parts, output, summary_fields);

    ToolOutputPayload {
        aggregated_output: parts.join("\n"),
        spool_path: None,
    }
}

/// Shared lifecycle state for assistant text, reasoning, and model-emitted tool calls.
#[derive(Debug, Default)]
pub struct SharedLifecycleEmitter {
    next_item_index: u64,
    assistant: StreamingTextState,
    reasoning: StreamingTextState,
    reasoning_stage: Option<String>,
    tool_calls: HashMap<String, ToolCallStreamState>,
    pending_events: Vec<ThreadEvent>,
}

impl SharedLifecycleEmitter {
    /// Generate the next unique item ID for lifecycle events.
    #[must_use]
    pub fn next_item_id(&mut self) -> String {
        let id = self.next_item_index;
        self.next_item_index += 1;
        format!("item_{id}")
    }

    /// Emit a completed agent message event with the full text.
    pub fn emit_completed_agent_message(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::AgentMessage(AgentMessageItem { text: text.to_string() }),
            },
        }));
    }

    /// Replace the current assistant streaming text. Returns `true` if the text changed.
    pub fn replace_assistant_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.assistant, text)
    }

    /// Whether the assistant text stream has been started.
    #[must_use]
    pub fn assistant_started(&self) -> bool {
        self.assistant.started
    }

    /// Length of the accumulated assistant text in bytes.
    #[must_use]
    pub fn assistant_len(&self) -> usize {
        self.assistant.text.len()
    }

    /// Append a delta to the assistant text stream. Returns `true` if content was added.
    pub fn append_assistant_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.assistant, delta)
    }

    /// Emit a snapshot of the current assistant text as an item event.
    pub fn emit_assistant_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        emit_text_snapshot(&mut self.pending_events, &mut self.assistant, item_id, |text| {
            ThreadItemDetails::AgentMessage(AgentMessageItem { text })
        })
    }

    /// Complete the assistant text stream, emitting a final completed event.
    pub fn complete_assistant_stream(&mut self) -> bool {
        complete_text_stream(&mut self.pending_events, &mut self.assistant, |text| {
            ThreadItemDetails::AgentMessage(AgentMessageItem { text })
        })
    }

    /// Emit a completed reasoning event with the full text.
    pub fn emit_completed_reasoning(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let item_id = self.next_item_id();
        self.pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    text: text.to_string(),
                    stage: self.reasoning_stage.clone(),
                })),
            },
        }));
    }

    /// Replace the current reasoning streaming text. Returns `true` if the text changed.
    pub fn replace_reasoning_text(&mut self, text: &str) -> bool {
        replace_stream_text(&mut self.reasoning, text)
    }

    /// Append a delta to the reasoning text stream. Returns `true` if content was added.
    pub fn append_reasoning_delta(&mut self, delta: &str) -> bool {
        append_stream_delta(&mut self.reasoning, delta)
    }

    /// Update the reasoning stage label. Returns `true` if the stage changed.
    pub fn set_reasoning_stage(&mut self, stage: Option<String>) -> bool {
        if self.reasoning_stage == stage {
            return false;
        }
        self.reasoning_stage = stage;
        true
    }

    /// Length of the accumulated reasoning text in bytes.
    #[must_use]
    pub fn reasoning_len(&self) -> usize {
        self.reasoning.text.len()
    }

    /// Whether the reasoning text stream has been started.
    #[must_use]
    pub fn reasoning_started(&self) -> bool {
        self.reasoning.started
    }

    /// Emit a snapshot of the current reasoning text as an item event.
    pub fn emit_reasoning_snapshot(&mut self, item_id: Option<String>) -> bool {
        let item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let stage = self.reasoning_stage.clone();
        emit_text_snapshot(&mut self.pending_events, &mut self.reasoning, item_id, move |text| {
            ThreadItemDetails::Reasoning(Box::new(ReasoningItem { text, stage: stage.clone() }))
        })
    }

    /// Emit an update event reflecting the current reasoning stage.
    pub fn emit_reasoning_stage_update(&mut self) -> bool {
        if !self.reasoning.started {
            return false;
        }
        let Some(item_id) = self.reasoning.item_id.clone() else {
            return false;
        };
        self.pending_events.push(ThreadEvent::ItemUpdated(ItemUpdatedEvent {
            item: ThreadItem {
                context: None,
                id: item_id,
                details: ThreadItemDetails::Reasoning(Box::new(ReasoningItem {
                    text: self.reasoning.text.clone(),
                    stage: self.reasoning_stage.clone(),
                })),
            },
        }));
        true
    }

    /// Complete the reasoning text stream, emitting a final completed event.
    pub fn complete_reasoning_stream(&mut self) -> bool {
        let stage = self.reasoning_stage.clone();
        complete_text_stream(&mut self.pending_events, &mut self.reasoning, move |text| {
            ThreadItemDetails::Reasoning(Box::new(ReasoningItem { text, stage: stage.clone() }))
        })
    }

    /// Start tracking a tool call, emitting an item-started event.
    pub fn start_tool_call(&mut self, call_id: &str, tool_name: Option<String>, item_id: Option<String>) -> bool {
        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }
        if buffer.started {
            return false;
        }

        buffer.started = true;
        self.pending_events.push(tool_started_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            None,
            Some(call_id),
        ));
        true
    }

    /// Append an argument delta to an in-progress tool call.
    pub fn append_tool_call_delta(
        &mut self,
        call_id: &str,
        delta: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        if delta.is_empty() {
            return false;
        }

        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if !buffer.started {
            buffer.started = true;
            if buffer.name.is_none() {
                buffer.name = tool_name;
            }
            self.pending_events.push(tool_started_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                None,
                Some(call_id),
            ));
        } else if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        buffer.arguments.push_str(delta);
        // Throttle intermediate `item.updated` events: emit the first delta
        // eagerly (so the UI shows progress), then only when enough new bytes
        // have accumulated and the per-call cap hasn't been reached. The final
        // `complete_tool_call` always emits the full arguments regardless.
        let new_len = buffer.arguments.len();
        let should_emit = buffer.update_events == 0
            || (buffer.update_events < MAX_TOOL_ARG_UPDATE_EVENTS
                && new_len.saturating_sub(buffer.last_emitted_args_len) >= MIN_TOOL_ARG_UPDATE_BYTES);
        if should_emit {
            buffer.last_emitted_args_len = new_len;
            buffer.update_events += 1;
            let arguments = progress_tool_arguments(&buffer.arguments);
            self.pending_events.push(tool_invocation_updated_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                Some(&arguments),
                Some(call_id),
                ToolCallStatus::InProgress,
            ));
        }
        true
    }

    pub fn complete_tool_call(&mut self, call_id: &str, status: ToolCallStatus, outcome: Option<ToolOutcome>) -> bool {
        let Some(buffer) = self.tool_calls.remove(call_id) else {
            return false;
        };
        if !buffer.started {
            return false;
        }

        let arguments = if buffer.arguments.is_empty() {
            None
        } else {
            Some(progress_tool_arguments(&buffer.arguments))
        };
        let resolved_outcome = outcome.unwrap_or_else(|| tool_outcome_from_status(&status));
        self.pending_events.push(tool_invocation_completed_event(
            buffer.item_id,
            buffer.name.as_deref().unwrap_or_default(),
            arguments.as_ref(),
            Some(call_id),
            status,
            resolved_outcome,
        ));
        true
    }

    #[must_use]
    pub fn tool_call_item_id(&self, call_id: &str) -> Option<&str> {
        self.tool_calls.get(call_id).map(|buffer| buffer.item_id.as_str())
    }

    pub fn sync_tool_call_arguments(
        &mut self,
        call_id: &str,
        arguments: &str,
        tool_name: Option<String>,
        item_id: Option<String>,
    ) -> bool {
        let generated_item_id = item_id.unwrap_or_else(|| self.next_item_id());
        let buffer = self
            .tool_calls
            .entry(call_id.to_string())
            .or_insert_with(|| ToolCallStreamState {
                item_id: generated_item_id,
                name: None,
                arguments: String::new(),
                started: false,
                last_emitted_args_len: 0,
                update_events: 0,
            });

        if buffer.name.is_none() {
            buffer.name = tool_name;
        }

        if !buffer.started {
            buffer.started = true;
            self.pending_events.push(tool_started_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                None,
                Some(call_id),
            ));
        }

        if buffer.arguments == arguments {
            return false;
        }

        buffer.arguments.clear();
        buffer.arguments.push_str(arguments);
        let args = progress_tool_arguments(&buffer.arguments);
        self.pending_events.push(tool_invocation_updated_event(
            buffer.item_id.clone(),
            buffer.name.as_deref().unwrap_or_default(),
            Some(&args),
            Some(call_id),
            ToolCallStatus::InProgress,
        ));
        buffer.last_emitted_args_len = buffer.arguments.len();
        buffer.update_events = buffer.update_events.saturating_add(1);
        true
    }

    pub fn complete_open_items(&mut self) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(ToolCallStatus::Completed);
    }

    pub fn complete_open_text_items(&mut self) {
        let _ = self.complete_assistant_stream();
        let _ = self.complete_reasoning_stream();
    }

    pub fn complete_open_items_with_tool_status(&mut self, status: ToolCallStatus) {
        self.complete_open_text_items();
        self.complete_open_tool_calls_with_status(status);
    }

    pub fn complete_open_tool_calls_with_status(&mut self, status: ToolCallStatus) {
        let call_ids = self.tool_calls.keys().cloned().collect::<Vec<_>>();
        for call_id in call_ids {
            let _ = self.complete_tool_call(&call_id, status.clone(), None);
        }
    }

    /// Emit a final `item.updated` carrying the full accumulated arguments for
    /// each open tool call, bypassing the intermediate-update throttle.
    ///
    /// Tool calls remain open (not completed); callers complete them via
    /// [`Self::complete_tool_call`] when execution finishes. This guarantees
    /// the authoritative streamed arguments are visible after streaming ends
    /// even when intermediate deltas were throttled (e.g. small tool calls
    /// whose completing delta fell below the byte threshold). Updates that
    /// would carry the same length as the last emitted snapshot are skipped
    /// to avoid redundant events for large tool calls whose last intermediate
    /// update already captured the full arguments.
    pub fn flush_open_tool_call_arguments(&mut self) {
        let call_ids: Vec<String> = self.tool_calls.keys().cloned().collect();
        for call_id in call_ids {
            let Some(buffer) = self.tool_calls.get_mut(&call_id) else {
                continue;
            };
            if buffer.arguments.is_empty() || buffer.last_emitted_args_len == buffer.arguments.len() {
                continue;
            }
            let arguments = progress_tool_arguments(&buffer.arguments);
            self.pending_events.push(tool_invocation_updated_event(
                buffer.item_id.clone(),
                buffer.name.as_deref().unwrap_or_default(),
                Some(&arguments),
                Some(&call_id),
                ToolCallStatus::InProgress,
            ));
            buffer.last_emitted_args_len = buffer.arguments.len();
        }
    }

    #[must_use]
    pub fn drain_events(&mut self) -> Vec<ThreadEvent> {
        std::mem::take(&mut self.pending_events)
    }
}

fn replace_stream_text(state: &mut StreamingTextState, text: &str) -> bool {
    if state.text == text {
        return false;
    }
    state.text.clear();
    state.text.push_str(text);
    true
}

fn append_stream_delta(state: &mut StreamingTextState, delta: &str) -> bool {
    if delta.is_empty() {
        return false;
    }
    state.text.push_str(delta);
    true
}

fn emit_text_snapshot(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    item_id: String,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if state.text.trim().is_empty() {
        return false;
    }

    let item_id = state.item_id.get_or_insert(item_id).clone();
    let item = ThreadItem {
        context: None,
        id: item_id,
        details: build_details(state.text.clone()),
    };

    if state.started {
        pending_events.push(ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }));
    } else {
        state.started = true;
        pending_events.push(ThreadEvent::ItemStarted(ItemStartedEvent { item }));
    }
    true
}

fn complete_text_stream(
    pending_events: &mut Vec<ThreadEvent>,
    state: &mut StreamingTextState,
    build_details: impl FnOnce(String) -> ThreadItemDetails,
) -> bool {
    if !state.started {
        return false;
    }

    let Some(item_id) = state.item_id.take() else {
        state.started = false;
        state.text.clear();
        return false;
    };

    state.started = false;
    let text = std::mem::take(&mut state.text);
    pending_events.push(ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: item_id,
            details: build_details(text),
        },
    }));
    true
}

#[must_use]
pub fn tool_output_item_id(call_item_id: &str) -> String {
    format!("{call_item_id}:output")
}

fn tool_invocation_item(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    outcome: Option<ToolOutcome>,
) -> ThreadItem {
    ThreadItem {
        context: None,
        id: item_id,
        details: ThreadItemDetails::ToolInvocation(Box::new(ToolInvocationItem {
            tool_name: tool_name.to_string(),
            arguments: arguments.cloned(),
            tool_call_id: tool_call_id.map(str::to_string),
            status,
            outcome,
        })),
    }
}

fn tool_output_item(
    call_item_id: &str,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
) -> ThreadItem {
    ThreadItem {
        context: None,
        id: tool_output_item_id(call_item_id),
        details: ThreadItemDetails::ToolOutput(Box::new(ToolOutputItem {
            call_id: call_item_id.to_string(),
            tool_call_id: tool_call_id.map(str::to_string),
            spool_path: spool_path.map(str::to_string),
            output: output.into(),
            exit_code,
            status,
        })),
    }
}

#[must_use]
pub fn tool_started_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, ToolCallStatus::InProgress, None),
    })
}

#[must_use]
pub fn tool_invocation_updated_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status, None),
    })
}

#[must_use]
pub fn tool_invocation_completed_event(
    item_id: String,
    tool_name: &str,
    arguments: Option<&Value>,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    outcome: ToolOutcome,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_invocation_item(item_id, tool_name, arguments, tool_call_id, status, Some(outcome)),
    })
}

#[must_use]
pub fn tool_output_started_event(call_item_id: String, tool_call_id: Option<&str>) -> ThreadEvent {
    ThreadEvent::ItemStarted(ItemStartedEvent {
        item: tool_output_item(&call_item_id, tool_call_id, ToolCallStatus::InProgress, None, None, String::new()),
    })
}

#[must_use]
pub fn tool_output_updated_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    ThreadEvent::ItemUpdated(ItemUpdatedEvent {
        item: tool_output_item(&call_item_id, tool_call_id, ToolCallStatus::InProgress, None, None, output),
    })
}

#[must_use]
pub fn tool_output_completed_event(
    call_item_id: String,
    tool_call_id: Option<&str>,
    status: ToolCallStatus,
    exit_code: Option<i32>,
    spool_path: Option<&str>,
    output: impl Into<String>,
) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: tool_output_item(&call_item_id, tool_call_id, status, exit_code, spool_path, output),
    })
}

#[must_use]
#[cold]
pub fn error_item_completed_event(item_id: String, message: impl Into<String>) -> ThreadEvent {
    ThreadEvent::ItemCompleted(ItemCompletedEvent {
        item: ThreadItem {
            context: None,
            id: item_id,
            details: ThreadItemDetails::Error(ErrorItem { message: message.into() }),
        },
    })
}

fn progress_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn explanation_file_attribution_ignores_noops_failures_and_unrelated_reads() {
        let args = serde_json::json!({"path":"a.rs", "content":"same"});
        let noop = serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","is_empty":true,"content":""}]});
        assert!(file_change_completed_event("call", "write_file", &args, &noop).is_none());
        let changed = serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","content":"--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new","additions":1,"deletions":1},{"path":"b.rs","operation":"updated","is_empty":true}]});
        let event = file_change_completed_event("call", "write_file", &args, &changed).unwrap();
        let ThreadEvent::ItemCompleted(e) = event else {
            panic!("expected completed file change")
        };
        let ThreadItemDetails::FileChange(change) = e.item.details else {
            panic!("expected file change")
        };
        assert_eq!(change.changes.len(), 1);
        assert_eq!(change.changes[0].path, "a.rs");
        assert_eq!(change.additions, Some(1));
        assert_eq!(change.diff_incomplete, None);
        let incomplete = serde_json::json!({"success":true,"diff":[{"path":"a.rs","operation":"updated","content":"+new","additions":1,"deletions":0,"truncated":true},{"path":"b.rs","operation":"updated","skipped":true}]});
        let ThreadEvent::ItemCompleted(event) =
            file_change_completed_event("second", "write_file", &args, &incomplete).unwrap()
        else {
            panic!("file event")
        };
        let ThreadItemDetails::FileChange(change) = event.item.details else {
            panic!("file details")
        };
        assert_eq!(change.diff_incomplete, Some(true));
        assert_eq!(change.additions, None, "partial totals must remain unavailable");
        assert!(file_change_completed_event("call", "read_file", &args, &changed).is_none());
        assert!(
            file_change_completed_event(
                "call",
                "write_file",
                &args,
                &serde_json::json!({"success":false,"modified_files":["a.rs"]})
            )
            .is_none()
        );
    }
    use serde_json::json;

    use super::*;

    #[test]
    fn tool_started_event_omits_arguments_when_absent() {
        let event = tool_started_event("item".to_string(), "shell", None, Some("call_1"));
        let ThreadEvent::ItemStarted(ItemStartedEvent { item }) = event else {
            panic!("expected started item");
        };
        let ThreadItemDetails::ToolInvocation(details) = item.details else {
            panic!("expected tool invocation");
        };
        assert!(details.arguments.is_none());
        assert_eq!(details.tool_name, "shell");
    }

    #[test]
    fn tool_output_updated_event_streams_in_progress_output() {
        let event = tool_output_updated_event("item".to_string(), Some("call_1"), "abc");
        let ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }) = event else {
            panic!("expected updated item");
        };
        let ThreadItemDetails::ToolOutput(details) = item.details else {
            panic!("expected tool output");
        };
        assert_eq!(details.call_id, "item");
        assert_eq!(details.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(details.output, "abc");
        assert_eq!(details.status, ToolCallStatus::InProgress);
    }

    #[test]
    fn tool_output_payload_preserves_spool_reference() {
        let payload = tool_output_payload_from_value(&json!({
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "output": "ignored"
        }));

        assert_eq!(payload.aggregated_output, "");
        assert_eq!(payload.spool_path.as_deref(), Some(".vtcode/context/tool_outputs/run-1.txt"));
    }

    #[test]
    fn tool_output_payload_preserves_spooled_metadata_with_reference() {
        let payload = tool_output_payload_from_value(&json!({
            "spool_path": ".vtcode/context/tool_outputs/run-1.txt",
            "preview": "bounded preview",
            "spooled_bytes": 42,
            "stderr_preview": "diagnostic preview",
            "generated_files": ["src/generated.rs"],
            "next_action": "Inspect the generated file."
        }));

        assert_eq!(payload.spool_path.as_deref(), Some(".vtcode/context/tool_outputs/run-1.txt"));
        assert!(payload.aggregated_output.contains("bounded preview"));
        assert!(payload.aggregated_output.contains("\"spooled_bytes\": 42"));
        assert!(payload.aggregated_output.contains("diagnostic preview"));
        assert!(payload.aggregated_output.contains("src/generated.rs"));
        assert!(payload.aggregated_output.contains("Inspect the generated file."));
    }

    #[test]
    fn tool_output_payload_includes_apply_patch_diff_content() {
        let payload = tool_output_payload_from_value(&json!({
            "success": true,
            "applied": ["[1/1] Updated file: README.md (1 chunk)"],
            "modified_files": ["README.md"],
            "diff": [{
                "path": "README.md",
                "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
                "skipped": false
            }]
        }));

        assert!(payload.aggregated_output.contains("diff --git a/README.md b/README.md"));
        assert!(payload.aggregated_output.contains("-before"));
        assert!(payload.aggregated_output.contains("+after"));
    }

    #[test]
    fn tool_output_payload_normalizes_legacy_write_diff_preview() {
        let payload = tool_output_payload_from_value(&json!({
            "success": true,
            "path": "README.md",
            "diff_preview": {
                "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
                "skipped": false
            }
        }));

        assert!(payload.aggregated_output.contains("diff preview for README.md"));
        assert!(payload.aggregated_output.contains("-before"));
        assert!(payload.aggregated_output.contains("+after"));
    }

    #[test]
    fn tool_output_payload_preserves_diff_metadata_without_repeating_preview() {
        let payload = tool_output_payload_from_value(&json!({
            "success": true,
            "applied": ["Updated README.md"],
            "modified_files": ["README.md"],
            "diff": [{
                "path": "README.md",
                "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
                "skipped": false
            }],
            "next_action": "Run the documentation checks."
        }));

        assert!(payload.aggregated_output.contains("Updated README.md"));
        assert!(payload.aggregated_output.contains("README.md"));
        assert!(payload.aggregated_output.contains("Run the documentation checks."));
        assert_eq!(payload.aggregated_output.matches("diff --git a/README.md b/README.md").count(), 1);
    }

    #[test]
    fn tool_output_payload_summarizes_list_results() {
        let payload = tool_output_payload_from_value(&json!({
            "items": [
                {"name": "app.rs", "path": "vtcode-tui/src/app.rs", "type": "file"},
                {"name": "core_tui", "path": "vtcode-tui/src/core_tui", "type": "directory"},
                {"name": "lib.rs", "path": "vtcode-tui/src/lib.rs", "type": "file"}
            ],
            "count": 3,
            "total": 11
        }));

        assert_eq!(payload.spool_path, None);
        assert!(payload.aggregated_output.contains("Listed 11 items"));
        assert!(payload.aggregated_output.contains("2 files, 1 directory"));
        assert!(payload.aggregated_output.contains("vtcode-tui/src/app.rs"));
    }

    #[test]
    fn tool_output_payload_combines_list_summary_with_message() {
        let payload = tool_output_payload_from_value(&json!({
            "items": [
                {"name": "app.rs", "path": "vtcode-tui/src/app.rs", "type": "file"},
                {"name": "core_tui", "path": "vtcode-tui/src/core_tui", "type": "directory"}
            ],
            "count": 2,
            "total": 2,
            "message": "[+3 more items]"
        }));

        assert!(payload.aggregated_output.contains("Listed 2 items"));
        assert!(payload.aggregated_output.contains("[+3 more items]"));
    }

    #[test]
    fn tool_output_payload_summarizes_match_results() {
        let payload = tool_output_payload_from_value(&json!({
            "matches": [
                {"path": "src/main.rs", "line_number": 12},
                {"file": "src/lib.rs", "line_number": 9}
            ],
            "total_match_count": 7
        }));

        assert_eq!(payload.spool_path, None);
        assert!(payload.aggregated_output.contains("Found 7 matches"));
        assert!(payload.aggregated_output.contains("src/main.rs"));
        assert!(payload.aggregated_output.contains("src/lib.rs"));
    }

    #[test]
    fn tool_output_payload_summarizes_nested_match_paths() {
        let payload = tool_output_payload_from_value(&json!({
            "matches": [
                {
                    "type": "match",
                    "data": {
                        "path": {"text": "vtcode-tui/src/core_tui/runner/mod.rs"},
                        "line_number": 27,
                        "lines": {"text": "runloop\n"}
                    }
                }
            ],
            "total_match_count": 1
        }));

        assert!(payload.aggregated_output.contains("Found 1 match"));
        assert!(payload.aggregated_output.contains("vtcode-tui/src/core_tui/runner/mod.rs"));
    }

    #[test]
    fn tool_output_payload_reports_empty_match_set() {
        let payload = tool_output_payload_from_value(&json!({
            "matches": [],
            "path": "crates/codegen/vtcode-core/src"
        }));

        assert_eq!(payload.aggregated_output, "No matches found");
        assert_eq!(payload.spool_path, None);
    }

    #[test]
    fn tool_output_payload_summarizes_code_search_results() {
        // Shape produced by the `code_search` tool (`CodeSearchResult`).
        let payload = tool_output_payload_from_value(&json!({
            "query": "startup timing",
            "filters": {"path": "src", "file_types": null, "result_types": null, "max_results": 10},
            "results": [
                {"result_type": "definition", "path": "src/main.rs", "line": 42},
                {"result_type": "usage", "path": "src/startup/mod.rs", "line": 7},
                {"result_type": "usage", "path": "src/main.rs", "line": 42}
            ],
            "returned": 3
        }));

        assert_eq!(payload.spool_path, None);
        assert!(payload.aggregated_output.contains("Returned 3 results"));
        assert!(payload.aggregated_output.contains("for `startup timing`"));
        assert!(payload.aggregated_output.contains("src/main.rs:42"));
        assert!(payload.aggregated_output.contains("src/startup/mod.rs:7"));
        // Duplicated path:line sampled once only.
        assert_eq!(payload.aggregated_output.matches("src/main.rs:42").count(), 1);
    }

    #[test]
    fn tool_output_payload_reports_empty_code_search_results() {
        let payload = tool_output_payload_from_value(&json!({
            "query": "nonexistent_symbol",
            "results": [],
            "returned": 0
        }));

        assert_eq!(payload.aggregated_output, "No results found for `nonexistent_symbol`");
    }

    #[test]
    fn tool_output_payload_includes_structured_recovery_guidance() {
        let payload = tool_output_payload_from_value(&json!({
            "matches": [],
            "path": "src/agent",
            "hint": "Pattern looks like a code fragment.",
            "next_action": "Retry with a larger parseable pattern."
        }));

        assert!(payload.aggregated_output.contains("No matches found"));
        assert!(payload.aggregated_output.contains("Pattern looks like a code fragment."));
        assert!(payload.aggregated_output.contains("Retry with a larger parseable pattern."));
    }

    #[test]
    fn tool_output_payload_preserves_distinct_stream_aliases_once() {
        let payload = tool_output_payload_from_value(&json!({
            "output": "merged stdout\nmerged stderr",
            "stdout": "merged stdout",
            "stderr": "merged stderr"
        }));

        assert_eq!(payload.aggregated_output, "merged stdout\nmerged stderr");
    }

    #[test]
    fn tool_output_payload_preserves_equal_named_streams_without_merged_output() {
        let payload = tool_output_payload_from_value(&json!({
            "stdout": "same output",
            "stderr": "same output"
        }));

        assert!(payload.aggregated_output.contains("[stdout]\nsame output"));
        assert!(payload.aggregated_output.contains("[stderr]\nsame output"));
        assert_eq!(payload.aggregated_output.matches("same output").count(), 2);
    }

    #[test]
    fn tool_output_payload_preserves_subset_named_streams_without_merged_output() {
        let payload = tool_output_payload_from_value(&json!({
            "stdout": "shared line\nstdout only",
            "stderr": "shared line"
        }));

        assert!(payload.aggregated_output.contains("[stdout]\nshared line\nstdout only"));
        assert!(payload.aggregated_output.contains("[stderr]\nshared line"));
        assert_eq!(payload.aggregated_output.matches("shared line").count(), 2);
    }

    #[test]
    fn tool_output_payload_preserves_overlapping_named_streams_with_merged_output() {
        let payload = tool_output_payload_from_value(&json!({
            "output": "shared line\nmerged only",
            "stdout": "shared line",
            "stderr": "shared line"
        }));

        assert!(payload.aggregated_output.contains("[stdout]\nshared line"));
        assert!(payload.aggregated_output.contains("[stderr]\nshared line"));
        assert!(payload.aggregated_output.contains("shared line\nmerged only"));
    }

    #[test]
    fn tool_output_payload_preserves_structured_metadata_with_streams() {
        let payload = tool_output_payload_from_value(&json!({
            "stdout": "command output",
            "json_result": {"answer": 42},
            "generated_files": {"files": ["src/generated.rs"]},
            "next_action": "Review the generated file."
        }));

        assert!(payload.aggregated_output.contains("command output"));
        assert!(payload.aggregated_output.contains("Structured output:"));
        assert!(payload.aggregated_output.contains("src/generated.rs"));
        assert!(payload.aggregated_output.contains("\"answer\": 42"));
        assert!(payload.aggregated_output.contains("Review the generated file."));
    }

    #[test]
    fn tool_output_payload_preserves_structured_only_result_metadata() {
        let payload = tool_output_payload_from_value(&json!({
            "json_result": {"answer": 42},
            "metadata_flag": false,
            "metadata_count": 0,
        }));

        assert!(payload.aggregated_output.contains("Structured output:"));
        assert!(payload.aggregated_output.contains("\"answer\": 42"));
        assert!(payload.aggregated_output.contains("\"metadata_flag\": false"));
        assert!(payload.aggregated_output.contains("\"metadata_count\": 0"));
    }

    #[test]
    fn tool_output_payload_preserves_structured_error_details() {
        let payload = tool_output_payload_from_value(&json!({
            "error": {
                "message": "command failed",
                "code": "E_COMMAND",
                "retryable": true,
            }
        }));

        assert!(payload.aggregated_output.contains("command failed"));
        assert!(payload.aggregated_output.contains("\"code\": \"E_COMMAND\""));
        assert!(payload.aggregated_output.contains("\"retryable\": true"));
    }

    #[test]
    fn tool_output_payload_preserves_streams_alongside_diff_preview() {
        let payload = tool_output_payload_from_value(&json!({
            "stdout": "apply completed",
            "stderr": "warning: generated file was already present",
            "diff": [{
                "path": "README.md",
                "content": "diff --git a/README.md b/README.md\n-before\n+after\n",
                "skipped": false,
            }]
        }));

        assert!(payload.aggregated_output.contains("diff --git a/README.md b/README.md"));
        assert!(payload.aggregated_output.contains("[stdout]\napply completed"));
        assert!(
            payload
                .aggregated_output
                .contains("[stderr]\nwarning: generated file was already present")
        );
    }

    #[test]
    fn tool_output_payload_preserves_non_object_results() {
        assert_eq!(tool_output_payload_from_value(&json!("  plain result\n")).aggregated_output, "  plain result\n");
        assert_eq!(tool_output_payload_from_value(&json!(42)).aggregated_output, "42");
        assert_eq!(tool_output_payload_from_value(&json!(null)).aggregated_output, "");
    }

    #[test]
    fn tool_invocation_completed_event_embeds_outcome() {
        let event = tool_invocation_completed_event(
            "tool_1".to_string(),
            "exec_command",
            Some(&json!({"cmd": "pwd"})),
            Some("call_1"),
            ToolCallStatus::Failed,
            ToolOutcome::HookDenied,
        );
        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = event else {
            panic!("expected completed item");
        };
        let ThreadItemDetails::ToolInvocation(details) = item.details else {
            panic!("expected tool invocation");
        };
        assert_eq!(details.status, ToolCallStatus::Failed);
        assert_eq!(details.outcome, Some(ToolOutcome::HookDenied));
    }

    #[test]
    fn complete_tool_call_infers_outcome_from_status() {
        let mut emitter = SharedLifecycleEmitter::default();
        let call_id = "call_1".to_string();
        emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
        emitter.sync_tool_call_arguments(&call_id, "{\"cmd\":\"pwd\"}", Some("exec_command".to_string()), None);
        emitter.complete_tool_call(&call_id, ToolCallStatus::Completed, None);
        let events = emitter.drain_events();
        // events[0] = tool_started, events[1] = tool_invocation_updated, events[2] = tool_invocation_completed
        let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = &events[2] else {
            panic!("expected completed item at index 2, got {:?}", events[0]);
        };
        let ThreadItemDetails::ToolInvocation(details) = &item.details else {
            panic!("expected tool invocation");
        };
        assert_eq!(details.outcome, Some(ToolOutcome::Success));
    }

    #[test]
    fn append_tool_call_delta_throttles_intermediate_updates() {
        let mut emitter = SharedLifecycleEmitter::default();
        let call_id = "call_t".to_string();
        emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
        // Clear the ItemStarted event so we only count delta-driven ItemUpdated events.
        let _ = emitter.drain_events();

        // Send 600 one-byte deltas. The first delta always emits (update_events == 0).
        // Subsequent deltas only emit after MIN_TOOL_ARG_UPDATE_BYTES (512) new bytes.
        // Expected ItemUpdated events: at 1 byte (first) and at 513 bytes (threshold met).
        for _ in 0..600 {
            emitter.append_tool_call_delta(&call_id, "x", None, None);
        }
        let events = emitter.drain_events();
        let item_updated_count = events.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
        assert_eq!(
            item_updated_count, 2,
            "expected 2 throttled ItemUpdated events for 600 1-byte deltas, got {item_updated_count}"
        );

        // The complete event must still carry the full accumulated arguments.
        emitter.complete_tool_call(&call_id, ToolCallStatus::Completed, None);
        let events = emitter.drain_events();
        let completed = events
            .iter()
            .find_map(|e| {
                if let ThreadEvent::ItemCompleted(ItemCompletedEvent { item }) = e {
                    Some(item)
                } else {
                    None
                }
            })
            .expect("should have a completed event");
        let ThreadItemDetails::ToolInvocation(details) = &completed.details else {
            panic!("expected tool invocation in completed event");
        };
        assert!(
            details.arguments.as_ref().is_some_and(|a| a.to_string().contains("xxx")),
            "completed event should carry the full accumulated arguments"
        );
    }

    #[test]
    fn append_tool_call_delta_caps_intermediate_update_events() {
        let mut emitter = SharedLifecycleEmitter::default();
        let call_id = "call_c".to_string();
        emitter.start_tool_call(&call_id, Some("exec_command".to_string()), None);
        let _ = emitter.drain_events();

        // Send 5000 one-byte deltas. With MIN_TOOL_ARG_UPDATE_BYTES=512 and
        // MAX_TOOL_ARG_UPDATE_EVENTS=8, emissions stop after 8 updates even
        // though the threshold keeps being met.
        for _ in 0..5000 {
            emitter.append_tool_call_delta(&call_id, "x", None, None);
        }
        let events = emitter.drain_events();
        let item_updated_count = events.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
        assert_eq!(
            item_updated_count, MAX_TOOL_ARG_UPDATE_EVENTS,
            "intermediate updates should be capped at MAX_TOOL_ARG_UPDATE_EVENTS, got {item_updated_count}"
        );
    }

    #[test]
    fn flush_open_tool_call_arguments_emits_full_args_for_throttled_small_calls() {
        let mut emitter = SharedLifecycleEmitter::default();
        let call_id = "call_flush".to_string();
        emitter.start_tool_call(&call_id, Some("shell".to_string()), None);
        let _ = emitter.drain_events();

        // Stream a small JSON tool call in two deltas. The first delta emits
        // eagerly (incomplete JSON); the second is below the byte threshold
        // and is throttled out.
        emitter.append_tool_call_delta(&call_id, "{\"cmd\":\"ec", None, None);
        emitter.append_tool_call_delta(&call_id, "ho hi\"}", None, None);
        let intermediate = emitter.drain_events();
        let intermediate_updated = intermediate.iter().filter(|e| matches!(e, ThreadEvent::ItemUpdated(_))).count();
        assert_eq!(intermediate_updated, 1, "first delta emits one eager update");

        // Flush should emit a final update with the full, valid-JSON arguments.
        emitter.flush_open_tool_call_arguments();
        let flushed = emitter.drain_events();
        let updated = flushed
            .iter()
            .filter_map(|e| {
                if let ThreadEvent::ItemUpdated(ItemUpdatedEvent { item }) = e {
                    Some(&item.details)
                } else {
                    None
                }
            })
            .next_back();
        let ThreadItemDetails::ToolInvocation(details) = updated.expect("flush should emit an update") else {
            panic!("expected tool invocation update");
        };
        assert_eq!(
            details.arguments.as_ref().and_then(|a| a.get("cmd")).and_then(|c| c.as_str()),
            Some("echo hi"),
            "flush should carry the full accumulated arguments"
        );
        // Tool call should still be open (no completed event).
        assert!(flushed.iter().all(|e| !matches!(e, ThreadEvent::ItemCompleted(_))));
    }

    #[test]
    fn flush_open_tool_call_arguments_skips_redundant_flush() {
        let mut emitter = SharedLifecycleEmitter::default();
        let call_id = "call_skip".to_string();
        emitter.start_tool_call(&call_id, Some("shell".to_string()), None);
        let _ = emitter.drain_events();

        // A single large delta (>= MIN_TOOL_ARG_UPDATE_BYTES) emits with the
        // full arguments, so the last_emitted_args_len already equals the
        // accumulated length. A subsequent flush should be a no-op.
        let big = "x".repeat(MIN_TOOL_ARG_UPDATE_BYTES);
        emitter.append_tool_call_delta(&call_id, &big, None, None);
        let _ = emitter.drain_events();

        emitter.flush_open_tool_call_arguments();
        let flushed = emitter.drain_events();
        assert!(flushed.is_empty(), "flush should skip when the last update already carried the full arguments");
    }
}
