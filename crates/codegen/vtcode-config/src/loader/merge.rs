use crate::loader::layers::ConfigLayerMetadata;
use hashbrown::HashMap;

/// Recursively merge two TOML values.
///
/// If both values are tables, they are merged recursively.
/// Otherwise, the `overlay` value replaces the `base` value.
pub fn merge_toml_values(base: &mut toml::Value, overlay: &toml::Value) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                if let Some(base_value) = base_table.get_mut(key) {
                    merge_toml_values(base_value, value);
                } else {
                    base_table.insert(key.clone(), value.clone());
                }
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
        }
    }
}

/// Recursively merge two TOML values and record which layer last wrote each path.
pub fn merge_toml_values_with_origins(
    base: &mut toml::Value,
    overlay: &toml::Value,
    origins: &mut HashMap<String, ConfigLayerMetadata>,
    layer: &ConfigLayerMetadata,
) {
    merge_with_origins(base, overlay, "", origins, layer);
}

fn merge_with_origins(
    base: &mut toml::Value,
    overlay: &toml::Value,
    path: &str,
    origins: &mut HashMap<String, ConfigLayerMetadata>,
    layer: &ConfigLayerMetadata,
) {
    match (base, overlay) {
        (toml::Value::Table(base_table), toml::Value::Table(overlay_table)) => {
            for (key, value) in overlay_table {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };

                if let Some(base_value) = base_table.get_mut(key) {
                    if base_value.is_table() && value.is_table() {
                        merge_with_origins(base_value, value, &child_path, origins, layer);
                    } else {
                        *base_value = value.clone();
                        assign_origins(value, &child_path, origins, layer);
                    }
                } else {
                    base_table.insert(key.clone(), value.clone());
                    assign_origins(value, &child_path, origins, layer);
                }
            }
        }
        (base, overlay) => {
            *base = overlay.clone();
            if !path.is_empty() {
                assign_origins(overlay, path, origins, layer);
            }
        }
    }
}

fn assign_origins(
    value: &toml::Value,
    path: &str,
    origins: &mut HashMap<String, ConfigLayerMetadata>,
    layer: &ConfigLayerMetadata,
) {
    match value {
        toml::Value::Table(table) => {
            for (key, child) in table {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                assign_origins(child, &child_path, origins, layer);
            }
        }
        _ => {
            origins.insert(path.to_string(), layer.clone());
        }
    }
}

/// Promote legacy top-level `default_provider` / `default_model` keys into the
/// canonical `[agent]` table within a single layer value.
///
/// Older global files (and hand-edited configs) use bare
/// `default_provider = "ollama"` / `default_model = "..."` at the document
/// root. `VTCodeConfig` only deserializes `agent.provider` /
/// `agent.default_model`, so without this promotion those keys are silently
/// dropped and the runtime falls back to the compiled-in default.
/// The promotion is per-layer so normal layer precedence still applies: an
/// explicit `[agent] provider` in the same file wins over its own top-level
/// alias, and higher-precedence layers win over lower ones.
pub(crate) fn normalize_legacy_top_level_provider_aliases(value: &mut toml::Value) {
    let Some(table) = value.as_table_mut() else {
        return;
    };

    let legacy_provider = table
        .get("default_provider")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);
    let legacy_model = table
        .get("default_model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    if legacy_provider.is_none() && legacy_model.is_none() {
        return;
    }

    let agent_entry = table
        .entry("agent".to_string())
        .or_insert(toml::Value::Table(toml::Table::new()));
    let Some(agent_table) = agent_entry.as_table_mut() else {
        return;
    };

    if let Some(provider) = legacy_provider
        && !agent_table.contains_key("provider")
    {
        agent_table.insert("provider".to_string(), toml::Value::String(provider));
    }
    if let Some(model) = legacy_model
        && !agent_table.contains_key("default_model")
    {
        agent_table.insert("default_model".to_string(), toml::Value::String(model));
    }
}
