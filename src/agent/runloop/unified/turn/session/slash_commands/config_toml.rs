use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml::Value as TomlValue;
use vtcode_core::config::loader::ConfigManager;
use vtcode_core::config::loader::layers::ConfigLayerSource;

pub(super) fn ensure_child_table<'a>(
    table: &'a mut toml::map::Map<String, TomlValue>,
    key: &str,
) -> &'a mut toml::map::Map<String, TomlValue> {
    let entry = table
        .entry(key.to_string())
        .or_insert_with(|| TomlValue::Table(Default::default()));
    if !entry.is_table() {
        *entry = TomlValue::Table(Default::default());
    }
    entry
        .as_table_mut()
        .expect("table entry should be a table after initialization")
}

pub(super) fn load_toml_value(path: &Path) -> Result<TomlValue> {
    if !path.exists() {
        return Ok(TomlValue::Table(Default::default()));
    }

    let content =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(TomlValue::Table(Default::default()));
    }

    toml::from_str::<TomlValue>(&content)
        .with_context(|| format!("Failed to parse {}", path.display()))
}

pub(super) fn save_toml_value(path: &Path, root: &TomlValue) -> Result<()> {
    let is_empty = root.as_table().is_some_and(|table| table.is_empty());
    if is_empty {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("Failed to remove {}", path.display()))?;
        }
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    fs::write(path, toml::to_string_pretty(root)?)
        .with_context(|| format!("Failed to write {}", path.display()))
}

pub(super) fn preferred_workspace_config_path(
    manager: &ConfigManager,
    workspace: &Path,
) -> PathBuf {
    manager
        .layer_stack()
        .layers()
        .iter()
        .rev()
        .find_map(|layer| match &layer.source {
            ConfigLayerSource::Workspace { file } if layer.is_enabled() => Some(file.clone()),
            _ => None,
        })
        .unwrap_or_else(|| workspace.join(manager.config_file_name()))
}
