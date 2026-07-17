use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::defaults::ConfigDefaultsProvider;

pub const DEFAULT_GITIGNORE_FILE_NAME: &str = ".vtcodegitignore";

/// Determine where configuration and gitignore files should be created when
/// bootstrapping a workspace.
pub fn determine_bootstrap_targets(
    workspace: &Path,
    use_home_dir: bool,
    config_file_name: &str,
    defaults_provider: &dyn ConfigDefaultsProvider,
) -> Result<(PathBuf, PathBuf)> {
    if let (true, Some(home_config_path)) = (
        use_home_dir,
        select_home_config_path(defaults_provider, config_file_name),
    ) {
        let gitignore_path = gitignore_path_for(&home_config_path);
        return Ok((home_config_path, gitignore_path));
    }

    let config_path = workspace.join(config_file_name);
    let gitignore_path = workspace.join(DEFAULT_GITIGNORE_FILE_NAME);
    Ok((config_path, gitignore_path))
}

/// Returns the preferred gitignore path for a given configuration file.
pub fn gitignore_path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|parent| parent.join(DEFAULT_GITIGNORE_FILE_NAME))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_GITIGNORE_FILE_NAME))
}

/// Ensures the parent directory for the provided path exists, creating it if
/// necessary.
pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if parent.exists() {
            return Ok(());
        }

        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    Ok(())
}

/// Selects the home directory configuration path from the defaults provider or
/// falls back to the system home directory.
pub fn select_home_config_path(
    defaults_provider: &dyn ConfigDefaultsProvider,
    config_file_name: &str,
) -> Option<PathBuf> {
    let home_paths = defaults_provider.home_config_paths(config_file_name);
    home_paths
        .into_iter()
        .next()
        .or_else(|| default_home_dir().map(|dir| dir.join(config_file_name)))
}

/// Attempts to resolve the current user's home directory using common
/// environment variables and the `dirs` crate fallback.
pub fn default_home_dir() -> Option<PathBuf> {
    if let Ok(home) = std::env::var("HOME") {
        return Some(PathBuf::from(home));
    }

    if let Ok(userprofile) = std::env::var("USERPROFILE") {
        return Some(PathBuf::from(userprofile));
    }

    dirs::home_dir()
}
