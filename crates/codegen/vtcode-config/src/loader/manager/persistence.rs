//! Sparse configuration saves and comment-preserving TOML persistence.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result, bail};
use vtcode_commons::VtCodePaths;

use super::{ConfigLayerSource, ConfigManager, VTCodeConfig};

fn ensure_private_parent_dir(path: &Path) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let _ = VtCodePaths::ensure_user_dir(parent)
        .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    Ok(())
}

impl ConfigManager {
    /// Persist configuration to a specific path, preserving comments.
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        Self::save_config_to_path_internal(path, config, false)
    }

    /// Persist configuration to a repository-controlled path.
    ///
    /// Repository and project files may contain ordinary settings, but they
    /// must never receive trusted provider definitions or provider endpoint and
    /// credential overrides from the merged configuration. Existing protected
    /// keys are removed as well so this method can repair files written by an
    /// older version that flattened the effective configuration.
    pub fn save_repository_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        Self::save_config_to_path_internal(path, config, true)
    }

    fn save_config_to_path_internal(
        path: impl AsRef<Path>,
        config: &VTCodeConfig,
        repository_controlled: bool,
    ) -> Result<()> {
        let path = path.as_ref();
        ensure_private_parent_dir(path)?;
        let config_to_persist = if repository_controlled {
            Self::repository_safe_config(config)
        } else {
            config.clone()
        };
        let sparse_value =
            Self::sparse_config_value(&config_to_persist).context("Failed to prepare sparse configuration")?;
        let sparse_content =
            toml::to_string_pretty(&sparse_value).context("Failed to serialize sparse configuration")?;

        // If file exists, preserve comments by using toml_edit. Read and
        // publish through the shared no-follow/atomic file policy so a
        // user-controlled symlink cannot redirect configuration writes.
        let existing_content = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to read symlinked config file: {}", path.display())
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Config path is not a regular file: {}", path.display())
            }
            Ok(_) => Some(
                String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
                    .with_context(|| format!("Failed to read existing config: {}", path.display()))?,
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect config: {}", path.display()));
            }
        };

        if let Some(original_content) = existing_content {
            let mut doc = original_content
                .parse::<toml_edit::DocumentMut>()
                .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
            Self::remove_deprecated_config_keys(&mut doc);
            if repository_controlled {
                Self::remove_repository_provider_settings(&mut doc);
            }

            let new_doc: toml_edit::DocumentMut = sparse_content
                .parse()
                .context("Failed to parse sparse serialized configuration")?;
            let default_value =
                toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
            let default_doc: toml_edit::DocumentMut = toml::to_string_pretty(&default_value)
                .context("Failed to serialize default configuration")?
                .parse()
                .context("Failed to parse default serialized configuration")?;

            // Update values while preserving structure and comments
            Self::merge_sparse_toml_documents(&mut doc, &new_doc, &default_doc);
            // The sparse merge cannot express the deletion of optional fields
            // whose default serializes to absent (they appear in neither the
            // sparse update nor the defaults), so resets issued through save
            // paths (e.g. service_tier back to Project default) would leave
            // stale file values behind forever. Reconcile those explicitly.
            Self::prune_cleared_optional_fields(&mut doc, &config_to_persist);

            VtCodePaths::write_private_file_atomic(path, doc.to_string().as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        } else {
            VtCodePaths::write_private_file_atomic(path, sparse_content.as_bytes())
                .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        }

        Ok(())
    }

    fn repository_safe_config(config: &VTCodeConfig) -> VTCodeConfig {
        let mut safe_config = config.clone();
        safe_config.custom_providers.clear();
        for provider_override in safe_config.provider_overrides.values_mut() {
            provider_override.base_url = None;
            provider_override.api_key_env = None;
        }
        safe_config
    }

    /// Remove file values for optional fields the effective config reset to
    /// their absent default (e.g. `provider.openai.service_tier` back to
    /// Project default). The sparse merge cannot express these deletions
    /// (absent from both the sparse update and the defaults), so each entry
    /// pairs a dotted path with a typed cleared-check. Only listed paths are
    /// touched; foreign keys are never removed. Extend the list when a new
    /// resettable optional reaches a save path.
    fn prune_cleared_optional_fields(doc: &mut toml_edit::DocumentMut, config: &VTCodeConfig) {
        // Each entry: (dotted path, is_cleared).
        let cleared: &[(&[&str], bool)] = &[
            (&["provider", "openai", "service_tier"], config.provider.openai.service_tier.is_none()),
            (&["provider", "anthropic", "speed"], config.provider.anthropic.speed.is_none()),
        ];
        for (path, is_cleared) in cleared {
            if *is_cleared {
                Self::remove_path_and_prune_empty_parents(doc.as_table_mut(), path);
            }
        }
    }

    /// Remove `path` from `table`, then drop ancestor tables left empty.
    /// Returns true when `table` itself is now empty.
    fn remove_path_and_prune_empty_parents(table: &mut toml_edit::Table, path: &[&str]) -> bool {
        let Some((first, rest)) = path.split_first() else {
            return table.is_empty();
        };
        if rest.is_empty() {
            table.remove(first);
        } else if let Some(child) = table.get_mut(first).and_then(toml_edit::Item::as_table_mut) {
            if Self::remove_path_and_prune_empty_parents(child, rest) {
                table.remove(first);
            }
        }
        table.is_empty()
    }

    fn remove_deprecated_config_keys(doc: &mut toml_edit::DocumentMut) {
        let table = doc.as_table_mut();
        table.remove("project_doc_max_bytes");
        table.remove("project_doc_fallback_filenames");
        Self::remove_table_keys(table, "agent", &["autonomous_mode", "default_editing_mode"]);
        Self::remove_table_keys(table, "permissions", &["allowed_tools", "disallowed_tools", "auto_permission"]);
    }

    fn remove_table_keys(table: &mut toml_edit::Table, section: &str, keys: &[&str]) {
        let Some(section) = table.get_mut(section).and_then(toml_edit::Item::as_table_mut) else {
            return;
        };

        for key in keys {
            section.remove(key);
        }
    }

    pub fn sparse_config_value(config: &VTCodeConfig) -> Result<toml::Value> {
        let mut value = toml::Value::try_from(config).context("Failed to serialize configuration")?;
        let default_value =
            toml::Value::try_from(VTCodeConfig::default()).context("Failed to serialize default configuration")?;
        Self::prune_default_values(&mut value, &default_value);
        Ok(value)
    }

    fn prune_default_values(value: &mut toml::Value, default_value: &toml::Value) -> bool {
        match (value, default_value) {
            (toml::Value::Table(table), toml::Value::Table(default_table)) => {
                table.retain(|key, child| {
                    default_table
                        .get(key)
                        .is_none_or(|default_child| !Self::prune_default_values(child, default_child))
                });
                table.is_empty()
            }
            (value, default_value) => value == default_value,
        }
    }

    /// Merge TOML documents, preserving comments and structure from original
    fn merge_sparse_toml_documents(
        original: &mut toml_edit::DocumentMut,
        new: &toml_edit::DocumentMut,
        default_doc: &toml_edit::DocumentMut,
    ) {
        Self::merge_sparse_tables(original.as_table_mut(), new.as_table(), default_doc.as_table());
    }

    fn merge_sparse_tables(original: &mut toml_edit::Table, new: &toml_edit::Table, default_table: &toml_edit::Table) {
        let mut remove_keys = Vec::with_capacity(default_table.len());

        for (key, default_value) in default_table.iter() {
            if let Some(new_value) = new.get(key) {
                if let Some(original_value) = original.get_mut(key) {
                    Self::merge_sparse_items(original_value, new_value, default_value);
                } else {
                    original[key] = new_value.clone();
                }
            } else {
                let Some(original_value) = original.get_mut(key) else {
                    continue;
                };
                if Self::remove_known_default_item(original_value, default_value) {
                    remove_keys.push(key.to_string());
                }
            }
        }

        for key in remove_keys {
            original.remove(&key);
        }

        for (key, new_value) in new.iter() {
            if default_table.contains_key(key) {
                continue;
            }
            if let Some(original_value) = original.get_mut(key) {
                *original_value = new_value.clone();
            } else {
                original[key] = new_value.clone();
            }
        }
    }

    fn merge_sparse_items(original: &mut toml_edit::Item, new: &toml_edit::Item, default_value: &toml_edit::Item) {
        match (original, new, default_value) {
            (
                toml_edit::Item::Table(orig_table),
                toml_edit::Item::Table(new_table),
                toml_edit::Item::Table(default_table),
            ) => Self::merge_sparse_tables(orig_table, new_table, default_table),
            (orig, new, _) => {
                *orig = new.clone();
            }
        }
    }

    fn remove_known_default_item(original: &mut toml_edit::Item, default_value: &toml_edit::Item) -> bool {
        match (original, default_value) {
            (toml_edit::Item::Table(orig_table), toml_edit::Item::Table(default_table)) => {
                let mut remove_keys = Vec::new();
                for (key, default_child) in default_table.iter() {
                    let Some(orig_child) = orig_table.get_mut(key) else {
                        continue;
                    };
                    if Self::remove_known_default_item(orig_child, default_child) {
                        remove_keys.push(key.to_string());
                    }
                }
                for key in remove_keys {
                    orig_table.remove(&key);
                }
                orig_table.is_empty()
            }
            _ => true,
        }
    }

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        let (target, repository_controlled) = if let Some(path) = &self.config_path {
            if self.write_global_config_to_canonical || self.is_global_config_path(path) {
                (self.preferred_user_config_path().unwrap_or_else(|| path.clone()), false)
            } else {
                (path.clone(), !self.explicit_config_is_trusted)
            }
        } else if let Some(workspace_root) = &self.workspace_root {
            (workspace_root.join(&self.config_file_name), !self.explicit_config_is_trusted)
        } else {
            let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
            (cwd.join(&self.config_file_name), !self.explicit_config_is_trusted)
        };

        if repository_controlled {
            Self::save_repository_config_to_path(target, config)?;
        } else {
            Self::save_config_to_path(target, config)?;
        }

        #[cfg(not(test))]
        if let Some(workspace) = &self.workspace_root {
            Self::invalidate_workspace_cache(workspace);
        }

        self.sync_from_config(config)
    }

    fn is_global_config_path(&self, path: &Path) -> bool {
        self.layer_stack.layers().iter().any(|layer| {
            layer.is_enabled()
                && matches!(layer.source, ConfigLayerSource::System { .. } | ConfigLayerSource::User { .. })
                && match &layer.source {
                    ConfigLayerSource::System { file } | ConfigLayerSource::User { file } => file == path,
                    _ => false,
                }
        })
    }

    /// Sync internal config from a saved config
    /// Call this after save_config to keep internal state in sync
    fn sync_from_config(&mut self, config: &VTCodeConfig) -> Result<()> {
        self.config = config.clone();
        Ok(())
    }
}
