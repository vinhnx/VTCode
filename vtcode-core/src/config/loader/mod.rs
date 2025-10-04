use crate::config::acp::AgentClientProtocolConfig;
use crate::config::context::ContextFeaturesConfig;
use crate::config::core::{
    AgentConfig, AutomationConfig, CommandsConfig, PromptCachingConfig, SecurityConfig, ToolsConfig,
};
use crate::config::mcp::McpClientConfig;
use crate::config::router::RouterConfig;
use crate::config::telemetry::TelemetryConfig;
use crate::config::{PtyConfig, UiConfig};
use crate::project::SimpleProjectManager;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Syntax highlighting configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SyntaxHighlightingConfig {
    /// Enable syntax highlighting for tool output
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Theme to use for syntax highlighting
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Enable theme caching for better performance
    #[serde(default = "default_true")]
    pub cache_themes: bool,

    /// Maximum file size for syntax highlighting (in MB)
    #[serde(default = "default_max_file_size")]
    pub max_file_size_mb: usize,

    /// Languages to enable syntax highlighting for
    #[serde(default = "default_enabled_languages")]
    pub enabled_languages: Vec<String>,

    /// Performance settings - highlight timeout in milliseconds
    #[serde(default = "default_highlight_timeout")]
    pub highlight_timeout_ms: u64,
}

fn default_true() -> bool {
    true
}
fn default_theme() -> String {
    "base16-ocean.dark".to_string()
}
fn default_max_file_size() -> usize {
    10
}
fn default_enabled_languages() -> Vec<String> {
    vec![
        "rust".to_string(),
        "python".to_string(),
        "javascript".to_string(),
        "typescript".to_string(),
        "go".to_string(),
        "java".to_string(),
        "cpp".to_string(),
        "c".to_string(),
        "php".to_string(),
        "html".to_string(),
        "css".to_string(),
        "sql".to_string(),
        "csharp".to_string(),
        "bash".to_string(),
    ]
}
fn default_highlight_timeout() -> u64 {
    5000
}

impl Default for SyntaxHighlightingConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            theme: default_theme(),
            cache_themes: default_true(),
            max_file_size_mb: default_max_file_size(),
            enabled_languages: default_enabled_languages(),
            highlight_timeout_ms: default_highlight_timeout(),
        }
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
            let default_config = VTCodeConfig::default();
            let config_content = toml::to_string_pretty(&default_config)
                .context("Failed to serialize default configuration")?;

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
        let default_config = VTCodeConfig::default();
        let config_content = toml::to_string_pretty(&default_config)
            .context("Failed to serialize default configuration")?;

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
        Ok(Self {
            config: VTCodeConfig::default(),
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
