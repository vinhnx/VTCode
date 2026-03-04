use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use toml::Value as TomlValue;
use vtcode_config::defaults;
use vtcode_config::loader::layers::{ConfigLayerMetadata, ConfigLayerSource};
use vtcode_config::loader::{
    ConfigBuilder, ConfigManager, VTCodeConfig, fingerprint_str, merge_toml_values,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigReadRequest {
    pub workspace: PathBuf,
    #[serde(default)]
    pub runtime_overrides: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigLayerView {
    pub source: ConfigLayerSource,
    pub metadata: ConfigLayerMetadata,
    pub disabled_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigReadResponse {
    pub effective_config: serde_json::Value,
    pub merged_version: String,
    pub layers: Vec<ConfigLayerView>,
    pub origins: BTreeMap<String, ConfigLayerMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigWriteTarget {
    User,
    Workspace,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConfigWriteStrategy {
    Replace,
    Upsert,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWriteRequest {
    pub workspace: PathBuf,
    pub target: ConfigWriteTarget,
    pub path: String,
    pub value: TomlValue,
    pub strategy: ConfigWriteStrategy,
    #[serde(default)]
    pub expected_layer_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideMetadata {
    pub source: ConfigLayerSource,
    pub metadata: ConfigLayerMetadata,
    pub effective_value: TomlValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigWriteResponse {
    pub merged_version: String,
    pub written_layer_version: String,
    pub effective_value: Option<TomlValue>,
    pub overridden_metadata: Option<OverrideMetadata>,
}

pub struct ConfigService;

impl ConfigService {
    pub fn read(request: ConfigReadRequest) -> Result<ConfigReadResponse> {
        let mut builder = ConfigBuilder::new().workspace(request.workspace.clone());
        if !request.runtime_overrides.is_empty() {
            builder = builder.cli_overrides(&request.runtime_overrides);
        }
        let manager = builder.build().context("Failed to build configuration")?;
        let (effective_toml, origins) = manager.layer_stack().effective_config_with_origins();
        let effective_config = serde_json::to_value(&effective_toml)
            .context("Failed to serialize effective configuration to JSON")?;
        let merged_version = merged_version(manager.layer_stack().layers());

        let layers = manager
            .layer_stack()
            .layers()
            .iter()
            .map(|layer| ConfigLayerView {
                source: layer.source.clone(),
                metadata: layer.metadata.clone(),
                disabled_reason: layer
                    .disabled_reason
                    .as_ref()
                    .map(|reason| format!("{reason:?}")),
                error: layer.error.as_ref().map(|error| error.message.clone()),
            })
            .collect();

        let origins = origins.into_iter().collect::<BTreeMap<_, _>>();
        Ok(ConfigReadResponse {
            effective_config,
            merged_version,
            layers,
            origins,
        })
    }

    pub fn write(request: ConfigWriteRequest) -> Result<ConfigWriteResponse> {
        if request.path.trim().is_empty() {
            bail!("Config path cannot be empty");
        }

        let manager =
            ConfigManager::load_from_workspace(&request.workspace).with_context(|| {
                format!(
                    "Failed to load workspace config from {}",
                    request.workspace.display()
                )
            })?;

        let target_path = resolve_target_path(&manager, &request.workspace, &request.target)?;

        let current_version = manager
            .layer_stack()
            .layers()
            .iter()
            .find(|layer| source_matches_target(&layer.source, &request.target, &target_path))
            .map(|layer| layer.metadata.version.clone());

        if let Some(expected) = request.expected_layer_version.as_ref()
            && current_version.as_ref() != Some(expected)
        {
            bail!(
                "Layer version mismatch for {} (expected {}, got {})",
                target_path.display(),
                expected,
                current_version.unwrap_or_else(|| "<missing>".to_string())
            );
        }

        let mut target_toml = load_or_default_toml(&target_path)?;
        apply_write(
            &mut target_toml,
            &request.path,
            &request.value,
            request.strategy.clone(),
        )?;

        let updated_config: VTCodeConfig = target_toml.clone().try_into().with_context(|| {
            format!(
                "Updated configuration at {} could not be deserialized",
                target_path.display()
            )
        })?;
        updated_config
            .validate()
            .context("Updated configuration failed validation")?;

        ConfigManager::save_config_to_path(&target_path, &updated_config).with_context(|| {
            format!(
                "Failed to write updated configuration to {}",
                target_path.display()
            )
        })?;

        let reloaded_manager = ConfigManager::load_from_workspace(&request.workspace)
            .context("Failed to reload configuration after write")?;
        let (effective_toml, origins) = reloaded_manager
            .layer_stack()
            .effective_config_with_origins();

        let written_layer = reloaded_manager
            .layer_stack()
            .layers()
            .iter()
            .find(|layer| source_matches_target(&layer.source, &request.target, &target_path))
            .with_context(|| {
                format!(
                    "Unable to find written layer {} in reloaded stack",
                    target_path.display()
                )
            })?;

        let effective_value = get_value_by_path(&effective_toml, &request.path).cloned();
        let overridden_metadata = if let Some(origin) = origins.get(&request.path) {
            if origin.version != written_layer.metadata.version {
                let source = reloaded_manager
                    .layer_stack()
                    .layers()
                    .iter()
                    .find(|layer| layer.metadata.name == origin.name)
                    .map(|layer| layer.source.clone())
                    .unwrap_or(ConfigLayerSource::Runtime);

                effective_value.clone().map(|value| OverrideMetadata {
                    source,
                    metadata: origin.clone(),
                    effective_value: value,
                })
            } else {
                None
            }
        } else {
            None
        };

        Ok(ConfigWriteResponse {
            merged_version: merged_version(reloaded_manager.layer_stack().layers()),
            written_layer_version: written_layer.metadata.version.clone(),
            effective_value,
            overridden_metadata,
        })
    }
}

fn merged_version(layers: &[vtcode_config::loader::layers::ConfigLayerEntry]) -> String {
    let mut parts = Vec::with_capacity(layers.len());
    for layer in layers {
        if !layer.is_enabled() {
            continue;
        }
        parts.push(format!(
            "{}:{}",
            layer.metadata.name, layer.metadata.version
        ));
    }
    fingerprint_str(&parts.join("|"))
}

fn resolve_target_path(
    manager: &ConfigManager,
    workspace: &Path,
    target: &ConfigWriteTarget,
) -> Result<PathBuf> {
    match target {
        ConfigWriteTarget::Workspace => {
            let root = manager.workspace_root().unwrap_or(workspace).to_path_buf();
            Ok(root.join(manager.config_file_name()))
        }
        ConfigWriteTarget::User => {
            let provider = defaults::current_config_defaults();
            let paths = provider.home_config_paths(manager.config_file_name());
            if let Some(path) = paths.first() {
                return Ok(path.clone());
            }
            let home = dirs::home_dir().context("Could not resolve home directory")?;
            Ok(home.join(".vtcode").join(manager.config_file_name()))
        }
        ConfigWriteTarget::Project => {
            let provider = defaults::current_config_defaults();
            let workspace_root = manager.workspace_root().unwrap_or(workspace);
            let workspace_paths = provider.workspace_paths_for(workspace_root);
            let config_dir = workspace_paths.config_dir();
            let project_name = ConfigManager::current_project_name(workspace_root)
                .context("Could not resolve project name for project-level config")?;
            Ok(config_dir
                .join("projects")
                .join(project_name)
                .join("config")
                .join(manager.config_file_name()))
        }
    }
}

fn source_matches_target(
    source: &ConfigLayerSource,
    target: &ConfigWriteTarget,
    path: &Path,
) -> bool {
    match (source, target) {
        (ConfigLayerSource::User { file }, ConfigWriteTarget::User) => file == path,
        (ConfigLayerSource::Workspace { file }, ConfigWriteTarget::Workspace) => file == path,
        (ConfigLayerSource::Project { file }, ConfigWriteTarget::Project) => file == path,
        _ => false,
    }
}

fn load_or_default_toml(path: &Path) -> Result<TomlValue> {
    if !path.exists() {
        return Ok(TomlValue::Table(toml::Table::new()));
    }

    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config file {}", path.display()))
}

fn apply_write(
    root: &mut TomlValue,
    path: &str,
    value: &TomlValue,
    strategy: ConfigWriteStrategy,
) -> Result<()> {
    let existing = get_or_create_path_mut(root, path)?;
    match strategy {
        ConfigWriteStrategy::Replace => {
            *existing = value.clone();
        }
        ConfigWriteStrategy::Upsert => {
            if existing.is_table() && value.is_table() {
                merge_toml_values(existing, value);
            } else {
                *existing = value.clone();
            }
        }
    }
    Ok(())
}

fn get_or_create_path_mut<'a>(root: &'a mut TomlValue, path: &str) -> Result<&'a mut TomlValue> {
    let mut current = root;
    let parts: Vec<&str> = path.split('.').filter(|part| !part.is_empty()).collect();
    if parts.is_empty() {
        bail!("Invalid empty config path");
    }

    for (index, part) in parts.iter().enumerate() {
        let is_last = index == parts.len() - 1;
        let table = current
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("Path '{}' traverses non-table value", path))?;

        if is_last {
            let entry = table
                .entry((*part).to_string())
                .or_insert_with(|| TomlValue::Table(toml::Table::new()));
            return Ok(entry);
        }

        current = table
            .entry((*part).to_string())
            .or_insert_with(|| TomlValue::Table(toml::Table::new()));
    }

    bail!("Could not resolve config path '{}'", path)
}

fn get_value_by_path<'a>(root: &'a TomlValue, path: &str) -> Option<&'a TomlValue> {
    let mut current = root;
    for part in path.split('.').filter(|part| !part.is_empty()) {
        let table = current.as_table()?;
        current = table.get(part)?;
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use serial_test::serial;
    use vtcode_commons::reference::StaticWorkspacePaths;
    use vtcode_config::defaults::WorkspacePathsDefaults;
    use vtcode_config::defaults::provider::with_config_defaults_provider_for_test;

    #[test]
    #[serial]
    fn read_returns_layers_and_origins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path();
        let home_config = workspace.join("home").join("vtcode.toml");
        let workspace_config = workspace.join("vtcode.toml");
        fs::create_dir_all(home_config.parent().expect("home parent")).expect("home dir");

        fs::write(&home_config, "agent.provider = \"openai\"\n").expect("home config");
        fs::write(
            &workspace_config,
            "agent.provider = \"anthropic\"\nagent.default_model = \"claude-sonnet-4-5\"\n",
        )
        .expect("workspace config");

        let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
        let provider =
            WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![home_config]);

        with_config_defaults_provider_for_test(Arc::new(provider), || {
            let response = ConfigService::read(ConfigReadRequest {
                workspace: workspace.to_path_buf(),
                runtime_overrides: Vec::new(),
            })
            .expect("read response");

            assert!(!response.layers.is_empty());
            assert!(!response.merged_version.is_empty());
            assert!(response.origins.contains_key("agent.provider"));
        });
    }

    #[test]
    #[serial]
    fn write_reports_override_when_higher_layer_wins() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path();
        let home_config = workspace.join("home").join("vtcode.toml");
        let workspace_config = workspace.join("vtcode.toml");
        fs::create_dir_all(home_config.parent().expect("home parent")).expect("home dir");

        fs::write(&home_config, "agent.provider = \"openai\"\n").expect("home config");
        fs::write(&workspace_config, "agent.provider = \"gemini\"\n").expect("workspace config");

        let static_paths = StaticWorkspacePaths::new(workspace, workspace.join(".vtcode"));
        let provider =
            WorkspacePathsDefaults::new(Arc::new(static_paths)).with_home_paths(vec![home_config]);

        with_config_defaults_provider_for_test(Arc::new(provider), || {
            let response = ConfigService::write(ConfigWriteRequest {
                workspace: workspace.to_path_buf(),
                target: ConfigWriteTarget::User,
                path: "agent.provider".to_string(),
                value: TomlValue::String("anthropic".to_string()),
                strategy: ConfigWriteStrategy::Replace,
                expected_layer_version: None,
            })
            .expect("write response");

            assert_eq!(
                response.effective_value,
                Some(TomlValue::String("gemini".to_string()))
            );
            assert!(response.overridden_metadata.is_some());
        });
    }

    #[test]
    #[serial]
    fn write_rejects_stale_expected_version() {
        let temp = tempfile::tempdir().expect("tempdir");
        let workspace = temp.path();
        let workspace_config = workspace.join("vtcode.toml");
        fs::write(&workspace_config, "agent.provider = \"openai\"\n").expect("workspace config");

        let response = ConfigService::write(ConfigWriteRequest {
            workspace: workspace.to_path_buf(),
            target: ConfigWriteTarget::Workspace,
            path: "agent.provider".to_string(),
            value: TomlValue::String("anthropic".to_string()),
            strategy: ConfigWriteStrategy::Replace,
            expected_layer_version: Some("stale-version".to_string()),
        });

        assert!(response.is_err());
        let error = format!("{:#}", response.expect_err("expected stale version error"));
        assert!(error.contains("Layer version mismatch"));
    }
}
