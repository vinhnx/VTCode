use toml::Value as TomlValue;
use vtcode_tui::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::config_section_headings::{
    heading_for_path, humanize_identifier,
};

use super::docs::FieldDoc;
pub(super) fn display_title(label: &str, path: &str, value: &TomlValue) -> String {
    if label.starts_with('[') {
        return format!("Item {}", label);
    }

    match value {
        TomlValue::Table(_) => heading_for_path(path).title.into_owned(),
        _ => humanize_identifier(label),
    }
}

pub(super) fn section_subtitle(path: &str, value: &TomlValue) -> String {
    let heading = heading_for_path(path);
    let count = count_leaf_entries(value);
    let mut parts = Vec::new();
    if !heading.summary.is_empty() {
        parts.push(heading.summary.into_owned());
    }
    parts.push(format!(
        "{} setting{}",
        count,
        if count == 1 { "" } else { "s" }
    ));
    parts.join(" • ")
}

pub(super) fn setting_subtitle(summary: &str, description: &str, adjustable: bool) -> String {
    let value_display = if adjustable {
        format!("<- {} ->", summary)
    } else {
        summary.to_string()
    };
    let mut parts = vec![value_display];
    if !description.is_empty() {
        parts.push(description.to_string());
    }
    parts.join(" • ")
}

pub(super) fn collection_subtitle(summary: String, description: &str) -> String {
    let mut parts = vec![summary];
    if !description.is_empty() {
        parts.push(description.to_string());
    }
    parts.join(" • ")
}

pub(super) fn search_value_for_missing_doc(
    path: &str,
    label: &str,
    doc: Option<&FieldDoc>,
) -> String {
    let mut parts = vec![path.to_string(), label.to_string(), "unset".to_string()];
    if let Some(doc) = doc {
        if !doc.description.is_empty() {
            parts.push(doc.description.clone());
        }
        if !doc.options.is_empty() {
            parts.push(doc.options.join(" "));
        }
    }
    parts.join(" ").to_ascii_lowercase()
}

pub(super) fn section_item(label: &str) -> InlineListItem {
    InlineListItem {
        title: label.to_string(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    }
}

pub(super) fn action_item(
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    action: &str,
) -> InlineListItem {
    InlineListItem {
        title: title.to_string(),
        subtitle: Some(subtitle.to_string()),
        badge: badge.map(str::to_string),
        indent: 0,
        selection: Some(InlineListSelection::ConfigAction(action.to_string())),
        search_value: Some(format!("{} {}", title, subtitle)),
    }
}

pub(super) fn search_value_with_content(
    path: &str,
    label: &str,
    value: &TomlValue,
    doc: Option<&FieldDoc>,
) -> String {
    let mut parts = vec![
        path.to_string(),
        label.to_string(),
        display_title(label, path, value),
    ];

    if matches!(value, TomlValue::Table(_)) {
        let heading = heading_for_path(path);
        if !heading.summary.is_empty() {
            parts.push(heading.summary.into_owned());
        }
    }

    collect_search_terms(path, value, &mut parts);

    if let Some(doc) = doc {
        if !doc.description.is_empty() {
            parts.push(doc.description.clone());
        }
        if !doc.options.is_empty() {
            parts.push(doc.options.join(" "));
        }
    }
    parts.join(" ").to_ascii_lowercase()
}

fn collect_search_terms(path: &str, value: &TomlValue, parts: &mut Vec<String>) {
    match value {
        TomlValue::String(value) => parts.push(value.clone()),
        TomlValue::Integer(value) => parts.push(value.to_string()),
        TomlValue::Float(value) => parts.push(value.to_string()),
        TomlValue::Boolean(value) => {
            parts.push(value.to_string());
            parts.push(if *value { "on" } else { "off" }.to_string());
        }
        TomlValue::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{}[{}]", path, index);
                parts.push(child_path.clone());
                collect_search_terms(&child_path, child, parts);
            }
        }
        TomlValue::Table(table) => {
            for key in sorted_table_keys(table) {
                let Some(child) = table.get(key) else {
                    continue;
                };
                let child_path = format!("{}.{}", path, key);
                parts.push(child_path.clone());
                parts.push(humanize_identifier(key));
                collect_search_terms(&child_path, child, parts);
            }
        }
        _ => {}
    }
}

pub(super) fn sorted_table_keys(table: &toml::map::Map<String, TomlValue>) -> Vec<&String> {
    let mut keys: Vec<&String> = table.keys().collect();
    keys.sort();
    keys
}

pub(super) fn count_leaf_entries(value: &TomlValue) -> usize {
    match value {
        TomlValue::Table(table) => table.values().map(count_leaf_entries).sum(),
        TomlValue::Array(values) => values.len().max(1),
        _ => 1,
    }
}

pub(super) fn summarize_value(value: &TomlValue) -> String {
    match value {
        TomlValue::String(text) => format!("\"{}\"", truncate_middle(text, 48)),
        TomlValue::Integer(number) => number.to_string(),
        TomlValue::Float(number) => number.to_string(),
        TomlValue::Boolean(value) => {
            if *value {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        TomlValue::Array(values) => format!(
            "{} item{}",
            values.len(),
            if values.len() == 1 { "" } else { "s" }
        ),
        TomlValue::Table(_) => {
            let count = count_leaf_entries(value);
            format!("{} setting{}", count, if count == 1 { "" } else { "s" })
        }
        _ => "<unsupported>".to_string(),
    }
}

fn truncate_middle(value: &str, max_len: usize) -> String {
    let total_chars = value.chars().count();
    if total_chars <= max_len {
        return value.to_string();
    }
    if max_len <= 3 {
        return "...".to_string();
    }

    let prefix_len = max_len / 2;
    let suffix_len = max_len.saturating_sub(prefix_len + 3);
    let prefix: String = value.chars().take(prefix_len).collect();
    let suffix: String = value
        .chars()
        .skip(total_chars.saturating_sub(suffix_len))
        .collect();
    format!("{prefix}...{suffix}")
}
