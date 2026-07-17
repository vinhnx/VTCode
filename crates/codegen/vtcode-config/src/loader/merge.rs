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
