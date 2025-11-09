//! Theme Configuration File Support
//!
//! Parses custom .vtcode/theme.toml files with Git/LS-style syntax for colors.
//! This allows users to customize colors beyond system defaults.

use std::path::Path;
use anyhow::{Context, Result};
use anstyle::Style as AnsiStyle;
use serde::{Deserialize, Serialize};
use crate::utils::CachedStyleParser;

/// Theme configuration that can be loaded from a .vtcode/theme.toml file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    /// Colors for CLI elements
    #[serde(default)]
    pub cli: CliColors,
    
    /// Colors for diff rendering
    #[serde(default)]
    pub diff: DiffColors,
    
    /// Colors for status output
    #[serde(default)]
    pub status: StatusColors,
    
    /// Colors for file types (LS_COLORS-style)
    #[serde(default)]
    pub files: FileColors,
}

impl ThemeConfig {
    /// Load theme configuration from a TOML file
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read theme file: {}", path.display()))?;
        
        let config: ThemeConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse theme file: {}", path.display()))?;
        
        Ok(config)
    }
    
    /// Create default theme configuration
    pub fn default() -> Self {
        Self::default_config()
    }
    
    /// Returns a default configuration
    fn default_config() -> Self {
        Self {
            cli: CliColors::default(),
            diff: DiffColors::default(),
            status: StatusColors::default(),
            files: FileColors::default(),
        }
    }
}

/// Colors for CLI elements like prompts, messages, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliColors {
    /// Color for success messages
    #[serde(default = "default_cli_success")]
    pub success: String,
    
    /// Color for error messages
    #[serde(default = "default_cli_error")]
    pub error: String,
    
    /// Color for warning messages
    #[serde(default = "default_cli_warning")]
    pub warning: String,
    
    /// Color for info messages
    #[serde(default = "default_cli_info")]
    pub info: String,
    
    /// Color for prompt text
    #[serde(default = "default_cli_prompt")]
    pub prompt: String,
}

impl Default for CliColors {
    fn default() -> Self {
        Self {
            success: "green".to_string(),
            error: "red".to_string(),
            warning: "yellow".to_string(),
            info: "cyan".to_string(),
            prompt: "bold blue".to_string(),
        }
    }
}

/// Colors for diff rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffColors {
    /// Color for added lines in diff
    #[serde(default = "default_diff_new")]
    pub new: String,
    
    /// Color for removed lines in diff
    #[serde(default = "default_diff_old")]
    pub old: String,
    
    /// Color for context/unchanged lines in diff
    #[serde(default = "default_diff_context")]
    pub context: String,
    
    /// Color for diff headers
    #[serde(default = "default_diff_header")]
    pub header: String,
    
    /// Color for diff metadata
    #[serde(default = "default_diff_meta")]
    pub meta: String,
    
    /// Color for diff fragment indicators
    #[serde(default = "default_diff_frag")]
    pub frag: String,
}

impl Default for DiffColors {
    fn default() -> Self {
        Self {
            new: "green".to_string(),
            old: "red".to_string(),
            context: "white".to_string(),
            header: "bold yellow".to_string(),
            meta: "cyan".to_string(),
            frag: "magenta".to_string(),
        }
    }
}

/// Colors for status output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusColors {
    /// Color for added files
    #[serde(default = "default_status_added")]
    pub added: String,
    
    /// Color for modified files
    #[serde(default = "default_status_modified")]
    pub modified: String,
    
    /// Color for deleted files
    #[serde(default = "default_status_deleted")]
    pub deleted: String,
    
    /// Color for untracked files
    #[serde(default = "default_status_untracked")]
    pub untracked: String,
    
    /// Color for current branch
    #[serde(default = "default_status_current")]
    pub current: String,
    
    /// Color for local branches
    #[serde(default = "default_status_local")]
    pub local: String,
    
    /// Color for remote branches
    #[serde(default = "default_status_remote")]
    pub remote: String,
}

impl Default for StatusColors {
    fn default() -> Self {
        Self {
            added: "green".to_string(),
            modified: "red".to_string(),
            deleted: "red bold".to_string(),
            untracked: "magenta".to_string(),
            current: "green bold".to_string(),
            local: "cyan".to_string(),
            remote: "blue".to_string(),
        }
    }
}

