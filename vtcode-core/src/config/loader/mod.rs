use crate::config::acp::AgentClientProtocolConfig;
use crate::config::context::ContextFeaturesConfig;
use crate::config::core::{
    AgentConfig, AutomationConfig, CommandsConfig, PromptCachingConfig, SecurityConfig, ToolsConfig,
};
use crate::config::defaults::{self, SyntaxHighlightingDefaults};
use crate::config::mcp::McpClientConfig;
use crate::config::router::RouterConfig;
use crate::config::telemetry::TelemetryConfig;
use crate::config::{PtyConfig, UiConfig};
use crate::project::SimpleProjectManager;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Syntax highlighting configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyntaxHighlightingConfig {
    /// Enable syntax highlighting for tool output
    #[serde(default = "defaults::syntax_highlighting::enabled")]
    pub enabled: bool,

    /// Theme to use for syntax highlighting
    #[serde(default = "defaults::syntax_highlighting::theme")]
    pub theme: String,

    /// Enable theme caching for better performance
    #[serde(default = "defaults::syntax_highlighting::cache_themes")]
    pub cache_themes: bool,

    /// Maximum file size for syntax highlighting (in MB)
    #[serde(default = "defaults::syntax_highlighting::max_file_size_mb")]
    pub max_file_size_mb: usize,

    /// Languages to enable syntax highlighting for
    #[serde(default = "defaults::syntax_highlighting::enabled_languages")]
    pub enabled_languages: Vec<String>,

    /// Performance settings - highlight timeout in milliseconds
    #[serde(default = "defaults::syntax_highlighting::highlight_timeout_ms")]
    pub highlight_timeout_ms: u64,
}

impl Default for SyntaxHighlightingConfig {
    fn default() -> Self {
        Self {
            enabled: defaults::syntax_highlighting::enabled(),
            theme: defaults::syntax_highlighting::theme(),
            cache_themes: defaults::syntax_highlighting::cache_themes(),
            max_file_size_mb: defaults::syntax_highlighting::max_file_size_mb(),
            enabled_languages: defaults::syntax_highlighting::enabled_languages(),
            highlight_timeout_ms: defaults::syntax_highlighting::highlight_timeout_ms(),
        }
    }
}

impl SyntaxHighlightingConfig {
    pub fn validate(&self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        ensure!(
            self.max_file_size_mb >= SyntaxHighlightingDefaults::min_file_size_mb(),
            "Syntax highlighting max_file_size_mb must be at least {} MB",
            SyntaxHighlightingDefaults::min_file_size_mb()
        );

        ensure!(
            self.highlight_timeout_ms >= SyntaxHighlightingDefaults::min_highlight_timeout_ms(),
            "Syntax highlighting highlight_timeout_ms must be at least {} ms",
            SyntaxHighlightingDefaults::min_highlight_timeout_ms()
        );

        ensure!(
            !self.theme.trim().is_empty(),
            "Syntax highlighting theme must not be empty"
        );

        ensure!(
            self.enabled_languages
                .iter()
                .all(|lang| !lang.trim().is_empty()),
            "Syntax highlighting languages must not contain empty entries"
        );

        Ok(())
    }
}

/// Main configuration structure for VTCode
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VTCodeConfig {
    /// Agent-wide settings
    #[serde(default)]
    pub agent: AgentConfig,

    /// Tool execution policies
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Unix command permissions
    #[serde(default)]
    pub commands: CommandsConfig,

    /// Security settings
    #[serde(default)]
    pub security: SecurityConfig,

    /// UI settings
    #[serde(default)]
    pub ui: UiConfig,

    /// PTY settings
    #[serde(default)]
    pub pty: PtyConfig,

    /// Context features (e.g., Decision Ledger)
    #[serde(default)]
    pub context: ContextFeaturesConfig,

    /// Router configuration (dynamic model + engine selection)
    #[serde(default)]
    pub router: RouterConfig,

    /// Telemetry configuration (logging, trajectory)
    #[serde(default)]
    pub telemetry: TelemetryConfig,

    /// Syntax highlighting configuration
    #[serde(default)]
    pub syntax_highlighting: SyntaxHighlightingConfig,

    /// Automation configuration
    #[serde(default)]
    pub automation: AutomationConfig,

    /// Prompt cache configuration (local + provider integration)
    #[serde(default)]
    pub prompt_cache: PromptCachingConfig,

    /// Model Context Protocol configuration
    #[serde(default)]
    pub mcp: McpClientConfig,

    /// Agent Client Protocol configuration
    #[serde(default)]
    pub acp: AgentClientProtocolConfig,
}

impl Default for VTCodeConfig {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            tools: ToolsConfig::default(),
            commands: CommandsConfig::default(),
            security: SecurityConfig::default(),
            ui: UiConfig::default(),
            pty: PtyConfig::default(),
            context: ContextFeaturesConfig::default(),
            router: RouterConfig::default(),
            telemetry: TelemetryConfig::default(),
            syntax_highlighting: SyntaxHighlightingConfig::default(),
            automation: AutomationConfig::default(),
            prompt_cache: PromptCachingConfig::default(),
            mcp: McpClientConfig::default(),
            acp: AgentClientProtocolConfig::default(),
        }
    }
}

