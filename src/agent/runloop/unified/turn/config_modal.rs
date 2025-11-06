use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::task;
use toml::Value as TomlValue;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};

pub(crate) const CONFIG_MODAL_TITLE: &str = "VTCode Configuration";
pub(crate) const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];
const SENSITIVE_MASK: &str = "********";

pub(crate) struct ConfigModalContent {
    pub(crate) title: String,
    pub(crate) source_label: String,
    pub(crate) config_lines: Vec<String>,
}

pub(crate) async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg))
        .await
        .map_err(|err| anyhow::anyhow!("failed to join configuration load task: {}", err))?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_sensitive_keys_in_tables() {
        let mut value = TomlValue::Table(
            [
                (
                    "api_key".to_string(),
                    TomlValue::String("secret".to_string()),
                ),
                (
                    "nested".to_string(),
                    TomlValue::Table(
                        [("token".to_string(), TomlValue::String("value".to_string()))]
                            .into_iter()
                            .collect(),
                    ),
                ),
            ]
            .into_iter()
            .collect(),
        );

        mask_sensitive_config(&mut value);

        let TomlValue::Table(table) = value else {
            panic!("expected table")
        };
        assert_eq!(
            table.get("api_key"),
            Some(&TomlValue::String(SENSITIVE_MASK.to_string()))
        );
        if let Some(TomlValue::Table(nested)) = table.get("nested") {
            assert_eq!(
                nested.get("token"),
                Some(&TomlValue::String(SENSITIVE_MASK.to_string()))
            );
        } else {
            panic!("expected nested table");
        }
    }
}
