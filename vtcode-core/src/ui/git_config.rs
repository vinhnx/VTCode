/// Git configuration color parsing
///
/// Parses [color "..."] sections from .git/config and converts them to anstyle::Style objects.
/// This allows vtcode to respect user's Git color configuration for diff and status visualization.
///
/// # Example
/// ```ignore
/// use vtcode_core::ui::git_config::GitColorConfig;
/// use std::path::Path;
///
/// let config = GitColorConfig::from_git_config(Path::new(".git/config"))?;
/// let diff_style = config.diff_new;
/// ```

use anyhow::{Context, Result};
use anstyle::Style;
use std::path::Path;

/// Parsed Git configuration colors for diff, status, and branch visualization
#[derive(Debug, Clone)]
pub struct GitColorConfig {
    /// Color for added lines in diff (default: green)
    pub diff_new: Style,
    /// Color for removed lines in diff (default: red)
    pub diff_old: Style,
    /// Color for context/unchanged lines in diff (default: none)
    pub diff_context: Style,
    /// Color for diff headers (default: none)
    pub diff_header: Style,
    /// Color for file metadata lines (default: none)
    pub diff_meta: Style,
    /// Color for stat +++ markers (default: green)
    pub diff_frag: Style,
    
    /// Color for added files in status (default: green)
    pub status_added: Style,
    /// Color for modified files in status (default: red)
    pub status_modified: Style,
    /// Color for deleted files in status (default: red)
    pub status_deleted: Style,
    /// Color for untracked files in status (default: none)
    pub status_untracked: Style,
    
    /// Color for current branch (default: none)
    pub branch_current: Style,
    /// Color for local branches (default: none)
    pub branch_local: Style,
    /// Color for remote branches (default: none)
    pub branch_remote: Style,
}