impl VTCodeConfig {
    pub fn validate(&self) -> Result<()> {
        self.syntax_highlighting
            .validate()
            .context("Invalid syntax_highlighting configuration")?;

        self.context
            .validate()
            .context("Invalid context configuration")?;

        self.router
            .validate()
            .context("Invalid router configuration")?;

        Ok(())
    }

    /// Bootstrap project with config + gitignore
    pub fn bootstrap_project<P: AsRef<Path>>(workspace: P, force: bool) -> Result<Vec<String>> {
        Self::bootstrap_project_with_options(workspace, force, false)
    }

    /// Bootstrap project with config + gitignore, with option to create in home directory
    pub fn bootstrap_project_with_options<P: AsRef<Path>>(
        workspace: P,
        force: bool,
        use_home_dir: bool,
    ) -> Result<Vec<String>> {
        let workspace = workspace.as_ref();
        let mut created_files = Vec::new();

        // Determine where to create the config file
        let (config_path, gitignore_path) = if use_home_dir {
            // Create in user's home directory
            if let Some(home_dir) = ConfigManager::get_home_dir() {
                let vtcode_dir = home_dir.join(".vtcode");
                // Create .vtcode directory if it doesn't exist
                if !vtcode_dir.exists() {
                    fs::create_dir_all(&vtcode_dir).with_context(|| {
                        format!("Failed to create directory: {}", vtcode_dir.display())
                    })?;
                }
                (
                    vtcode_dir.join("vtcode.toml"),
                    vtcode_dir.join(".vtcodegitignore"),
                )
            } else {
                // Fallback to workspace if home directory cannot be determined
                let config_path = workspace.join("vtcode.toml");
                let gitignore_path = workspace.join(".vtcodegitignore");
                (config_path, gitignore_path)
            }
        } else {
            // Create in workspace
            let config_path = workspace.join("vtcode.toml");
            let gitignore_path = workspace.join(".vtcodegitignore");
            (config_path, gitignore_path)
        };

        // Create vtcode.toml
        if !config_path.exists() || force {
            let config_content = Self::default_vtcode_toml_template();

            fs::write(&config_path, config_content).with_context(|| {
                format!("Failed to write config file: {}", config_path.display())
            })?;

            created_files.push("vtcode.toml".to_string());
        }

        // Create .vtcodegitignore
        if !gitignore_path.exists() || force {
            let gitignore_content = Self::default_vtcode_gitignore();
            fs::write(&gitignore_path, gitignore_content).with_context(|| {
                format!(
                    "Failed to write gitignore file: {}",
                    gitignore_path.display()
                )
            })?;

            created_files.push(".vtcodegitignore".to_string());
        }

        Ok(created_files)
    }

    /// Generate default .vtcodegitignore content
    fn default_vtcode_toml_template() -> String {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../vtcode.toml.example")).to_string()
    }

    fn default_vtcode_gitignore() -> String {
        r#"# Security-focused exclusions
.env, .env.local, secrets/, .aws/, .ssh/

# Development artifacts
target/, build/, dist/, node_modules/, vendor/

# Database files
*.db, *.sqlite, *.sqlite3

# Binary files
*.exe, *.dll, *.so, *.dylib, *.bin

# IDE files (comprehensive)
.vscode/, .idea/, *.swp, *.swo
"#
        .to_string()
    }

    /// Create sample configuration file
    pub fn create_sample_config<P: AsRef<Path>>(output: P) -> Result<()> {
        let output = output.as_ref();
        let config_content = Self::default_vtcode_toml_template();

        fs::write(output, config_content)
            .with_context(|| format!("Failed to write config file: {}", output.display()))?;

        Ok(())
    }
}

/// Configuration manager for loading and validating configurations
#[derive(Clone)]
pub struct ConfigManager {
    config: VTCodeConfig,
    config_path: Option<PathBuf>,
    project_manager: Option<SimpleProjectManager>,
    project_name: Option<String>,
}

impl ConfigManager {
    /// Load configuration from the default locations
    pub fn load() -> Result<Self> {
        Self::load_from_workspace(std::env::current_dir()?)
    }

    /// Get the user's home directory path
    fn get_home_dir() -> Option<PathBuf> {
        // Try standard environment variables
        if let Ok(home) = std::env::var("HOME") {
            return Some(PathBuf::from(home));
        }

        // Try USERPROFILE on Windows
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            return Some(PathBuf::from(userprofile));
        }

