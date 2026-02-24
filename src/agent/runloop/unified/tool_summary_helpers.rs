use std::collections::HashSet;

use serde_json::Value;

pub(super) fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

pub(super) fn describe_fetch_action(_args: &Value) -> (String, HashSet<String>) {
    ("Use Fetch".into(), HashSet::new())
}

pub(super) fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
    let mut used = HashSet::new();
    if let Some(parts) = args
        .get("command")
        .and_then(|value| value.as_array())
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|parts: &Vec<String>| !parts.is_empty())
    {
        used.insert("command".to_string());
        let joined = parts.join(" ");
        let summary = truncate_middle(&joined, 70);
        return Some((summary, used));
    }

    if let Some(cmd) = args
        .get("command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        used.insert("command".to_string());
        let summary = truncate_middle(cmd.trim(), 70);
        return Some((summary, used));
    }

    if let Some(cmd) = args
        .get("raw_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("raw_command".to_string());
        let summary = truncate_middle(cmd, 70);
        return Some((summary, used));
    }

    if let Some(cmd) = args
        .get("cmd")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("cmd".to_string());
        let summary = truncate_middle(cmd, 70);
        return Some((summary, used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 70);
        return Some((summary, used));
    }

    None
}

pub(super) fn describe_list_files(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(path) = lookup_string(args, "path") {
        let mut used = HashSet::new();
        used.insert("path".to_string());
        let location = if path == "." {
            "workspace root".to_string()
        } else {
            truncate_middle(&path, 60)
        };
        return Some((format!("List files in {}", location), used));
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

pub(super) fn describe_grep_file(args: &Value) -> Option<(String, HashSet<String>)> {
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
                    truncate_middle(&path, 40)
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
) -> Option<(String, HashSet<String>)> {
    for key in keys {
        if let Some(value) = lookup_string(args, key) {
            let mut used = HashSet::new();
            used.insert((*key).to_string());
            let summary = truncate_middle(&value, 60);
            return Some((format!("{} {}", verb, summary), used));
        }
    }
    None
}

pub(super) fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
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

pub(super) fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let sanitized: String = text
        .chars()
        .map(|c| {
            if matches!(c, '\n' | '\r' | '\t') {
                ' '
            } else {
                c
            }
        })
        .collect();
    let char_count = sanitized.chars().count();
    if char_count <= max_len {
        return sanitized;
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);

    let head: String = sanitized.chars().take(head_len).collect();
    let mut result = String::with_capacity(head.len() + tail_len + 1);
    result.push_str(&head);
    result.push('…');
    if tail_len > 0 {
        let mut tail_rev: Vec<char> = sanitized.chars().rev().take(tail_len).collect();
        tail_rev.reverse();
        let tail: String = tail_rev.into_iter().collect();
        result.push_str(&tail);
    }
    result
}

pub(super) fn collect_param_details(args: &Value, keys: &HashSet<String>) -> Vec<String> {
    let mut details = Vec::new();
    let Some(map) = args.as_object() else {
        return details;
    };
    let include_all = keys.is_empty();
    for (key, value) in map {
        // Skip command-related keys and edit_file content keys (too verbose)
        if matches!(
            key.as_str(),
            "command" | "raw_command" | "bash_command" | "cmd" | "old_str" | "new_str"
        ) {
            continue;
        }
        if !include_all && keys.contains(key) {
            continue;
        }
        match value {
            Value::String(s) if !s.is_empty() => {
                details.push(format!("{}: {}", humanize_key(key), truncate_middle(s, 60)))
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
                details.push(format!("{}: {}", humanize_key(key), num));
            }
            _ => {}
        }
    }
    details
}

pub(super) fn should_render_command_line(highlights: &HashSet<String>) -> bool {
    highlights.is_empty()
        || (!highlights.contains("command")
            && !highlights.contains("raw_command")
            && !highlights.contains("bash_command")
            && !highlights.contains("cmd"))
}

pub(super) fn command_line_for_args(args: &Value) -> Option<String> {
    let command = if let Some(array) = args.get("command").and_then(Value::as_array) {
        let joined = array
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    } else {
        args.get("command")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                args.get("raw_command")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                args.get("bash_command")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| args.get("cmd").and_then(Value::as_str).map(str::to_string))
    }?;

    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_middle(trimmed, 120))
}

pub(super) fn highlight_texts_for_summary(
    args: &Value,
    highlights: &HashSet<String>,
) -> Vec<String> {
    let mut values = Vec::new();
    for key in highlights {
        if let Some(value) = lookup_string(args, key) {
            let limit = match key.as_str() {
                "pattern" | "name_pattern" | "content_pattern" => 40,
                "command" | "raw_command" | "bash_command" => 70,
                _ => 60,
            };
            values.push(truncate_middle(&value, limit));
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
}