/// File type colors using LS_COLORS-style patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileColors {
    /// Directory color
    #[serde(default = "default_file_directory")]
    pub directory: String,
    
    /// Symbolic link color
    #[serde(default = "default_file_symlink")]
    pub symlink: String,
    
    /// Executable file color
    #[serde(default = "default_file_executable")]
    pub executable: String,
    
    /// Regular file color
    #[serde(default = "default_file_regular")]
    pub regular: String,
    
    /// Custom colors for file extensions
    #[serde(default)]
    pub extensions: std::collections::HashMap<String, String>,
}

impl Default for FileColors {
    fn default() -> Self {
        let mut extensions = std::collections::HashMap::new();
        extensions.insert("rs".to_string(), "cyan".to_string());
        extensions.insert("js".to_string(), "yellow".to_string());
        extensions.insert("ts".to_string(), "blue".to_string());
        extensions.insert("py".to_string(), "green".to_string());
        extensions.insert("toml".to_string(), "magenta".to_string());
        extensions.insert("md".to_string(), "white".to_string());
        
        Self {
            directory: "bold blue".to_string(),
            symlink: "bold magenta".to_string(),
            executable: "bold green".to_string(),
            regular: "".to_string(),
            extensions,
        }
    }
}

// Default value functions
fn default_cli_success() -> String { "green".to_string() }
fn default_cli_error() -> String { "red".to_string() }
fn default_cli_warning() -> String { "yellow".to_string() }
fn default_cli_info() -> String { "cyan".to_string() }
fn default_cli_prompt() -> String { "bold blue".to_string() }

fn default_diff_new() -> String { "green".to_string() }
fn default_diff_old() -> String { "red".to_string() }
fn default_diff_context() -> String { "white".to_string() }
fn default_diff_header() -> String { "bold yellow".to_string() }
fn default_diff_meta() -> String { "cyan".to_string() }
fn default_diff_frag() -> String { "magenta".to_string() }

fn default_status_added() -> String { "green".to_string() }
fn default_status_modified() -> String { "red".to_string() }
fn default_status_deleted() -> String { "red bold".to_string() }
fn default_status_untracked() -> String { "magenta".to_string() }
fn default_status_current() -> String { "green bold".to_string() }
fn default_status_local() -> String { "cyan".to_string() }
fn default_status_remote() -> String { "blue".to_string() }

fn default_file_directory() -> String { "bold blue".to_string() }
fn default_file_symlink() -> String { "bold magenta".to_string() }
fn default_file_executable() -> String { "bold green".to_string() }
fn default_file_regular() -> String { "".to_string() }

impl ThemeConfig {
    /// Convert CLI colors to anstyle::Style
    pub fn parse_cli_styles(&self) -> Result<ParsedCliColors> {
        let parser = CachedStyleParser::default();
        Ok(ParsedCliColors {
            success: parser.parse_flexible(&self.cli.success)?,
            error: parser.parse_flexible(&self.cli.error)?,
            warning: parser.parse_flexible(&self.cli.warning)?,
            info: parser.parse_flexible(&self.cli.info)?,
            prompt: parser.parse_flexible(&self.cli.prompt)?,
        })
    }
    
    /// Convert diff colors to anstyle::Style
    pub fn parse_diff_styles(&self) -> Result<ParsedDiffColors> {
        let parser = CachedStyleParser::default();
        Ok(ParsedDiffColors {
            new: parser.parse_flexible(&self.diff.new)?,
            old: parser.parse_flexible(&self.diff.old)?,
            context: parser.parse_flexible(&self.diff.context)?,
            header: parser.parse_flexible(&self.diff.header)?,
            meta: parser.parse_flexible(&self.diff.meta)?,
            frag: parser.parse_flexible(&self.diff.frag)?,
        })
    }
    
    /// Convert status colors to anstyle::Style
    pub fn parse_status_styles(&self) -> Result<ParsedStatusColors> {
        let parser = CachedStyleParser::default();
        Ok(ParsedStatusColors {
            added: parser.parse_flexible(&self.status.added)?,
            modified: parser.parse_flexible(&self.status.modified)?,
            deleted: parser.parse_flexible(&self.status.deleted)?,
            untracked: parser.parse_flexible(&self.status.untracked)?,
            current: parser.parse_flexible(&self.status.current)?,
            local: parser.parse_flexible(&self.status.local)?,
            remote: parser.parse_flexible(&self.status.remote)?,
        })
    }
    
