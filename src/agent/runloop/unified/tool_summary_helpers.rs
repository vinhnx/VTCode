use hashbrown::HashSet;

use serde_json::Value;
use std::borrow::Cow;
use std::path::Path;
use vtcode_commons::formatting::truncate_middle;
pub(super) use vtcode_commons::formatting::truncate_path_middle;

pub(super) fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

pub(super) fn describe_fetch_action(_args: &Value) -> (String, HashSet<String>) {
    ("Use Fetch".into(), HashSet::new())
}

/// Ordered argument keys that may carry a shell command string.
const SHELL_COMMAND_KEYS: &[&str] = &["command", "raw_command", "bash_command", "cmd"];

/// Extract a shell command string from the common command argument keys.
///
/// Returns the (un-truncated) command text and the argument key it came from so
/// callers can record which key was used. The `command` key may be a JSON array
/// (joined with spaces) or a string; per-key emptiness/trim behavior is preserved
/// for backwards compatibility.
fn extract_command(args: &Value) -> Option<(String, &'static str)> {
    if let Some(array) = args.get("command").and_then(Value::as_array) {
        let joined: String = array
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !joined.is_empty() {
            return Some((joined, "command"));
        }
    }
    for &key in SHELL_COMMAND_KEYS {
        let Some(value) = args.get(key).and_then(Value::as_str) else { continue };
        // The `command` key trims before the emptiness check; the others do not,
        // matching historical per-key behavior.
        let (text, ok) = if key == "command" {
            let trimmed = value.trim();
            (trimmed.to_string(), !trimmed.is_empty())
        } else {
            (value.to_string(), !value.is_empty())
        };
        if ok {
            return Some((text, key));
        }
    }
    None
}

pub(super) fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let (command, key) = extract_command(args)?;
    let mut used = HashSet::new();
    used.insert(key.to_string());
    Some((truncate_middle(&command, 70), used))
}

