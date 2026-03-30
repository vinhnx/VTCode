use anyhow::{Result, anyhow};
use toml::Value as TomlValue;
use vtcode_tui::app::{InlineListItem, InlineListSelection};

use super::docs::FIELD_DOCS;
use super::mutations::resolve_cycle_options;
use super::path::get_node;
use super::render::{
    action_item, collection_subtitle, display_title, search_value_for_missing_doc,
    search_value_with_content, section_item, section_subtitle, setting_subtitle, summarize_value,
};
use super::{
    ACTION_CONFIGURE_EDITOR, ACTION_PICK_LIGHTWEIGHT_MODEL, ACTION_PICK_MAIN_MODEL,
    ACTION_PREFIX_ARRAY_ADD, ACTION_PREFIX_ARRAY_POP, ACTION_PREFIX_OPEN, ACTION_PREFIX_SET,
    OPTIONAL_DOC_FIELDS, SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH, SETTINGS_MODEL_CONFIG_MAIN_PATH,
    SETTINGS_MODEL_CONFIG_PATH, SettingsPaletteState,
};
use crate::agent::runloop::unified::config_section_headings::humanize_identifier;

const HIDDEN_SETTINGS_PATHS: &[&str] = &["agent.autonomous_mode"];

pub(super) fn build_settings_items(
    state: &SettingsPaletteState,
    draft: &TomlValue,
) -> Result<Vec<InlineListItem>> {
    let mut items = Vec::new();

    items.push(section_item("Actions"));
    items.push(action_item(
        "Reload from disk",
        "Reload effective values from current configuration files",
        Some("Action"),
        super::ACTION_RELOAD,
    ));

    if let Some(view_path) = state.view_path.as_deref() {
        items.push(action_item(
            "Back to sections",
            "Return to the top-level settings sections",
            Some("Nav"),
            super::ACTION_OPEN_ROOT,
        ));

        if append_synthetic_model_config_items(&mut items, view_path, draft)? {
            return Ok(items);
        }

        let node = get_node(draft, view_path)
            .ok_or_else(|| anyhow!("Could not resolve settings path {}", view_path))?;
        append_node_items(&mut items, view_path, node, draft)?;
    } else if let TomlValue::Table(table) = draft {
        items.push(section_item("Quick Access"));
        items.push(action_item(
            "Model Config",
            "Edit main and lightweight model settings in one focused tree",
            Some("Section"),
            &format!("{}{}", ACTION_PREFIX_OPEN, SETTINGS_MODEL_CONFIG_PATH),
        ));
        items.push(action_item(
            "External Editor",
            "Open guided setup for /edit, Ctrl+E, and Cmd/Ctrl-click file links",
            Some("Setup"),
            ACTION_CONFIGURE_EDITOR,
        ));
        append_table_items(&mut items, table, None, None, draft);
    }

    Ok(items)
}

fn append_node_items(
    items: &mut Vec<InlineListItem>,
    path: &str,
    node: &TomlValue,
    draft_root: &TomlValue,
) -> Result<()> {
    match node {
        TomlValue::Table(table) => {
            append_table_items(items, table, Some(path), Some(node), draft_root);
        }
        TomlValue::Array(entries) => {
            items.push(action_item(
                "Add item",
                "Append a new default item to this array",
                Some("Array"),
                &format!("{}{}", ACTION_PREFIX_ARRAY_ADD, path),
            ));
            items.push(action_item(
                "Remove last item",
                "Remove the final array entry",
                Some("Array"),
                &format!("{}{}", ACTION_PREFIX_ARRAY_POP, path),
            ));

            for (index, value) in entries.iter().enumerate() {
                let child_path = format!("{}[{}]", path, index);
                let label = format!("[{}]", index);
                items.push(item_for_value(&label, &child_path, value, draft_root));
            }
        }
        _ => {
            items.push(item_for_value(path, path, node, draft_root));
        }
    }

    Ok(())
}