    /// Convert file colors to anstyle::Style
    pub fn parse_file_styles(&self) -> Result<ParsedFileColors> {
        let parser = CachedStyleParser::default();
        let mut extension_styles = std::collections::HashMap::new();
        for (ext, color_str) in &self.files.extensions {
            let style = parser.parse_flexible(color_str)
                .with_context(|| format!("Failed to parse style for extension '{}': {}", ext, color_str))?;
            extension_styles.insert(ext.clone(), style);
        }
        
        Ok(ParsedFileColors {
            directory: parser.parse_flexible(&self.files.directory)?,
            symlink: parser.parse_flexible(&self.files.symlink)?,
            executable: parser.parse_flexible(&self.files.executable)?,
            regular: parser.parse_flexible(&self.files.regular)?,
            extensions: extension_styles,
        })
    }
}

/// Parsed CLI colors with anstyle::Style values
#[derive(Debug, Clone)]
pub struct ParsedCliColors {
    pub success: AnsiStyle,
    pub error: AnsiStyle,
    pub warning: AnsiStyle,
    pub info: AnsiStyle,
    pub prompt: AnsiStyle,
}

/// Parsed diff colors with anstyle::Style values
#[derive(Debug, Clone)]
pub struct ParsedDiffColors {
    pub new: AnsiStyle,
    pub old: AnsiStyle,
    pub context: AnsiStyle,
    pub header: AnsiStyle,
    pub meta: AnsiStyle,
    pub frag: AnsiStyle,
}

/// Parsed status colors with anstyle::Style values
#[derive(Debug, Clone)]
pub struct ParsedStatusColors {
    pub added: AnsiStyle,
    pub modified: AnsiStyle,
    pub deleted: AnsiStyle,
    pub untracked: AnsiStyle,
    pub current: AnsiStyle,
    pub local: AnsiStyle,
    pub remote: AnsiStyle,
}

/// Parsed file colors with anstyle::Style values
#[derive(Debug, Clone)]
pub struct ParsedFileColors {
    pub directory: AnsiStyle,
    pub symlink: AnsiStyle,
    pub executable: AnsiStyle,
    pub regular: AnsiStyle,
    pub extensions: std::collections::HashMap<String, AnsiStyle>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ThemeConfig::default();
        assert_eq!(config.cli.success, "green");
        assert_eq!(config.diff.new, "green");
        assert_eq!(config.status.added, "green");
        assert_eq!(config.files.directory, "bold blue");
    }

    #[test]
    fn test_load_from_toml() {
        let toml_content = r#"
[cli]
success = "bold green"
error = "bold red"

[diff]
new = "green"
old = "red"

[status]
added = "green"
modified = "yellow"

[files]
directory = "bold blue"
executable = "bold cyan"

[files.extensions]
"rs" = "bright cyan"
"py" = "bright yellow"
"#;

        let temp_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(&temp_file, toml_content).unwrap();

        let config = ThemeConfig::load_from_file(&temp_file).expect("Failed to load config");
        assert_eq!(config.cli.success, "bold green");
        assert_eq!(config.diff.new, "green");
        assert_eq!(config.files.extensions.get("rs"), Some(&"bright cyan".to_string()));
        assert_eq!(config.files.extensions.get("py"), Some(&"bright yellow".to_string()));
    }

    #[test]
    fn test_parse_styles() {
        let config = ThemeConfig::default();
        
        let cli_styles = config.parse_cli_styles().expect("Failed to parse CLI styles");
        assert_ne!(cli_styles.success, AnsiStyle::new());
        
        let diff_styles = config.parse_diff_styles().expect("Failed to parse diff styles");
        assert_ne!(diff_styles.new, AnsiStyle::new());
        
        let status_styles = config.parse_status_styles().expect("Failed to parse status styles");
        assert_ne!(status_styles.added, AnsiStyle::new());
        
        let file_styles = config.parse_file_styles().expect("Failed to parse file styles");
        assert_ne!(file_styles.directory, AnsiStyle::new());
    }

    #[test]
    fn test_parse_custom_styles() {
        let mut config = ThemeConfig::default();
        config.cli.success = "bold red ul".to_string();
        config.diff.new = "#00ff00".to_string();  // RGB green
        config.files.symlink = "01;35".to_string();  // ANSI code for bold magenta
        
        let cli_styles = config.parse_cli_styles().expect("Failed to parse CLI styles");
        assert!(cli_styles.success.get_effects().contains(anstyle::Effects::BOLD));
        assert!(cli_styles.success.get_effects().contains(anstyle::Effects::UNDERLINE));
        
        let diff_styles = config.parse_diff_styles().expect("Failed to parse diff styles");
        // The green color should be set
        assert_ne!(diff_styles.new.get_fg_color(), None);
        
        let file_styles = config.parse_file_styles().expect("Failed to parse file styles");
        assert!(file_styles.symlink.get_effects().contains(anstyle::Effects::BOLD));
    }
}