//! Global configuration persistence for the `vtcode mcp` subcommands.

use crate::config::VTCodeConfig;
use crate::config::loader::ConfigManager;
use anyhow::{Context, Result, anyhow};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use vtcode_commons::VtCodePaths;

static GLOBAL_CONFIG_PATH_OVERRIDE: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

pub(crate) fn load_global_config() -> Result<(VTCodeConfig, PathBuf)> {
    let path = global_config_path()?;
    let manager = ConfigManager::load_global()
        .with_context(|| format!("failed to load global configuration before writing {}", path.display()))?;
    Ok((manager.config().clone(), path))
}

pub(crate) async fn write_global_config(path: &Path, config: &VTCodeConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        VtCodePaths::ensure_user_dir(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let contents = toml::to_string_pretty(config).context("failed to serialize configuration")?;
    VtCodePaths::write_private_file_atomic(path, contents.as_bytes())
        .with_context(|| format!("failed to write configuration to {}", path.display()))?;
    Ok(())
}

fn global_config_path() -> Result<PathBuf> {
    if let Some(path) = GLOBAL_CONFIG_PATH_OVERRIDE
        .lock()
        .map_err(|_e| anyhow!("global config path override mutex poisoned"))?
        .clone()
    {
        return Ok(path);
    }

    VtCodePaths::resolve()
        .map(|paths| paths.config_file())
        .context("failed to resolve canonical VT Code configuration path")
}

#[doc(hidden)]
pub fn set_global_config_path_override_for_tests(path: Option<PathBuf>) -> Result<()> {
    let mut guard = GLOBAL_CONFIG_PATH_OVERRIDE
        .lock()
        .map_err(|_e| anyhow!("global config path override mutex poisoned"))?;
    *guard = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GLOBAL_CONFIG_PATH_OVERRIDE, global_config_path};
    use std::path::PathBuf;

    #[test]
    fn global_config_path_uses_test_override() {
        let override_path = PathBuf::from("/tmp/vtcode-mcp-test.toml");
        *GLOBAL_CONFIG_PATH_OVERRIDE.lock().expect("override mutex should be available") = Some(override_path.clone());
        let resolved = global_config_path().expect("global config path");
        assert_eq!(resolved, override_path);
        *GLOBAL_CONFIG_PATH_OVERRIDE.lock().expect("override mutex should be available") = None;
    }
}
