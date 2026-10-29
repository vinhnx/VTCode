use super::*;

impl ConfigManager {
    pub(super) fn repair_repository_config_file(path: &Path) -> Result<bool> {
        let metadata = match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!("Refusing to repair symlinked config file: {}", path.display())
            }
            Ok(metadata) if !metadata.is_file() => {
                bail!("Config path is not a regular file: {}", path.display())
            }
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => {
                return Err(error).with_context(|| format!("Failed to inspect config: {}", path.display()));
            }
        };
        debug_assert!(metadata.is_file());

        let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
            .with_context(|| format!("Failed to read existing config: {}", path.display()))?;
        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .with_context(|| format!("Failed to parse existing config: {}", path.display()))?;
        if !Self::remove_repository_provider_settings(&mut doc) {
            return Ok(false);
        }

        VtCodePaths::write_private_file_atomic(path, doc.to_string().as_bytes())
            .with_context(|| format!("Failed to repair config file: {}", path.display()))?;
        Ok(true)
    }

    pub(super) fn remove_repository_provider_settings(doc: &mut toml_edit::DocumentMut) -> bool {
        let table = doc.as_table_mut();
        let mut changed = table.remove("custom_providers").is_some();

        let remove_provider_overrides = {
            let Some(overrides_item) = table.get_mut("provider_overrides") else {
                return changed;
            };

            if let Some(overrides) = overrides_item.as_table_mut() {
                changed |= Self::remove_provider_override_settings_from_table(overrides);
                overrides.is_empty()
            } else if let Some(overrides) = overrides_item.as_inline_table_mut() {
                changed |= Self::remove_provider_override_settings_from_inline_table(overrides);
                overrides.is_empty()
            } else {
                false
            }
        };

        if remove_provider_overrides {
            changed |= table.remove("provider_overrides").is_some();
        }
        changed
    }

    fn remove_provider_override_settings_from_table(overrides: &mut toml_edit::Table) -> bool {
        let provider_names = overrides.iter().map(|(name, _)| name.to_string()).collect::<Vec<_>>();
        let mut empty_providers = Vec::new();
        let mut changed = false;

        for provider_name in provider_names {
            let Some(provider_item) = overrides.get_mut(&provider_name) else {
                continue;
            };

            let is_empty = if let Some(provider) = provider_item.as_table_mut() {
                changed |= provider.remove("base_url").is_some();
                changed |= provider.remove("api_key_env").is_some();
                provider.is_empty()
            } else if let Some(provider) = provider_item.as_inline_table_mut() {
                changed |= provider.remove("base_url").is_some();
                changed |= provider.remove("api_key_env").is_some();
                provider.is_empty()
            } else {
                false
            };

            if is_empty {
                empty_providers.push(provider_name);
            }
        }

        for provider_name in empty_providers {
            changed |= overrides.remove(&provider_name).is_some();
        }
        changed
    }

    fn remove_provider_override_settings_from_inline_table(overrides: &mut toml_edit::InlineTable) -> bool {
        let provider_names = overrides.iter().map(|(name, _)| name.to_string()).collect::<Vec<_>>();
        let mut empty_providers = Vec::new();
        let mut changed = false;

        for provider_name in provider_names {
            let Some(provider) = overrides
                .get_mut(&provider_name)
                .and_then(toml_edit::Value::as_inline_table_mut)
            else {
                continue;
            };

            changed |= provider.remove("base_url").is_some();
            changed |= provider.remove("api_key_env").is_some();
            if provider.is_empty() {
                empty_providers.push(provider_name);
            }
        }

        for provider_name in empty_providers {
            changed |= overrides.remove(&provider_name).is_some();
        }
        changed
    }

    pub(super) fn project_config_path(
        config_dir: &Path,
        workspace_root: &Path,
        config_file_name: &str,
    ) -> Option<PathBuf> {
        let project_name = Self::identify_current_project(workspace_root)?;
        Some(
            config_dir
                .join("projects")
                .join(project_name)
                .join("config")
                .join(config_file_name),
        )
    }

    pub(super) fn identify_current_project(workspace_root: &Path) -> Option<String> {
        let project_file = workspace_root.join(".vtcode-project");
        if let Ok(contents) = fs::read_to_string(&project_file) {
            let name = contents.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }

        workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.to_string())
    }
}