        // Fallback to dirs crate approach
        dirs::home_dir()
    }

    /// Load configuration from a specific workspace
    pub fn load_from_workspace(workspace: impl AsRef<Path>) -> Result<Self> {
        let workspace = workspace.as_ref();

        // Initialize project manager
        let project_manager = Some(SimpleProjectManager::new(workspace.to_path_buf()));
        let project_name = project_manager
            .as_ref()
            .and_then(|pm| pm.identify_current_project().ok());

        // Try vtcode.toml in workspace root first
        let config_path = workspace.join("vtcode.toml");
        if config_path.exists() {
            let config = Self::load_from_file(&config_path)?;
            return Ok(Self {
                config: config.config,
                config_path: config.config_path,
                project_manager,
                project_name,
            });
        }

        // Try .vtcode/vtcode.toml in workspace
        let fallback_path = workspace.join(".vtcode").join("vtcode.toml");
        if fallback_path.exists() {
            let config = Self::load_from_file(&fallback_path)?;
            return Ok(Self {
                config: config.config,
                config_path: config.config_path,
                project_manager,
                project_name,
            });
        }

        // Try ~/.vtcode/vtcode.toml in user home directory
        if let Some(home_dir) = Self::get_home_dir() {
            let home_config_path = home_dir.join(".vtcode").join("vtcode.toml");
            if home_config_path.exists() {
                let config = Self::load_from_file(&home_config_path)?;
                return Ok(Self {
                    config: config.config,
                    config_path: config.config_path,
                    project_manager,
                    project_name,
                });
            }
        }

        // Try project-specific configuration
        if let (Some(pm), Some(pname)) = (&project_manager, &project_name) {
            let project_config_path = pm.config_dir(pname).join("vtcode.toml");
            if project_config_path.exists() {
                let config = Self::load_from_file(&project_config_path)?;
                return Ok(Self {
                    config: config.config,
                    config_path: config.config_path,
                    project_manager: Some(pm.clone()),
                    project_name: Some(pname.clone()),
                });
            }
        }

        // Use default configuration if no file found
        let config = VTCodeConfig::default();
        config
            .validate()
            .context("Default configuration failed validation")?;

        Ok(Self {
            config,
            config_path: None,
            project_manager,
            project_name,
        })
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        let config: VTCodeConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        config
            .validate()
            .with_context(|| format!("Failed to validate config file: {}", path.display()))?;

        // Initialize project manager but don't set project name since we're loading from file
        // Use current directory as workspace root for file-based loading
        let project_manager = std::env::current_dir()
            .ok()
            .map(|cwd| SimpleProjectManager::new(cwd));

        Ok(Self {
            config,
            config_path: Some(path.to_path_buf()),
            project_manager,
            project_name: None,
        })
    }

    /// Get the loaded configuration
    pub fn config(&self) -> &VTCodeConfig {
        &self.config
    }

    /// Get the configuration file path (if loaded from file)
    pub fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }

    /// Get session duration from agent config
    pub fn session_duration(&self) -> std::time::Duration {
        std::time::Duration::from_secs(60 * 60) // Default 1 hour
    }

    /// Get the project manager (if available)
    pub fn project_manager(&self) -> Option<&SimpleProjectManager> {
        self.project_manager.as_ref()
    }

    /// Get the project name (if identified)
    pub fn project_name(&self) -> Option<&str> {
        self.project_name.as_deref()
    }

    /// Persist configuration to a specific path
    pub fn save_config_to_path(path: impl AsRef<Path>, config: &VTCodeConfig) -> Result<()> {
        let path = path.as_ref();
        let content =
            toml::to_string_pretty(config).context("Failed to serialize configuration")?;
        fs::write(path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;
        Ok(())
    }

    /// Persist configuration to the manager's associated path or workspace
    pub fn save_config(&self, config: &VTCodeConfig) -> Result<()> {
        if let Some(path) = &self.config_path {
            return Self::save_config_to_path(path, config);
        }

        if let Some(manager) = &self.project_manager {
            let path = manager.workspace_root().join("vtcode.toml");
            return Self::save_config_to_path(path, config);
        }

        let cwd = std::env::current_dir().context("Failed to resolve current directory")?;
        let path = cwd.join("vtcode.toml");
        Self::save_config_to_path(path, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn syntax_highlighting_defaults_are_valid() {
        let config = SyntaxHighlightingConfig::default();
        config
            .validate()
            .expect("default syntax highlighting config should be valid");
    }

    #[test]
    fn vtcode_config_validation_fails_for_invalid_highlight_timeout() {
        let mut config = VTCodeConfig::default();
        config.syntax_highlighting.highlight_timeout_ms = 0;
        let error = config
            .validate()
            .expect_err("validation should fail for zero highlight timeout");
        assert!(
            error.to_string().contains("highlight timeout"),
            "expected error to mention highlight timeout, got: {}",
            error
        );
    }

    #[test]
    fn load_from_file_rejects_invalid_syntax_highlighting() {
        let mut temp_file = NamedTempFile::new().expect("failed to create temp file");
        writeln!(
            temp_file,
            "[syntax_highlighting]\nhighlight_timeout_ms = 0\n"
        )
        .expect("failed to write temp config");

        let result = ConfigManager::load_from_file(temp_file.path());
        assert!(result.is_err(), "expected validation error");
        let error = result.unwrap_err();
        assert!(
            error.to_string().contains("validate"),
            "expected validation context in error, got: {}",
            error
        );
    }
}
