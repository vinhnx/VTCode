use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use toml::Value as TomlValue;
use vtcode_commons::VtCodePaths;
use vtcode_core::config::loader::{ConfigManager, merge_toml_value_preserving_comments, read_config_toml_for_edit};

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

pub(super) fn edit_toml_value(path: &Path, update: impl FnOnce(&mut toml::Table) -> Result<()>) -> Result<()> {
    let original = read_config_toml_for_edit(path)?;
    let mut updated: toml::Table = toml::from_str(original.as_deref().unwrap_or_default())
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    update(&mut updated)?;

    let root = TomlValue::Table(updated);
    let content = match original.as_deref() {
        Some(original) => merge_toml_value_preserving_comments(original, &root)
            .with_context(|| format!("Failed to patch {}", path.display()))?,
        None if root.as_table().is_some_and(toml::Table::is_empty) => return Ok(()),
        None => toml::to_string_pretty(&root)?,
    };

    if original.as_deref() == Some(&content) {
        return Ok(());
    }
    if content.trim().is_empty() {
        return fs::remove_file(path).with_context(|| format!("Failed to remove {}", path.display()));
    }
    VtCodePaths::write_private_file_atomic(path, content.as_bytes())
        .with_context(|| format!("Failed to write {}", path.display()))
}

pub(super) fn preferred_workspace_config_path(manager: &ConfigManager, workspace: &Path) -> PathBuf {
    manager.preferred_workspace_config_path(workspace)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn edit_toml_value_rejects_symlink_without_touching_target() {
        let temp = tempfile::tempdir().expect("temp directory");
        let destination = temp.path().join("destination.toml");
        let link = temp.path().join("vtcode.toml");
        fs::write(&destination, "flag = false\n").expect("write destination");
        std::os::unix::fs::symlink(&destination, &link).expect("make config symlink");

        let result = edit_toml_value(&link, |table| {
            table.insert("flag".to_string(), TomlValue::Boolean(true));
            Ok(())
        });
        assert!(result.unwrap_err().to_string().contains("not a regular file"));
        assert_eq!(fs::read_to_string(destination).expect("read destination"), "flag = false\n");
    }
}
