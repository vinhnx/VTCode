//! Ghostty terminal configuration generator.
//!
//! Generates configuration for Ghostty terminal emulator.

use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::detector::TerminalType;
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::features::shell_integration;
use anyhow::Result;

/// Generate complete Ghostty configuration with all features
pub fn generate_config(
    features: &[crate::terminal_setup::detector::TerminalFeature],
) -> Result<String> {
    let mut config_lines = Vec::new();

    // Add header comment
    config_lines.push("# VT Code Terminal Configuration for Ghostty".to_string());
    config_lines.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_lines.push("# Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::Ghostty)?;
                config_lines.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                config_lines.push("# Enhanced copy/paste".to_string());
                config_lines.push("copy-on-select = true".to_string());
                config_lines.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                let shell_config = shell_integration::generate_config(TerminalType::Ghostty)?;
                config_lines.push(shell_config);
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_lines.push("# Theme will be configured separately".to_string());
                config_lines.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::Notifications => {
                config_lines.push("# System notifications".to_string());
                config_lines.push("bell = true".to_string());
                config_lines.push(String::new());
            }
        }
    }

    Ok(config_lines.join("\n"))
}

/// Merge Ghostty configuration with existing config file
pub fn merge_with_existing(existing_content: &str, new_config: &str) -> Result<String> {
    ConfigWriter::merge_with_markers(existing_content, new_config, ConfigFormat::PlainText)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_multiline_config() {
        let features = vec![TerminalFeature::Multiline];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("shift+enter"));
        assert!(config.contains("keybind"));
    }

    #[test]
    fn test_generate_all_features() {
        let features = vec![
            TerminalFeature::Multiline,
            TerminalFeature::CopyPaste,
            TerminalFeature::ShellIntegration,
            TerminalFeature::ThemeSync,
        ];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("shift+enter"));
        assert!(config.contains("copy-on-select"));
        assert!(config.contains("shell-integration"));
    }

    #[test]
    fn test_merge_with_existing() {
        let existing = "# User config\nfont-family = Monospace\n";
        let new_config = "keybind = shift+enter=text:\\n";

        let merged = merge_with_existing(existing, new_config).unwrap();

        assert!(merged.contains("font-family"));
        assert!(merged.contains("BEGIN VTCODE CONFIGURATION"));
        assert!(merged.contains("keybind"));
    }
}
