use toml::Value as TomlValue;
use vtcode_ui::tui::app::{InlineListItem, InlineListSelection};

use crate::agent::runloop::unified::config_section_headings::{heading_for_path, humanize_identifier};

use super::docs::FieldDoc;
use super::path::path_with_key;
use vtcode_commons::formatting::{format_float_display, truncate_middle};

pub(super) fn display_title(label: &str, path: &str, value: &TomlValue) -> String {
    if label.starts_with('[') && !ends_with_quoted_map_key(path) {
        return format!("Item {label}");
    }

    match value {
        TomlValue::Table(_) if ends_with_quoted_map_key(path) => label.to_string(),
        TomlValue::Table(_) => heading_for_path(path).title.into_owned(),
        _ => humanize_identifier(label),
    }
}

fn ends_with_quoted_map_key(path: &str) -> bool {
    path.rfind("[\"").is_some_and(|start| path[start..].ends_with("\"]"))
}

pub(super) fn search_value_for_missing_doc(path: &str, label: &str, doc: Option<&FieldDoc>) -> String {
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
    vtcode_ui::design::list::group_header(label)
}

/// Action row with default tone: badgeful rows get Accent, badgeless Neutral.
/// Prefer [`action_item_with_tone`] when the tone is semantic (danger/current).
pub(super) fn action_item(title: &str, subtitle: &str, badge: Option<&str>, action: &str) -> InlineListItem {
    let tone = if badge.is_some() {
        vtcode_commons::ui_protocol::InlineTone::Accent
    } else {
        vtcode_commons::ui_protocol::InlineTone::Neutral
    };
    action_item_with_tone(title, subtitle, badge, action, tone)
}

pub(super) fn action_item_with_tone(
    title: &str,
    subtitle: &str,
    badge: Option<&str>,
    action: &str,
    tone: vtcode_commons::ui_protocol::InlineTone,
) -> InlineListItem {
    vtcode_ui::design::list::action(
        title,
        subtitle,
        badge.map(str::to_string),
        tone,
        Some(InlineListSelection::ConfigAction(action.to_string())),
    )
}

pub(super) fn search_value_with_content(path: &str, label: &str, value: &TomlValue, doc: Option<&FieldDoc>) -> String {
    let mut parts = vec![path.to_string(), label.to_string(), display_title(label, path, value)];

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
        TomlValue::Float(value) => parts.push(format_float_display(*value)),
        TomlValue::Boolean(value) => {
            parts.push(value.to_string());
            parts.push(if *value { "on" } else { "off" }.to_string());
        }
        TomlValue::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let child_path = format!("{path}[{index}]");
                parts.push(child_path.clone());
                collect_search_terms(&child_path, child, parts);
            }
        }
        TomlValue::Table(table) => {
            for key in sorted_table_keys(table) {
                let Some(child) = table.get(key) else {
                    continue;
                };
                let child_path = path_with_key(path, key);
                parts.push(child_path.clone());
                parts.push(humanize_identifier(key));
                collect_search_terms(&child_path, child, parts);
            }
        }
        _ => {}
    }
}

pub(super) fn sorted_table_keys(table: &toml::map::Map<String, TomlValue>) -> Vec<&str> {
    let mut keys: Vec<&str> = table.keys().map(|s| s.as_str()).collect();
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
        TomlValue::String(text) => truncate_middle(text, 48),
        TomlValue::Integer(number) => number.to_string(),
        TomlValue::Float(number) => format_float_display(*number),
        TomlValue::Boolean(value) => {
            if *value {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        }
        TomlValue::Array(values) => {
            format!("{} item{}", values.len(), if values.len() == 1 { "" } else { "s" })
        }
        TomlValue::Table(_) => {
            let count = count_leaf_entries(value);
            format!("{} setting{}", count, if count == 1 { "" } else { "s" })
        }
        _ => "<unsupported>".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_value_collapses_f32_temperature_artifact() {
        // Screenshot artifact: `temperature = 0.7` (`f32`) rendered as
        // `0.699999988079071` after widening to `f64`, and one `+0.1` press
        // lands on `0.799999988079071`; asymmetric counterparts keep genuine
        // `f64` precision intact.
        assert_eq!(summarize_value(&TomlValue::Float(f64::from(0.7f32))), "0.7");
        assert_eq!(summarize_value(&TomlValue::Float(f64::from(0.3f32))), "0.3");
        assert_eq!(summarize_value(&TomlValue::Float(f64::from(0.7f32) + 0.1)), "0.8");
        assert_eq!(summarize_value(&TomlValue::Float(0.30000000000000004)), "0.3");
        assert_eq!(summarize_value(&TomlValue::Float(0.3333333333333333)), "0.3333333333333333");
        assert_eq!(summarize_value(&TomlValue::Float(1.0)), "1");
    }

    #[test]
    fn display_title_preserves_quoted_map_key_for_table() {
        let value = TomlValue::Table(toml::map::Map::new());

        assert_eq!(
            display_title("provider.with.dot", r#"mcp.allowlist.providers["provider.with.dot"]"#, &value,),
            "provider.with.dot"
        );
        assert_eq!(display_title("[section]", r#"mcp.allowlist.providers["[section]"]"#, &value,), "[section]");
        assert!(!ends_with_quoted_map_key(r#"mcp.allowlist.providers["provider.with.dot"].settings"#));
    }
}
