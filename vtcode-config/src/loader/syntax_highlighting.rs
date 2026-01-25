use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

use crate::defaults::{self, SyntaxHighlightingDefaults};

/// Syntax highlighting configuration
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
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