pub(super) fn describe_list_files(
    args: &Value,
    workspace_root: Option<&Path>,
) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            let rel = relativize_to_workspace(&path, workspace_root);
            truncate_path_middle(&rel, 60)
        };
        return Some((format!("List files in {location}"), used));
    }
    if let Some(pattern) = lookup_string(args, "name_pattern") {
        let mut used = HashSet::new();
        used.insert("name_pattern".to_string());
        return Some((
            format!("Find files named {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    if let Some(pattern) = lookup_string(args, "content_pattern") {
        let mut used = HashSet::new();
        used.insert("content_pattern".to_string());
        return Some((
            format!("Search files for {}", truncate_middle(&pattern, 40)),
            used,
        ));
    }
    None
}

pub(super) fn describe_grep_file(
    args: &Value,
    workspace_root: Option<&Path>,
) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let path = lookup_string(args, "path");
    match (pattern, path) {
        (Some(pat), Some(path)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("path".to_string());
            Some((
                format!(
                    "Grep {} in {}",
                    truncate_middle(&pat, 40),
                    truncate_path_middle(&relativize_to_workspace(&path, workspace_root), 40)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Grep {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

pub(super) fn describe_path_action(
    args: &Value,
    verb: &str,
    keys: &[&str],
    workspace_root: Option<&Path>,
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let rel = relativize_to_workspace(&value, workspace_root);
            let summary = truncate_path_middle(&rel, 60);
            let annotated_summary = annotate_skill_doc_summary(rel.as_ref(), summary);
            return Some((format!("{verb} {annotated_summary}"), used));
        }
    }
    None
}

fn annotate_skill_doc_summary(raw_path: &str, summary: String) -> String {
    let path = Path::new(raw_path.trim());
    let is_skill_doc = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("SKILL.md"));
    if !is_skill_doc {
        return summary;
    }

    let Some(skill_name) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
    else {
        return summary;
    };

    format!("{summary} ({skill_name} skill)")
}

pub(super) fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Keys whose values are file-system paths and should be displayed relative to
/// the workspace root when possible.
fn is_path_key(key: &str) -> bool {
    matches!(
        key,
        "path" | "file_path" | "filename" | "destination" | "source"
    )
}

/// Relativize an absolute `path` against the `workspace_root` for compact display.
///
/// Returns the path unchanged when `workspace_root` is `None`, the path is not
/// absolute, or it does not lie within the workspace root.
pub(super) fn relativize_to_workspace<'a>(
    path: &'a str,
    workspace_root: Option<&Path>,
) -> Cow<'a, str> {
    let Some(root) = workspace_root else {
        return Cow::Borrowed(path);
    };
    let p = Path::new(path);
    if p.is_absolute() {
        if let Ok(rel) = p.strip_prefix(root) {
            // `rel` is empty only when the path equals the root itself; keep the
            // original form in that degenerate case for clarity.
            if !rel.as_os_str().is_empty() {
                return Cow::Owned(rel.to_string_lossy().into_owned());
            }
        }
    }
    Cow::Borrowed(path)
}

pub(super) fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap_or_default();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}





pub(super) fn collect_param_details(
    args: &Value,
    keys: &HashSet<String>,
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    let include_all = keys.is_empty();
    for (key, value) in map {
        // Skip command-related and raw content keys (too verbose in summaries)
        if matches!(
            key.as_str(),
            "command"
                | "raw_command"
                | "bash_command"
                | "cmd"
                | "old_str"
                | "new_str"
                | "content"
                | "new_content"
                | "text"
                | "patch"
                | "code"
        ) {
            continue;
        }
        // Skip infrastructure/plumbing parameters that are implementation details
        if is_noise_param(key) {
            continue;
        }
        if !include_all && keys.contains(key) {
            continue;
        }
        match value {
            Value::String(s) if !s.is_empty() => {
                // Render file-system path values relative to the workspace root.
                let display: Cow<'_, str> = if is_path_key(key) {
                    relativize_to_workspace(s, workspace_root)
                } else {
                    Cow::Borrowed(s.as_str())
                };
                details.push(format!(
                    "{}: {}",
                    humanize_key(key),
                    truncate_middle(&display, 60)
                ))
            }
            Value::Bool(true) => {
                details.push(humanize_key(key));
            }
            Value::Array(items) => {
                let strings: Vec<String> = items
                    .iter()
                    .filter_map(|item| item.as_str().map(|s| s.to_string()))
                    .collect();
                if !strings.is_empty() {
                    details.push(format!(
                        "{}: {}",
                        humanize_key(key),
                        summarize_list(&strings, 2, 60)
                    ));
                }
            }
            Value::Number(num) => {
                // Skip zero-valued numbers — they are defaults and add no information
                if num.as_f64().is_some_and(|n| n == 0.0) {
                    continue;
                }
                details.push(format!("{}: {}", humanize_key(key), num));
            }
            _ => {}
        }
    }
    details
}

/// Returns `true` for parameter keys that are infrastructure/plumbing noise
/// and should be omitted from the human-facing transcript summary.
fn is_noise_param(key: &str) -> bool {
    matches!(
        key,
        // Timeouts and size limits
        "timeout_secs"
            | "timeout"
            | "max_bytes"
            | "max_matches"
            // Search plumbing
            | "debug_query"
            | "strictness"
            | "case_sensitive"
            | "literal"
            | "context_lines"
            // Execution plumbing
            | "shell"
            | "login"
            | "tty"
            | "sandbox_permissions"
            | "additional_permissions"
            | "justification"
            | "prefix_rule"
            | "workdir"
            | "cwd"
            | "language"
            | "spool_path"
            | "query"
            // Tool identity / routing
            | "type"
            | "tool_call_id"
            | "call_type"
            // Redundant with summary headline (e.g., "Read file" already implies action=read)
            | "action"
    )
}

pub(super) fn should_render_command_line(highlights: &HashSet<String>) -> bool {
    highlights.is_empty()
        || (!highlights.contains("command")
            && !highlights.contains("raw_command")
            && !highlights.contains("bash_command")
            && !highlights.contains("cmd"))
}

pub(super) fn command_line_for_args(args: &Value) -> Option<String> {
    let (command, _) = extract_command(args)?;
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_middle(trimmed, 120))
}

pub(super) fn highlight_texts_for_summary(
    args: &Value,
    highlights: &HashSet<String>,
    workspace_root: Option<&Path>,
) -> Vec<String> {
    let mut values = Vec::new();
    for key in highlights {
        if let Some(value) = lookup_string(args, key) {
            let limit = match key.as_str() {
                "pattern" | "name_pattern" | "content_pattern" => 40,
                "command" | "raw_command" | "bash_command" => 70,
                _ => 60,
            };
            // Render file-system path values relative to the workspace root.
            let display: Cow<'_, str> = if is_path_key(key) {
                relativize_to_workspace(&value, workspace_root)
            } else {
                Cow::Borrowed(&value)
            };
            values.push(truncate_middle(&display, limit));
        }
    }
    values
}

