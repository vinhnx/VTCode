//! Transcript normalization helpers for session archives.

use crate::tools::tool_intent::canonical_command_session_tool_name;
use rustc_hash::FxHashSet;
use vtcode_commons::formatting::collapse_whitespace as collapse_whitespace_inner;

pub(crate) fn collapse_whitespace(text: &str) -> String {
    collapse_whitespace_inner(text)
}

pub fn should_drop_transcript_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("Latest tool output:")
        || trimmed.starts_with("Latest user request:")
        || trimmed.starts_with("Tool output 1:")
        || trimmed.starts_with("Structured result with fields:")
        || trimmed.starts_with("Reuse the latest tool outputs already collected in this turn")
        || trimmed.starts_with("Interrupt received. Stopping task...")
}

pub fn normalize_recovery_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("[!] Turn balancer:")
        || trimmed.starts_with("[!] Navigation Loop:")
        || trimmed.starts_with("[!] Navigation loop:")
    {
        return Some("Repeated low-signal tool churn triggered recovery.".to_string());
    }

    if trimmed
        .contains("I couldn't produce a final synthesis because the model returned no answer on the recovery pass.")
    {
        return Some("Recovery pass failed to produce a final synthesis.".to_string());
    }

    None
}

pub fn summarize_tool_block(lines: &[String], start: usize) -> (String, usize) {
    let header = lines[start].trim().to_string();
    let mut command_continuations = Vec::new();
    let mut metadata = Vec::new();
    let mut metadata_seen = FxHashSet::default();
    let mut index = start + 1;

    while index < lines.len() {
        let raw = lines[index].trim_end();
        let trimmed = raw.trim_start();
        if trimmed.starts_with("• ") || trimmed.starts_with("[!]") || !is_tool_detail_line(trimmed) {
            break;
        }

        if let Some(continuation) = trimmed.strip_prefix("│ ") {
            let continuation = continuation.trim();
            if !continuation.is_empty() {
                command_continuations.push(continuation.to_string());
            }
        } else if let Some(extra) = summarize_tool_detail(trimmed)
            && metadata_seen.insert(extra.clone())
        {
            metadata.push(extra);
        }

        index += 1;
    }

    let mut summary = header;
    if !command_continuations.is_empty() {
        summary.push(' ');
        summary.push_str(&command_continuations.join(" "));
    }
    if !metadata.is_empty() {
        summary.push_str(" [");
        summary.push_str(&metadata.join(", "));
        summary.push(']');
    }

    (collapse_whitespace(&summary), index)
}

pub(crate) fn is_tool_detail_line(line: &str) -> bool {
    line.starts_with("│ ")
        || line.starts_with("└ ")
        || line.starts_with("✓ ")
        || line.starts_with("✗ ")
        || line.starts_with("… +")
        || line.starts_with("Large output was spooled")
        || line == "(no output)"
}

pub(crate) fn summarize_tool_detail(line: &str) -> Option<String> {
    let path = line
        .strip_prefix("└ Path:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("path {value}"));
    if path.is_some() {
        return path;
    }

    let pattern = line
        .strip_prefix("└ Pattern:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("pattern {value}"));
    if pattern.is_some() {
        return pattern;
    }

    let filter = line
        .strip_prefix("└ Filter:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("filter {value}"));
    if filter.is_some() {
        return filter;
    }

    let glob = line
        .strip_prefix("└ Glob:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("glob {value}"));
    if glob.is_some() {
        return glob;
    }

    if let Some(status) = line.strip_prefix("✗ ") {
        let status = status.trim();
        if !status.is_empty() {
            return Some(status.to_string());
        }
    }

    None
}

pub fn normalized_transcript_key(text: &str) -> String {
    collapse_whitespace(text).to_ascii_lowercase()
}

pub fn format_repeated_summary(line: &str, repeats: usize) -> String {
    if repeats <= 1 {
        return line.to_string();
    }
    format!("{line} (repeated x{repeats})")
}

pub fn push_clean_transcript_line(target: &mut Vec<String>, line: String) {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        if target.last().is_none_or(|last| !last.is_empty()) {
            target.push(String::new());
        }
        return;
    }

    if target
        .last()
        .is_some_and(|last| normalized_transcript_key(last) == normalized_transcript_key(trimmed))
    {
        return;
    }

    target.push(line);
}

pub fn normalize_session_tool_name(name: &str) -> String {
    canonical_command_session_tool_name(name).unwrap_or(name).to_string()
}

pub fn normalize_distinct_tools_for_summary(distinct_tools: &[String]) -> Vec<String> {
    let mut normalized = Vec::with_capacity(distinct_tools.len());
    let mut seen = std::collections::BTreeSet::new();

    for tool in distinct_tools {
        let mapped = normalize_session_tool_name(tool);
        if seen.insert(mapped.clone()) {
            normalized.push(mapped);
        }
    }

    normalized
}
