use std::collections::HashSet;

use anyhow::Result;
use serde_json::Value;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_tool_call_summary(
    renderer: &mut AnsiRenderer,
    tool_name: &str,
    args: &Value,
) -> Result<()> {
    let (headline, _) = describe_tool_action(tool_name, args);
    renderer.line(MessageStyle::Info, &format!("→ {}", headline))?;

    Ok(())
}

pub(crate) fn describe_tool_action(tool_name: &str, args: &Value) -> (String, HashSet<String>) {
    match tool_name {
        tool_names::RUN_TERMINAL_CMD | tool_names::BASH => describe_shell_command(args)
            .unwrap_or_else(|| ("Run shell command".to_string(), HashSet::new())),
        tool_names::LIST_FILES => {
            describe_list_files(args).unwrap_or_else(|| ("List files".to_string(), HashSet::new()))
        }
        tool_names::GREP_SEARCH => describe_grep_search(args)
            .unwrap_or_else(|| ("Search with grep".to_string(), HashSet::new())),
        tool_names::READ_FILE => describe_path_action(args, "Read file", &["path"])
            .unwrap_or_else(|| ("Read file".to_string(), HashSet::new())),
        tool_names::WRITE_FILE => describe_path_action(args, "Write file", &["path"])
            .unwrap_or_else(|| ("Write file".to_string(), HashSet::new())),
        tool_names::EDIT_FILE => describe_path_action(args, "Edit file", &["path"])
            .unwrap_or_else(|| ("Edit file".to_string(), HashSet::new())),
        tool_names::CREATE_FILE => describe_path_action(args, "Create file", &["path"])
            .unwrap_or_else(|| ("Create file".to_string(), HashSet::new())),
        tool_names::DELETE_FILE => describe_path_action(args, "Delete file", &["path"])
            .unwrap_or_else(|| ("Delete file".to_string(), HashSet::new())),
        tool_names::CURL => {
            describe_curl(args).unwrap_or_else(|| ("Fetch URL".to_string(), HashSet::new()))
        }
        tool_names::SIMPLE_SEARCH => describe_simple_search(args)
            .unwrap_or_else(|| ("Search workspace".to_string(), HashSet::new())),
        tool_names::SRGN => describe_srgn(args)
            .unwrap_or_else(|| ("Search and replace".to_string(), HashSet::new())),
        tool_names::APPLY_PATCH => ("Apply workspace patch".to_string(), HashSet::new()),
        tool_names::UPDATE_PLAN => ("Update task plan".to_string(), HashSet::new()),
        _ => (
            format!("Use {}", humanize_tool_name(tool_name)),
            HashSet::new(),
        ),
    }
}

pub(crate) fn humanize_tool_name(name: &str) -> String {
    humanize_key(name)
}

fn describe_shell_command(args: &Value) -> Option<(String, HashSet<String>)> {
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
        let summary = truncate_middle(&joined, 60);
        return Some((format!("Run command {}", summary), used));
    }

    if let Some(cmd) = args
        .get("bash_command")
        .and_then(|value| value.as_str())
        .filter(|s| !s.is_empty())
    {
        used.insert("bash_command".to_string());
        let summary = truncate_middle(cmd, 60);
        return Some((format!("Run bash {}", summary), used));
    }

    None
}

fn describe_list_files(args: &Value) -> Option<(String, HashSet<String>)> {
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

fn describe_grep_search(args: &Value) -> Option<(String, HashSet<String>)> {
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

fn describe_simple_search(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(query) = lookup_string(args, "query") {
        let mut used = HashSet::new();
        used.insert("query".to_string());
        return Some((format!("Search for {}", truncate_middle(&query, 50)), used));
    }
    None
}

fn describe_srgn(args: &Value) -> Option<(String, HashSet<String>)> {
    let pattern = lookup_string(args, "pattern");
    let replacement = lookup_string(args, "replacement");
    match (pattern, replacement) {
        (Some(pat), Some(rep)) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            used.insert("replacement".to_string());
            Some((
                format!(
                    "Replace {} → {}",
                    truncate_middle(&pat, 30),
                    truncate_middle(&rep, 30)
                ),
                used,
            ))
        }
        (Some(pat), None) => {
            let mut used = HashSet::new();
            used.insert("pattern".to_string());
            Some((format!("Search for {}", truncate_middle(&pat, 40)), used))
        }
        _ => None,
    }
}

fn describe_path_action(
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

fn describe_curl(args: &Value) -> Option<(String, HashSet<String>)> {
    if let Some(url) = lookup_string(args, "url") {
        let mut used = HashSet::new();
        used.insert("url".to_string());
        return Some((format!("Fetch {}", truncate_middle(&url, 60)), used));
    }
    None
}

fn lookup_string(args: &Value, key: &str) -> Option<String> {
    args.as_object()
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn humanize_key(key: &str) -> String {
    let replaced = key.replace('_', " ");
    if replaced.is_empty() {
        return replaced;
    }
    let mut chars = replaced.chars();
    let first = chars.next().unwrap();
    let mut result = first.to_uppercase().collect::<String>();
    result.push_str(&chars.collect::<String>());
    result
}

fn truncate_middle(text: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max_len {
        return text.to_string();
    }
    if max_len <= 1 {
        return "…".to_string();
    }
    let head_len = max_len / 2;
    let tail_len = max_len.saturating_sub(head_len + 1);
    let mut result: String = chars.iter().take(head_len).collect();
    result.push('…');
    if tail_len > 0 {
        let tail: String = chars
            .iter()
            .rev()
            .take(tail_len)
            .cloned()
            .collect::<Vec<char>>()
            .into_iter()
            .rev()
            .collect();
        result.push_str(&tail);
    }
    result
}
