use super::*;

fn pluralize<'a>(count: u64, singular: &'a str, plural: &'a str) -> &'a str {
    if count == 1 { singular } else { plural }
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
