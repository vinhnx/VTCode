//! Theme Manager for loading and applying custom theme configurations
//!
//! This module provides functionality to load custom theme configurations
//! from .vtcode/theme.toml files and apply them to the application.

use std::path::Path;
use anyhow::Result;
use crate::ui::{ThemeConfig, GitColorConfig, FileColorizer};

#[derive(Debug, Clone)]
pub struct ThemeManager {
    /// Custom theme configuration loaded from theme.toml
    pub custom_config: Option<ThemeConfig>,
    
    /// System Git configuration
    pub git_config: Option<GitColorConfig>,
    
    /// System file colorizer
    pub file_colorizer: FileColorizer,
}

impl ThemeManager {
    /// Create a new ThemeManager with loaded configurations
    pub fn new(workspace_root: Option<&Path>) -> Self {
        let custom_config = Self::load_custom_config(workspace_root);
        let git_config = Self::load_git_config(workspace_root);
        let file_colorizer = FileColorizer::new();
        
        Self {
            custom_config,
            git_config,
            file_colorizer,
        }
    }
    
    /// Load custom theme configuration from .vtcode/theme.toml
    fn load_custom_config(workspace_root: Option<&Path>) -> Option<ThemeConfig> {
        if let Some(workspace) = workspace_root {
            let theme_path = workspace.join(".vtcode").join("theme.toml");
            match ThemeConfig::load_from_file(&theme_path) {
                Ok(config) => {
                    tracing::info!("Loaded custom theme configuration from: {}", theme_path.display());
                    Some(config)
                },
                Err(e) => {
                    if theme_path.exists() {
                        tracing::warn!("Failed to load theme config from {}: {}", theme_path.display(), e);
                    }
                    None
                }
            }
        } else {
            None
        }
    }
    
    /// Load Git configuration for diff/status colors
    fn load_git_config(workspace_root: Option<&Path>) -> Option<GitColorConfig> {
        if let Some(workspace) = workspace_root {
            let git_config_path = workspace.join(".git").join("config");
            if git_config_path.exists() {
                match GitColorConfig::from_git_config(&git_config_path) {
                    Ok(config) => {
                        tracing::info!("Loaded Git color configuration from: {}", git_config_path.display());
                        Some(config)
                    },
                    Err(e) => {
                        tracing::warn!("Failed to load Git config from {}: {}", git_config_path.display(), e);
                        GitColorConfig::default().into()
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    }
    
    /// Get the active theme configuration, falling back to defaults
    pub fn active_theme_config(&self) -> ThemeConfig {
        self.custom_config.clone().unwrap_or_else(ThemeConfig::default)
    }
    
    /// Load theme from a specific file path
    pub fn load_theme_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let config = ThemeConfig::load_from_file(path)?;
        self.custom_config = Some(config);
        Ok(())
    }
    
    /// Reset to default theme configuration
    pub fn reset_to_default(&mut self) {
        self.custom_config = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_theme_manager_default() {
        let manager = ThemeManager::new(None);
        assert!(manager.custom_config.is_none());
        assert!(manager.git_config.is_none());
        // FileColorizer should always be initialized
        assert_eq!(manager.file_colorizer.style_for_path(Path::new("/tmp/test.rs")), None);
    }

    #[test]
    fn test_load_custom_config() {
        let temp_dir = tempdir().unwrap();
        let vtcode_dir = temp_dir.path().join(".vtcode");
        fs::create_dir_all(&vtcode_dir).unwrap();
        
        let theme_content = r#"
[cli]
success = "bold green"
error = "bold red"

[diff]
new = "green"
old = "red"

[status]
added = "green"

[files]
directory = "bold blue"
"#;
        
        let theme_path = vtcode_dir.join("theme.toml");
        fs::write(&theme_path, theme_content).unwrap();

        let manager = ThemeManager::new(Some(temp_dir.path()));
        assert!(manager.custom_config.is_some());
        
        let config = manager.active_theme_config();
        assert_eq!(config.cli.success, "bold green");
        assert_eq!(config.diff.new, "green");
        assert_eq!(config.status.added, "green");
    }

    #[test]
    fn test_load_theme_from_file() {
        let temp_dir = tempdir().unwrap();
        let theme_content = r#"
[cli]
success = "cyan"
"#;
        
        let theme_path = temp_dir.path().join("custom_theme.toml");
        fs::write(&theme_path, theme_content).unwrap();

        let mut manager = ThemeManager::new(None);
        assert!(manager.custom_config.is_none());
        
        manager.load_theme_from_file(&theme_path).unwrap();
        assert!(manager.custom_config.is_some());
        
        let config = manager.active_theme_config();
        assert_eq!(config.cli.success, "cyan");
    }

    #[test]
    fn test_reset_to_default() {
        let temp_dir = tempdir().unwrap();
        let theme_content = r#"
[cli]
success = "yellow"
"#;
        
        let theme_path = temp_dir.path().join("custom_theme.toml");
        fs::write(&theme_path, theme_content).unwrap();

        let mut manager = ThemeManager::new(None);
        manager.load_theme_from_file(&theme_path).unwrap();
        assert!(manager.custom_config.is_some());
        
        manager.reset_to_default();
        assert!(manager.custom_config.is_none());
    }
}