impl Default for GitColorConfig {
    /// Returns default Git color configuration
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl GitColorConfig {
    /// Create Git color config with default values
    pub fn with_defaults() -> Self {
        Self {
            diff_new: crate::utils::style_helpers::style_from_color_name("green"),
            diff_old: crate::utils::style_helpers::style_from_color_name("red"),
            diff_context: Style::new(),
            diff_header: Style::new(),
            diff_meta: Style::new(),
            diff_frag: crate::utils::style_helpers::style_from_color_name("cyan"),
            status_added: crate::utils::style_helpers::style_from_color_name("green"),
            status_modified: crate::utils::style_helpers::style_from_color_name("red"),
            status_deleted: crate::utils::style_helpers::style_from_color_name("red"),
            status_untracked: Style::new(),
            branch_current: Style::new(),
            branch_local: Style::new(),
            branch_remote: Style::new(),
        }
    }

    /// Load Git colors from .git/config file
    ///
    /// Parses [color "diff"], [color "status"], and [color "branch"] sections.
    /// Falls back to defaults for any missing colors.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read, but parsing errors are logged
    /// and defaults are used for invalid color values.
    pub fn from_git_config(config_path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read Git config: {}", config_path.display()))?;

        let mut config = Self::with_defaults();

        // Parse [color "diff"] section
        if let Some(diff_new) = Self::extract_git_color(&content, "diff", "new") {
            config.diff_new = diff_new;
        }
        if let Some(diff_old) = Self::extract_git_color(&content, "diff", "old") {
            config.diff_old = diff_old;
        }
        if let Some(diff_context) = Self::extract_git_color(&content, "diff", "context") {
            config.diff_context = diff_context;
        }
        if let Some(diff_header) = Self::extract_git_color(&content, "diff", "header") {
            config.diff_header = diff_header;
        }
        if let Some(diff_meta) = Self::extract_git_color(&content, "diff", "meta") {
            config.diff_meta = diff_meta;
        }
        if let Some(diff_frag) = Self::extract_git_color(&content, "diff", "frag") {
            config.diff_frag = diff_frag;
        }

        // Parse [color "status"] section
        if let Some(status_added) = Self::extract_git_color(&content, "status", "added") {
            config.status_added = status_added;
        }
        if let Some(status_modified) = Self::extract_git_color(&content, "status", "modified") {
            config.status_modified = status_modified;
        }
        if let Some(status_deleted) = Self::extract_git_color(&content, "status", "deleted") {
            config.status_deleted = status_deleted;
        }
        if let Some(status_untracked) = Self::extract_git_color(&content, "status", "untracked") {
            config.status_untracked = status_untracked;
        }

        // Parse [color "branch"] section
        if let Some(branch_current) = Self::extract_git_color(&content, "branch", "current") {
            config.branch_current = branch_current;
        }
        if let Some(branch_local) = Self::extract_git_color(&content, "branch", "local") {
            config.branch_local = branch_local;
        }
        if let Some(branch_remote) = Self::extract_git_color(&content, "branch", "remote") {
            config.branch_remote = branch_remote;
        }

        Ok(config)
    }

    /// Extract a single Git color setting from config content
    ///
    /// Looks for patterns like: [color "section"] key = value
    fn extract_git_color(content: &str, section: &str, key: &str) -> Option<Style> {
        // Pattern: [color "section"]
        let section_pattern = format!(r#"\[color "{}"\]"#, regex::escape(section));
        
        // Find the section
        let section_re = regex::Regex::new(&section_pattern).ok()?;
        let section_start = section_re.find(content)?.end();
        
        // Find the next section or end of file
        let section_end = if let Some(next_section) = regex::Regex::new(r"\[").ok()
            .and_then(|re| re.find(&content[section_start..]))
        {
            section_start + next_section.start()
        } else {
            content.len()
        };
        
        let section_content = &content[section_start..section_end];
        
        // Pattern: key = value
        let key_pattern = format!(r"{}\s*=\s*(.+?)(?:\r?\n|$)", regex::escape(key));
        let key_re = regex::Regex::new(&key_pattern).ok()?;
        
        let value = key_re.captures(section_content)?
            .get(1)?
            .as_str()
            .trim();
        
        // Try to parse with anstyle_git directly
        anstyle_git::parse(value).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_git_color_config_defaults() {
        let config = GitColorConfig::default();
        assert_ne!(config.diff_new, Style::new());
        assert_ne!(config.diff_old, Style::new());
    }

    fn create_test_git_config(content: &str) -> Result<GitColorConfig> {
        let mut temp = tempfile::NamedTempFile::new()?;
        temp.write_all(content.as_bytes())?;
        temp.flush()?;
        GitColorConfig::from_git_config(temp.path())
    }

    #[test]
    fn test_parse_git_config_diff_section() {
        let config_text = r#"
[core]
    bare = false

[color "diff"]
    new = green
    old = red
    context = white
"#;
        let config = create_test_git_config(config_text).expect("Failed to parse test config");
        
        // Should have parsed colors
        assert_ne!(config.diff_new, Style::new());
        assert_ne!(config.diff_old, Style::new());
    }

    #[test]
    fn test_parse_git_config_status_section() {
        let config_text = r#"
[color "status"]
    added = green bold
    modified = red
    deleted = red bold
    untracked = magenta
"#;
        let config = create_test_git_config(config_text).expect("Failed to parse test config");
        
        // Should have parsed status colors
        assert_ne!(config.status_added, Style::new());
        assert_ne!(config.status_modified, Style::new());
    }

    #[test]
    fn test_parse_git_config_hex_colors() {
        let config_text = r#"
[color "diff"]
    new = #00ff00
    old = #ff0000
"#;
        let config = create_test_git_config(config_text).expect("Failed to parse test config");
        
        // Should have parsed hex colors
        assert_ne!(config.diff_new, Style::new());
        assert_ne!(config.diff_old, Style::new());
    }

    #[test]
    fn test_parse_git_config_missing_file() {
        let result = GitColorConfig::from_git_config(Path::new("/nonexistent/.git/config"));
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_git_config_empty_file() {
        let config_text = "";
        let config = create_test_git_config(config_text).expect("Failed to parse empty config");
        
        // Should fall back to defaults
        assert_ne!(config.diff_new, Style::new());
        assert_ne!(config.diff_old, Style::new());
    }

    #[test]
    fn test_parse_git_config_branch_section() {
        let config_text = r#"
[color "branch"]
    current = green bold
    local = cyan
    remote = red
"#;
        let config = create_test_git_config(config_text).expect("Failed to parse test config");
        
        // Should have parsed branch colors
        assert_ne!(config.branch_current, Style::new());
    }

    #[test]
    fn test_parse_git_config_all_sections() {
        let config_text = r#"
[color "diff"]
    new = green
    old = red

[color "status"]
    added = green
    modified = yellow

[color "branch"]
    current = cyan bold
"#;
        let config = create_test_git_config(config_text).expect("Failed to parse test config");
        
        // Should have parsed colors from all sections
        assert_ne!(config.diff_new, Style::new());
        assert_ne!(config.status_added, Style::new());
        assert_ne!(config.branch_current, Style::new());
    }
}
