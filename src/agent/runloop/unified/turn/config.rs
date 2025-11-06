use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use tokio::task;
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;

const CONFIG_MODAL_TITLE: &str = "VTCode Configuration";
pub(super) const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];
const SENSITIVE_MASK: &str = "********";

#[derive(Clone, Debug)]
pub(super) struct ConfigModalContent {
    pub(super) title: String,
    pub(super) source_label: String,
    pub(super) config_lines: Vec<String>,
}

pub(super) async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg))
        .await
        .map_err(|err| anyhow!("failed to join configuration load task: {}", err))?
}

pub(super) async fn bootstrap_config_files(workspace: PathBuf, force: bool) -> Result<Vec<String>> {
    let label = workspace.display().to_string();
    let result = task::spawn_blocking(move || VTCodeConfig::bootstrap_project(&workspace, force))
        .await
        .map_err(|err| anyhow!("failed to join configuration bootstrap task: {}", err))?;
    result.with_context(|| format!("failed to initialize configuration in {}", label))
}

pub(super) async fn refresh_vt_config(
    workspace: &Path,
    runtime_cfg: &CoreAgentConfig,
    vt_cfg: &mut Option<VTCodeConfig>,
) -> Result<()> {
    let mut snapshot = load_workspace_config_snapshot(workspace).await?;
    super::super::super::apply_runtime_overrides(Some(&mut snapshot), runtime_cfg);
    *vt_cfg = Some(snapshot);
    Ok(())
}

fn prepare_config_modal_content(
    workspace: &Path,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    let manager = ConfigManager::load_from_workspace(workspace).with_context(|| {
        format!(
            "failed to resolve configuration for workspace {}",
            workspace.display()
        )
    })?;

    let config_path = manager.config_path().map(Path::to_path_buf);
    let config_data = if config_path.is_some() {
        manager.config().clone()
    } else if let Some(snapshot) = vt_cfg {
        snapshot
    } else {
        manager.config().clone()
    };

    let mut value = TomlValue::try_from(config_data)
        .context("failed to serialize configuration for display")?;
    mask_sensitive_config(&mut value);

    let formatted =
        toml::to_string_pretty(&value).context("failed to render configuration to TOML")?;
    let config_lines = formatted.lines().map(|line| line.to_string()).collect();

    let source_label = if let Some(path) = config_path {
        format!("Configuration source: {}", path.display())
    } else {
        "No vtcode.toml file found; showing runtime defaults.".to_string()
    };

    Ok(ConfigModalContent {
        title: CONFIG_MODAL_TITLE.to_string(),
        source_label,
        config_lines,
    })
}

fn mask_sensitive_config(value: &mut TomlValue) {
    match value {
        TomlValue::Table(table) => {
            for (key, entry) in table.iter_mut() {
                if is_sensitive_key(key) {
                    *entry = TomlValue::String(SENSITIVE_MASK.to_string());
                } else {
                    mask_sensitive_config(entry);
                }
            }
        }
        TomlValue::Array(items) => {
            for item in items {
                mask_sensitive_config(item);
            }
        }
        _ => {}
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lowered = key.to_ascii_lowercase();
    SENSITIVE_KEYWORDS
        .iter()
        .any(|keyword| lowered.contains(keyword))
}

async fn load_workspace_config_snapshot(workspace: &Path) -> Result<VTCodeConfig> {
    let workspace_buf = workspace.to_path_buf();
    let label = workspace_buf.display().to_string();
    let result = task::spawn_blocking(move || {
        ConfigManager::load_from_workspace(&workspace_buf).map(|manager| manager.config().clone())
    })
    .await
    .map_err(|err| anyhow!("failed to join workspace config load task: {}", err))?;

    result.with_context(|| format!("failed to load configuration for {}", label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_sensitive_keys_in_tables_and_arrays() {
        let mut value = TomlValue::try_from(serde_json::json!({
            "api_key": "secret",
            "nested": {"token": "value"},
            "list": [
                {"password": "p"},
                {"nested": {"credential": "c"}},
                "ignored"
            ]
        }))
        .unwrap();

        mask_sensitive_config(&mut value);

        assert_eq!(
            value,
            TomlValue::try_from(serde_json::json!({
                "api_key": SENSITIVE_MASK,
                "nested": {"token": SENSITIVE_MASK},
                "list": [
                    {"password": SENSITIVE_MASK},
                    {"nested": {"credential": SENSITIVE_MASK}},
                    "ignored"
                ]
            }))
            .unwrap()
        );
    }

    #[test]
    fn identifies_sensitive_keys_case_insensitively() {
        assert!(is_sensitive_key("Api_Key"));
        assert!(is_sensitive_key("TOKEN"));
        assert!(!is_sensitive_key("username"));
    }
}
