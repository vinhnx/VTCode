/// VT Code Configuration Management
///
/// Handles loading and parsing of vtcode.toml configuration files
/// from the workspace root.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Root VT Code configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub workspace: WorkspaceConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}

/// AI Provider Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default = "default_model")]
    pub model: String,
}

/// Workspace Settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub analyze_on_startup: bool,
    #[serde(default = "default_max_tokens")]
    pub max_context_tokens: usize,
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

/// Security Settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default = "default_human_in_loop")]
    pub human_in_the_loop: bool,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_model() -> String {
    "claude-4-5-sonnet".to_string()
}

fn default_max_tokens() -> usize {
    8000
}

fn default_human_in_loop() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ai: AiConfig {
                provider: default_provider(),
                model: default_model(),
            },
            workspace: WorkspaceConfig {
                analyze_on_startup: false,
                max_context_tokens: default_max_tokens(),
                ignore_patterns: vec![],
            },
            security: SecurityConfig {
                human_in_the_loop: default_human_in_loop(),
                allowed_tools: vec![],
            },
        }
    }
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            model: default_model(),
        }
    }
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            analyze_on_startup: false,
            max_context_tokens: default_max_tokens(),
            ignore_patterns: vec![],
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            human_in_the_loop: default_human_in_loop(),
            allowed_tools: vec![],
        }
    }
}

/// Load configuration from vtcode.toml
pub fn load_config(path: &Path) -> Result<Config, String> {
    // Check if file exists
    if !path.exists() {
        return Ok(Config::default());
    }

    // Read the file
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read vtcode.toml: {}", e))?;

    // Parse TOML
    let config: Config =
        toml::from_str(&content).map_err(|e| format!("Failed to parse vtcode.toml: {}", e))?;

    Ok(config)
}

/// Find vtcode.toml in workspace root or parent directories
pub fn find_config(start_path: &Path) -> Option<Config> {
    let mut current = start_path.to_path_buf();

    loop {
        let config_path = current.join("vtcode.toml");
        if config_path.exists() {
            if let Ok(config) = load_config(&config_path) {
                return Some(config);
            }
        }

        // Move to parent directory
        if !current.pop() {
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.ai.provider, "anthropic");
        assert_eq!(config.ai.model, "claude-4-5-sonnet");
        assert_eq!(config.workspace.max_context_tokens, 8000);
        assert!(!config.workspace.analyze_on_startup);
        assert!(config.security.human_in_the_loop);
    }

    #[test]
    fn test_ai_config_defaults() {
        let config = AiConfig::default();
        assert_eq!(config.provider, "anthropic");
        assert_eq!(config.model, "claude-4-5-sonnet");
    }

    #[test]
    fn test_workspace_config_defaults() {
        let config = WorkspaceConfig::default();
        assert!(!config.analyze_on_startup);
        assert_eq!(config.max_context_tokens, 8000);
        assert!(config.ignore_patterns.is_empty());
    }

    #[test]
    fn test_security_config_defaults() {
        let config = SecurityConfig::default();
        assert!(config.human_in_the_loop);
        assert!(config.allowed_tools.is_empty());
    }
}
