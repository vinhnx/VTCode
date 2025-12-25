//! Kitty terminal configuration generator.
//!
//! Generates configuration for Kitty terminal emulator.

use anyhow::Result;
use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::detector::TerminalType;

/// Generate complete Kitty configuration with all features
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut config_lines = Vec::new();

    // Add header comment
    config_lines.push("# VTCode Terminal Configuration for Kitty".to_string());
    config_lines.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_lines.push("# Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::Kitty)?;
                config_lines.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                config_lines.push("# Enhanced copy/paste".to_string());
                config_lines.push("enable_bracketed_paste yes".to_string());
                config_lines.push("copy_on_select clipboard".to_string());
                config_lines.push("mouse_map shift+left click grabbed,ungrabbed mouse_selection normal".to_string());
                config_lines.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                config_lines.push("# Shell integration".to_string());
                config_lines.push("shell_integration enabled".to_string());
                config_lines.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_lines.push("# Theme colors will be configured separately".to_string());
                config_lines.push(String::new());
            }
        }
    }

    Ok(config_lines.join("\n"))
}

/// Merge Kitty configuration with existing config file
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
        assert!(config.contains("map"));
    }

    #[test]
    fn test_generate_copy_paste_config() {
        let features = vec![TerminalFeature::CopyPaste];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("bracketed_paste"));
        assert!(config.contains("copy_on_select"));
    }

    #[test]
    fn test_generate_all_features() {
        let features = vec![
            TerminalFeature::Multiline,
            TerminalFeature::CopyPaste,
            TerminalFeature::ShellIntegration,
        ];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("shift+enter"));
        assert!(config.contains("copy_on_select"));
        assert!(config.contains("shell_integration"));
    }

    #[test]
    fn test_merge_with_existing() {
        let existing = "# User config\nfont_family Monospace\n";
        let new_config = "map shift+enter send_text all \\n";

        let merged = merge_with_existing(existing, new_config).unwrap();

        assert!(merged.contains("font_family"));
        assert!(merged.contains("BEGIN VTCODE CONFIGURATION"));
        assert!(merged.contains("shift+enter"));
    }
}