pub(super) fn summarize_list(items: &[String], max_items: usize, max_len: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let shown: Vec<String> = items
        .iter()
        .take(max_items)
        .map(|s| truncate_middle(s, max_len))
        .collect();
    if items.len() > max_items {
        format!("{} +{} more", shown.join(", "), items.len() - max_items)
    } else {
        shown.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_describe_shell_command_new_format() {
        let args = json!({
            "command": ["bash", "-lc", "ls -R"]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "bash -lc ls -R");
    }

    #[test]
    fn test_describe_shell_command_bash_command_format() {
        let args = json!({
            "bash_command": "pwd"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "pwd");
    }

    #[test]
    fn test_describe_shell_command_truncation() {
        let long_command = "a".repeat(100);
        let args = json!({
            "command": [long_command]
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert!(description.contains("…"));
    }

    #[test]
    fn test_describe_shell_command_string_format() {
        let args = json!({
            "command": "cargo check -p vtcode"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "cargo check -p vtcode");
    }

    #[test]
    fn test_describe_shell_command_raw_command_fallback() {
        let args = json!({
            "raw_command": "cargo test -- --nocapture"
        });

        let result = describe_shell_command(&args);
        assert!(result.is_some());

        let (description, _used) = result.unwrap();
        assert_eq!(description, "cargo test -- --nocapture");
    }

    #[test]
    fn collect_param_details_skips_noise_params() {
        let args = json!({
            "action": "grep",
            "pattern": "agent loop",
            "strictness": "relaxed",
            "debug_query": "pattern",
            "detail_level": "full",
            "max_results": 20,
            "context_lines": 2,
            "scope": "repo",
            "max_bytes": 6000,
            "timeout_secs": 120
        });
        let mut keys = HashSet::new();
        keys.insert("pattern".to_string());
        let details = collect_param_details(&args, &keys, None);
        // Only detail_level, max_results, and scope should remain;
        // pattern is in keys (highlighted), noise params (including action) are skipped.
        for detail in &details {
            assert!(
                !detail.contains("Timeout")
                    && !detail.contains("Max bytes")
                    && !detail.contains("Debug query")
                    && !detail.contains("Strictness")
                    && !detail.contains("Context lines")
                    && !detail.contains("Action"),
                "Noise param leaked through: {detail}"
            );
        }
    }

    #[test]
    fn collect_param_details_skips_zero_numbers() {
        let args = json!({
            "action": "read",
            "path": "src/main.rs",
            "start_line": 1,
            "end_line": 200,
            "offset": 0,
            "limit": 0
        });
        let mut keys = HashSet::new();
        keys.insert("path".to_string());
        let details = collect_param_details(&args, &keys, None);
        for detail in &details {
            assert!(
                !detail.contains("Offset") && !detail.contains("Limit"),
                "Zero-valued param leaked through: {detail}"
            );
        }
        assert!(details.iter().any(|d| d.contains("Start line: 1")));
        assert!(details.iter().any(|d| d.contains("End line: 200")));
    }

    #[test]
    fn is_noise_param_matches_expected_keys() {
        assert!(is_noise_param("timeout_secs"));
        assert!(is_noise_param("max_bytes"));
        assert!(is_noise_param("debug_query"));
        assert!(is_noise_param("strictness"));
        assert!(is_noise_param("case_sensitive"));
        assert!(is_noise_param("context_lines"));
        assert!(is_noise_param("shell"));
        assert!(is_noise_param("sandbox_permissions"));
        assert!(is_noise_param("action")); // Redundant with summary headline
        // Read file params should pass through (not noise)
        assert!(!is_noise_param("offset"));
        assert!(!is_noise_param("limit"));
        assert!(!is_noise_param("head_lines"));
        assert!(!is_noise_param("tail_lines"));
        assert!(!is_noise_param("start_line"));
        assert!(!is_noise_param("end_line"));
        // Meaningful params should pass through
        assert!(!is_noise_param("pattern"));
        assert!(!is_noise_param("path"));
        assert!(!is_noise_param("mode"));
    }

    #[test]
    fn truncate_path_middle_breaks_at_separator() {
        let path = "/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/hello/src/main.rs";
        let truncated = truncate_path_middle(path, 40);
        // Should break at a '/' not in the middle of a word
        assert!(truncated.contains("…"));
        // The character after '…' should be a '/' or start of a path component
        if let Some(char_idx) = truncated.char_indices().find(|(_, c)| *c == '…') {
            let after: String = truncated[char_idx.0 + '…'.len_utf8()..].chars().collect();
            assert!(
                after.starts_with('/') || after.starts_with('h') || after.starts_with('s'),
                "Expected path break after ellipsis, got: {after}"
            );
        }
    }

    #[test]
    fn truncate_path_middle_short_path_not_truncated() {
        let path = "src/main.rs";
        let truncated = truncate_path_middle(path, 40);
        assert_eq!(truncated, "src/main.rs");
    }
}
