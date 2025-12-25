//! Safe atomic configuration file writing with smart merging.
//!
//! Provides atomic writes and marker-based config merging to preserve user customizations.

use anyhow::{Context, Result};
use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::NamedTempFile;

/// Configuration file format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFormat {
    /// Plain text or shell-style config (# comments)
    PlainText,
    /// TOML format
    Toml,
    /// JSON format
    Json,
    /// YAML format
    Yaml,
    /// JavaScript format (for .hyper.js, etc.)
    JavaScript,
}

/// Markers for identifying VTCode-managed configuration sections
pub const VTCODE_BEGIN_MARKER: &str = "# BEGIN VTCODE CONFIGURATION";
pub const VTCODE_END_MARKER: &str = "# END VTCODE CONFIGURATION";

/// Safe atomic configuration file writer
pub struct ConfigWriter;

impl ConfigWriter {
    /// Write content to a file atomically using a temp file and rename
    ///
    /// This ensures that the file is never left in a partially-written state
    pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
        }

        // Create temp file in the same directory for atomic rename
        let temp_file = NamedTempFile::new_in(
            path.parent().unwrap_or_else(|| Path::new(".")),
        )
        .with_context(|| format!("Failed to create temp file in directory: {}", path.display()))?;

        // Write content to temp file
        temp_file
            .as_file()
            .write_all(content.as_bytes())
            .with_context(|| "Failed to write content to temp file")?;

        // Sync to ensure data is written to disk
        temp_file
            .as_file()
            .sync_all()
            .with_context(|| "Failed to sync temp file to disk")?;

        // Atomically rename temp file to target path
        temp_file
            .persist(path)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    /// Merge new VTCode configuration section with existing config
    ///
    /// Removes old VTCode sections and adds the new section, preserving user customizations
    pub fn merge_with_markers(
        existing: &str,
        new_section: &str,
        format: ConfigFormat,
    ) -> Result<String> {
        // Remove any existing VTCode section
        let cleaned = Self::remove_vtcode_section(existing);

        // Add new VTCode section with markers
        let vtcode_section = Self::wrap_with_markers(new_section, format);

        // Determine where to insert the new section
        let merged = if cleaned.trim().is_empty() {
            // File is empty, just use the VTCode section
            vtcode_section
        } else {
            // Append VTCode section at the end
            format!("{}\n\n{}", cleaned.trim_end(), vtcode_section)
        };

        Ok(merged)
    }

    /// Remove existing VTCode configuration section from content
    fn remove_vtcode_section(content: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let mut result = Vec::new();
        let mut in_vtcode_section = false;

        for line in lines {
            if line.contains(VTCODE_BEGIN_MARKER) {
                in_vtcode_section = true;
                continue;
            }

            if line.contains(VTCODE_END_MARKER) {
                in_vtcode_section = false;
                continue;
            }

            if !in_vtcode_section {
                result.push(line);
            }
        }

        result.join("\n")
    }

    /// Wrap configuration content with VTCode markers
    fn wrap_with_markers(content: &str, format: ConfigFormat) -> String {
        let comment_prefix = match format {
            ConfigFormat::PlainText
            | ConfigFormat::Toml
            | ConfigFormat::Yaml => "#",
            ConfigFormat::Json => "//", // JSON doesn't support comments, but some parsers allow them
            ConfigFormat::JavaScript => "//",
        };

        let header = format!(
            "{} BEGIN VTCODE CONFIGURATION\n{} VTCode-managed section - auto-generated\n{} Do not edit manually",
            comment_prefix, comment_prefix, comment_prefix
        );

        let footer = format!("{} END VTCODE CONFIGURATION", comment_prefix);

        format!("{}\n{}\n{}", header, content.trim(), footer)
    }

    /// Detect config file format from extension
    pub fn detect_format(path: &Path) -> ConfigFormat {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            match ext {
                "toml" => ConfigFormat::Toml,
                "json" => ConfigFormat::Json,
                "yaml" | "yml" => ConfigFormat::Yaml,
                "js" => ConfigFormat::JavaScript,
                _ => ConfigFormat::PlainText,
            }
        } else {
            ConfigFormat::PlainText
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_atomic_write() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("test.conf");

        let content = "test content";
        ConfigWriter::write_atomic(&path, content).unwrap();

        assert!(path.exists());
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn test_remove_vtcode_section() {
        let content = r#"# User config
user_setting = 1

# BEGIN VTCODE CONFIGURATION
# VTCode-managed section - auto-generated
vtcode_setting = 2
# END VTCODE CONFIGURATION

# More user config
another_setting = 3
"#;

        let result = ConfigWriter::remove_vtcode_section(content);

        assert!(!result.contains("vtcode_setting"));
        assert!(result.contains("user_setting"));
        assert!(result.contains("another_setting"));
        assert!(!result.contains("VTCODE CONFIGURATION"));
    }

    #[test]
    fn test_merge_with_markers() {
        let existing = r#"# User config
user_setting = 1
"#;

        let new_section = "vtcode_setting = 2";

        let result = ConfigWriter::merge_with_markers(
            existing,
            new_section,
            ConfigFormat::PlainText,
        )
        .unwrap();

        assert!(result.contains("user_setting"));
        assert!(result.contains("vtcode_setting"));
        assert!(result.contains(VTCODE_BEGIN_MARKER));
        assert!(result.contains(VTCODE_END_MARKER));
    }

    #[test]
    fn test_merge_empty_file() {
        let new_section = "vtcode_setting = 1";

        let result = ConfigWriter::merge_with_markers(
            "",
            new_section,
            ConfigFormat::PlainText,
        )
        .unwrap();

        assert!(result.contains("vtcode_setting"));
        assert!(result.contains(VTCODE_BEGIN_MARKER));
    }

    #[test]
    fn test_detect_format() {
        assert_eq!(
            ConfigWriter::detect_format(Path::new("test.toml")),
            ConfigFormat::Toml
        );
        assert_eq!(
            ConfigWriter::detect_format(Path::new("test.json")),
            ConfigFormat::Json
        );
        assert_eq!(
            ConfigWriter::detect_format(Path::new("test.yaml")),
            ConfigFormat::Yaml
        );
        assert_eq!(
            ConfigWriter::detect_format(Path::new("test.js")),
            ConfigFormat::JavaScript
        );
        assert_eq!(
            ConfigWriter::detect_format(Path::new("test.conf")),
            ConfigFormat::PlainText
        );
    }

    #[test]
    fn test_wrap_with_markers_toml() {
        let content = "setting = 1";
        let result = ConfigWriter::wrap_with_markers(content, ConfigFormat::Toml);

        assert!(result.starts_with("# BEGIN VTCODE CONFIGURATION"));
        assert!(result.ends_with("# END VTCODE CONFIGURATION"));
        assert!(result.contains("setting = 1"));
    }

    #[test]
    fn test_wrap_with_markers_javascript() {
        let content = "const setting = 1;";
        let result = ConfigWriter::wrap_with_markers(content, ConfigFormat::JavaScript);

        assert!(result.starts_with("// BEGIN VTCODE CONFIGURATION"));
        assert!(result.contains("const setting = 1;"));
    }
}