fn append_synthetic_model_config_items(
    items: &mut Vec<InlineListItem>,
    view_path: &str,
    draft_root: &TomlValue,
) -> Result<bool> {
    match view_path {
        SETTINGS_MODEL_CONFIG_PATH => {
            items.push(section_item("Sections"));
            items.push(action_item(
                "Main Model",
                "Provider and default model for the active conversation model",
                Some("Section"),
                &format!("{}{}", ACTION_PREFIX_OPEN, SETTINGS_MODEL_CONFIG_MAIN_PATH),
            ));
            items.push(action_item(
                "Lightweight Model",
                "Shared lower-cost route for memory, prompt suggestions, and smaller delegated tasks",
                Some("Section"),
                &format!(
                    "{}{}",
                    ACTION_PREFIX_OPEN, SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH
                ),
            ));
            Ok(true)
        }
        SETTINGS_MODEL_CONFIG_MAIN_PATH => {
            items.push(section_item("Settings"));
            append_mapped_setting_item(items, draft_root, "agent.provider");
            append_mapped_setting_item(items, draft_root, "agent.default_model");
            Ok(true)
        }
        SETTINGS_MODEL_CONFIG_LIGHTWEIGHT_PATH => {
            let node = get_node(draft_root, "agent.small_model")
                .ok_or_else(|| anyhow!("Could not resolve settings path agent.small_model"))?;
            append_node_items(items, "agent.small_model", node, draft_root)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn append_mapped_setting_item(items: &mut Vec<InlineListItem>, draft_root: &TomlValue, path: &str) {
    let Some(value) = get_node(draft_root, path) else {
        return;
    };
    let label = path.rsplit('.').next().unwrap_or(path);
    items.push(item_for_value(label, path, value, draft_root));
}

fn append_table_items(
    items: &mut Vec<InlineListItem>,
    table: &toml::map::Map<String, TomlValue>,
    parent_path: Option<&str>,
    optional_doc_root: Option<&TomlValue>,
    draft_root: &TomlValue,
) {
    let mut section_items = Vec::new();
    let mut setting_items = Vec::new();

    for key in super::render::sorted_table_keys(table) {
        let Some(value) = table.get(key) else {
            continue;
        };
        let path = parent_path
            .map(|parent| format!("{parent}.{key}"))
            .unwrap_or_else(|| key.clone());
        if HIDDEN_SETTINGS_PATHS.contains(&path.as_str()) {
            continue;
        }
        let entry = item_for_value(key, &path, value, draft_root);
        if matches!(value, TomlValue::Table(_)) {
            section_items.push(entry);
        } else {
            setting_items.push(entry);
        }
    }

    if let (Some(root), Some(path)) = (optional_doc_root, parent_path) {
        append_missing_optional_doc_items(&mut setting_items, root, Some(path));
    }

    if !section_items.is_empty() {
        items.push(section_item("Sections"));
        items.extend(section_items);
    }
    if !setting_items.is_empty() {
        items.push(section_item("Settings"));
        items.extend(setting_items);
    }
}

fn append_missing_optional_doc_items(
    items: &mut Vec<InlineListItem>,
    root: &TomlValue,
    parent_path: Option<&str>,
) {
    for path in OPTIONAL_DOC_FIELDS {
        let lookup_path = parent_path
            .and_then(|parent| {
                path.strip_prefix(parent)
                    .and_then(|suffix| suffix.strip_prefix('.'))
            })
            .unwrap_or(path);
        if get_node(root, lookup_path).is_some() {
            continue;
        }

        let Some(label) = missing_doc_label(path, parent_path) else {
            continue;
        };
        items.push(item_for_missing_doc_value(label, path));
    }
}

fn item_for_value(
    label: &str,
    path: &str,
    value: &TomlValue,
    draft_root: &TomlValue,
) -> InlineListItem {
    let doc = FIELD_DOCS.lookup(path);
    let title = display_title(label, path, value);
    let description = doc
        .and_then(|entry| {
            if entry.description.is_empty() {
                None
            } else {
                Some(entry.description.clone())
            }
        })
        .unwrap_or_default();

    let summary = if path == "agent.small_model.model"
        && value.as_str().is_some_and(|current| current.is_empty())
    {
        "Automatic".to_string()
    } else {
        summarize_value(value)
    };
    let subtitle = setting_subtitle(&summary, &description, false);
    let search_value = search_value_with_content(path, label, value, doc);

    if path == "agent.default_model" || path == "agent.small_model.model" {
        let action = if path == "agent.default_model" {
            ACTION_PICK_MAIN_MODEL
        } else {
            ACTION_PICK_LIGHTWEIGHT_MODEL
        };
        return InlineListItem {
            title,
            subtitle: Some(subtitle),
            badge: Some("Pick".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(action.to_string())),
            search_value: Some(search_value),
        };
    }

    if path == "tools.editor" {
        return InlineListItem {
            title,
            subtitle: Some(section_subtitle(path, value)),
            badge: Some("Setup".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(
                ACTION_CONFIGURE_EDITOR.to_string(),
            )),
            search_value: Some(search_value),
        };
    }

    match value {
        TomlValue::Boolean(_) => InlineListItem {
            title,
            subtitle: Some(subtitle),
            badge: Some("On/Off".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}:toggle",
                ACTION_PREFIX_SET, path
            ))),
            search_value: Some(search_value),
        },
        TomlValue::Integer(_) | TomlValue::Float(_) => InlineListItem {
            title,
            subtitle: Some(setting_subtitle(&summary, &description, true)),
            badge: Some("Step".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}:inc",
                ACTION_PREFIX_SET, path
            ))),
            search_value: Some(search_value),
        },
        TomlValue::String(current) => {
            let has_options = resolve_cycle_options(Some(draft_root), path, current).len() > 1;
            InlineListItem {
                title,
                subtitle: Some(setting_subtitle(&summary, &description, has_options)),
                badge: has_options.then(|| "Pick".to_string()),
                indent: 0,
                selection: has_options.then(|| {
                    InlineListSelection::ConfigAction(format!(
                        "{}{}:cycle",
                        ACTION_PREFIX_SET, path
                    ))
                }),
                search_value: Some(search_value),
            }
        }
        TomlValue::Array(entries) => InlineListItem {
            title,
            subtitle: Some(collection_subtitle(
                format!(
                    "{} item{}",
                    entries.len(),
                    if entries.len() == 1 { "" } else { "s" }
                ),
                &description,
            )),
            badge: Some("List".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                ACTION_PREFIX_OPEN, path
            ))),
            search_value: Some(search_value),
        },
        TomlValue::Table(_) => InlineListItem {
            title,
            subtitle: Some(section_subtitle(path, value)),
            badge: Some("Section".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::ConfigAction(format!(
                "{}{}",
                ACTION_PREFIX_OPEN, path
            ))),
            search_value: Some(search_value),
        },
        _ => InlineListItem {
            title,
            subtitle: Some(setting_subtitle(&summary, &description, false)),
            badge: None,
            indent: 0,
            selection: None,
            search_value: Some(search_value),
        },
    }
}

fn item_for_missing_doc_value(label: &str, path: &str) -> InlineListItem {
    let doc = FIELD_DOCS.lookup(path);
    let description = doc
        .and_then(|entry| (!entry.description.is_empty()).then(|| entry.description.clone()))
        .unwrap_or_default();
    let has_options = doc.map(|entry| !entry.options.is_empty()).unwrap_or(false);

    InlineListItem {
        title: humanize_identifier(label),
        subtitle: Some(setting_subtitle("<unset>", &description, has_options)),
        badge: Some(if has_options {
            "Pick".to_string()
        } else {
            "Unset".to_string()
        }),
        indent: 0,
        selection: has_options.then(|| {
            InlineListSelection::ConfigAction(format!("{}{}:cycle", ACTION_PREFIX_SET, path))
        }),
        search_value: Some(search_value_for_missing_doc(path, label, doc)),
    }
}

fn missing_doc_label<'a>(path: &'a str, parent_path: Option<&str>) -> Option<&'a str> {
    match parent_path {
        Some(parent) => path
            .strip_prefix(parent)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .filter(|suffix| !suffix.contains('.') && !suffix.contains('[')),
        None => Some(path),
    }
}
