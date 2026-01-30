use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::task;
use toml::Value as TomlValue;
use vtcode_core::config::WorkspaceTrustLevel;
use vtcode_core::config::loader::{ConfigManager, VTCodeConfig};
use vtcode_core::utils::dot_config::load_user_config;

pub(crate) const CONFIG_MODAL_TITLE: &str = "VT Code Configuration";
#[allow(dead_code)]
pub(crate) const MODAL_CLOSE_HINT: &str = "Press Esc to close the configuration modal.";
const SENSITIVE_KEYWORDS: [&str; 5] = ["key", "token", "secret", "password", "credential"];
const SENSITIVE_MASK: &str = "********";

pub(crate) struct ConfigModalContent {
    #[allow(dead_code)]
    pub(crate) title: String,
    pub(crate) source_label: String,
    #[allow(dead_code)]
    pub(crate) trust_label: Option<String>,
    pub(crate) config_lines: Vec<String>,
}

pub(crate) async fn load_config_modal_content(
    workspace: PathBuf,
    vt_cfg: Option<VTCodeConfig>,
) -> Result<ConfigModalContent> {
    let trust_label = resolve_workspace_trust_label(&workspace, &vt_cfg).await;
    task::spawn_blocking(move || prepare_config_modal_content(&workspace, vt_cfg, trust_label))
        .await
        .map_err(|err| anyhow::anyhow!("failed to join configuration load task: {}", err))?
}

fn prepare_config_modal_content(
    workspace: &Path,
    vt_cfg: Option<VTCodeConfig>,
    trust_label: Option<String>,
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
        trust_label,
        config_lines,
    })
}

async fn resolve_workspace_trust_label(
    workspace: &Path,
    vt_cfg: &Option<VTCodeConfig>,
) -> Option<String> {
    if let Some(cfg) = vt_cfg
        && cfg.acp.zed.enabled
    {
        let mode_label = match cfg.acp.zed.workspace_trust {
            vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::FullAuto => "full_auto",
            vtcode_core::config::AgentClientProtocolZedWorkspaceTrustMode::ToolsPolicy => {
                "tools_policy"
            }
        };
        return Some(format!("Trust: acp:{}", mode_label));
    }

    let Ok(config) = load_user_config().await else {
        return None;
    };

    let workspace_key = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf())
        .to_string_lossy()
        .into_owned();
    config
        .workspace_trust
        .entries
        .get(&workspace_key)
        .map(|record| match record.level {
            WorkspaceTrustLevel::FullAuto => "Trust: full auto".to_string(),
            WorkspaceTrustLevel::ToolsPolicy => "Trust: tools policy".to_string(),
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
