use hashbrown::HashMap;
use regex::Regex;
use std::sync::LazyLock;

use crate::agent::runloop::unified::config_section_headings::normalize_config_path;

const FIELD_REFERENCE_MARKDOWN: &str = include_str!("../../../../../docs/config/config_field_reference.md");

#[derive(Debug, Clone, Default)]
pub(super) struct FieldDoc {
    pub type_name: String,
    pub default_value: String,
    pub description: String,
    pub options: Vec<String>,
}

#[derive(Debug, Default)]
pub(super) struct FieldDocIndex {
    by_path: HashMap<String, FieldDoc>,
}

impl FieldDocIndex {
    pub(super) fn lookup(&self, path: &str) -> Option<&FieldDoc> {
        let normalized = normalize_config_path(path);
        if let Some(doc) = self.by_path.get(path).or_else(|| self.by_path.get(&normalized)) {
            return Some(doc);
        }

        let mut segments: Vec<&str> = normalized.split('.').collect();
        for index in 0..segments.len() {
            if matches!(segments[index], "*" | "[]") {
                continue;
            }

            let original = segments[index];
            segments[index] = "*";
            let candidate = segments.join(".");
            if let Some(doc) = self.by_path.get(&candidate) {
                return Some(doc);
            }
            segments[index] = original;
        }

        None
    }

    pub(super) fn sorted_paths(&self) -> Vec<&str> {
        let mut paths: Vec<&str> = self.by_path.keys().map(String::as_str).collect();
        paths.sort_unstable();
        paths
    }
}

static QUOTED_VALUE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\"([^\"]+)\""#).expect("valid regex"));
pub(super) static FIELD_DOCS: LazyLock<FieldDocIndex> = LazyLock::new(parse_field_reference_markdown);

fn parse_field_reference_markdown() -> FieldDocIndex {
    let mut by_path = HashMap::new();

    for raw_line in FIELD_REFERENCE_MARKDOWN.lines() {
        let line = raw_line.trim();
        if !line.starts_with('|') {
            continue;
        }
        if line.contains("| Field | Type | Required | Default | Description |") {
            continue;
        }
        if line.contains("|-------") {
            continue;
        }

        let columns = split_markdown_row(line);
        if columns.len() < 7 {
            continue;
        }

        let field = columns[1].trim().trim_matches('`').to_string();
        if field.is_empty() {
            continue;
        }

        let type_name = columns[2].trim().to_string();
        let default_value = columns[4].trim().trim_matches('`').to_string();
        let description = columns[5].trim().to_string();

        let options = extract_options(&description);
        by_path.insert(field, FieldDoc { type_name, default_value, description, options });
    }

    FieldDocIndex { by_path }
}

fn split_markdown_row(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '|' {
            cells.push(current.trim().to_string());
            current.clear();
            continue;
        }

        current.push(ch);
    }

    cells.push(current.trim().to_string());
    cells
}

fn extract_options(description: &str) -> Vec<String> {
    let options_source = description
        .split_once("Options:")
        .map(|(_, tail)| tail)
        .or_else(|| description.split_once("options:").map(|(_, tail)| tail));

    let mut values: Vec<String> = Vec::new();

    if let Some(source) = options_source {
        for capture in QUOTED_VALUE_RE.captures_iter(source) {
            if let Some(value) = capture.get(1) {
                values.push(value.as_str().to_string());
            }
        }

        if values.is_empty() {
            values.extend(extract_comma_options(source));
        }
    }

    if values.is_empty()
        && let (Some(start), Some(end)) = (description.find('('), description.find(')'))
        && end > start
    {
        let inside = &description[start + 1..end];
        values.extend(extract_comma_options(inside));
    }

    values.sort();
    values.dedup();
    values
}

fn extract_comma_options(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .map(|entry| entry.trim_matches('\"').trim_matches('\''))
        .filter(|entry| !entry.is_empty())
        .filter(|entry| {
            entry
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/'))
        })
        .map(str::to_string)
        .collect()
}
