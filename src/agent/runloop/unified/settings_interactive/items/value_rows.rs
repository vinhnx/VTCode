use super::*;

pub(super) fn item_for_value(label: &str, path: &str, value: &TomlValue, draft_root: &TomlValue) -> InlineListItem {
    use vtcode_commons::ui_protocol::InlineTone;

    let doc = FIELD_DOCS.lookup(path);
    let title = display_title(label, path, value);
    let description = doc.and_then(|entry| (!entry.description.is_empty()).then(|| entry.description.clone()));
    let summary = summarize_value(value);
    let search_value = search_value_with_content(path, label, value, doc);

    if path == ConfigKey::DEFAULT_MODEL {
        return setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(ACTION_PICK_MAIN_MODEL.to_string())),
            search_value,
        );
    }

    if path == ConfigKey::EDITOR {
        return setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(ACTION_CONFIGURE_EDITOR.to_string())),
            search_value,
        );
    }

    match value {
        TomlValue::Boolean(enabled) => setting_row(
            title,
            if *enabled { "On".to_string() } else { "Off".to_string() },
            description,
            if *enabled {
                InlineTone::Success
            } else {
                InlineTone::Neutral
            },
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_SET}{path}:toggle"))),
            search_value,
        ),
        TomlValue::Integer(_) | TomlValue::Float(_) => setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_SET}{path}:inc"))),
            search_value,
        ),
        TomlValue::String(current) => {
            let has_options = resolve_cycle_options(Some(draft_root), path, current).len() > 1;
            let action = if has_options {
                format!("{ACTION_PREFIX_SET}{path}:cycle")
            } else {
                format!("{ACTION_PREFIX_EDIT}{path}")
            };
            setting_row(
                title,
                summary,
                description,
                InlineTone::Accent,
                Some(InlineListSelection::ConfigAction(action)),
                search_value,
            )
        }
        TomlValue::Array(entries) => setting_row(
            title,
            format!("{} item{}", entries.len(), if entries.len() == 1 { "" } else { "s" }),
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_OPEN}{path}"))),
            search_value,
        ),
        TomlValue::Table(_) => setting_row(
            title,
            summary,
            description,
            InlineTone::Accent,
            Some(InlineListSelection::ConfigAction(format!("{ACTION_PREFIX_OPEN}{path}"))),
            search_value,
        ),
        _ => setting_row(title, summary, description, InlineTone::Neutral, None, search_value),
    }
}

pub(super) fn item_for_missing_doc_value(label: &str, path: &str) -> InlineListItem {
    let doc = FIELD_DOCS.lookup(path);
    let description = doc.and_then(|entry| (!entry.description.is_empty()).then(|| entry.description.clone()));
    let has_options = doc.map(|entry| !entry.options.is_empty()).unwrap_or(false);
    let action = if has_options {
        format!("{ACTION_PREFIX_SET}{path}:cycle")
    } else {
        format!("{ACTION_PREFIX_EDIT}{path}")
    };

    setting_row(
        humanize_identifier(label),
        "<unset>".to_string(),
        description,
        vtcode_commons::ui_protocol::InlineTone::Neutral,
        Some(InlineListSelection::ConfigAction(action)),
        search_value_for_missing_doc(path, label, doc),
    )
}

pub(super) fn missing_doc_label<'a>(path: &'a str, parent_path: Option<&str>) -> Option<&'a str> {
    match parent_path {
        Some(parent) => path
            .strip_prefix(parent)
            .and_then(|suffix| suffix.strip_prefix('.'))
            .filter(|suffix| !suffix.contains('.') && !suffix.contains('[')),
        None => Some(path),
    }
}
