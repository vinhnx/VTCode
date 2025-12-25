//! Alacritty terminal configuration generator.
//!
//! Generates TOML configuration for Alacritty terminal emulator.

use anyhow::Result;
use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::detector::TerminalType;

/// Generate complete Alacritty configuration with all features
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut config_sections = Vec::new();

    // Add header comment
    config_sections.push("# VTCode Terminal Configuration for Alacritty".to_string());
    config_sections.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_sections.push("# Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::Alacritty)?;
                config_sections.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                config_sections.push("# Enhanced copy/paste".to_string());
                config_sections.push("[selection]".to_string());
                config_sections.push("save_to_clipboard = true".to_string());
                config_sections.push(String::new());
                config_sections.push("[[mouse.bindings]]".to_string());
                config_sections.push("mouse = \"Middle\"".to_string());
                config_sections.push("action = \"PasteSelection\"".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                config_sections.push("# Shell integration".to_string());
                config_sections.push("[shell]".to_string());
                config_sections.push("program = \"/bin/bash\"".to_string());
                config_sections.push("args = [\"--login\"]".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_sections.push("# Theme colors will be configured separately".to_string());
                config_sections.push(String::new());
            }
        }
    }

    Ok(config_sections.join("\n"))
}

/// Merge Alacritty configuration with existing TOML config file
pub fn merge_with_existing(existing_content: &str, new_config: &str) -> Result<String> {
    ConfigWriter::merge_with_markers(existing_content, new_config, ConfigFormat::Toml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_multiline_config() {
        let features = vec![TerminalFeature::Multiline];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("keyboard.bindings"));
        assert!(config.contains("Return"));
        assert!(config.contains("Shift"));
    }

    #[test]
    fn test_generate_copy_paste_config() {
        let features = vec![TerminalFeature::CopyPaste];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("[selection]"));
        assert!(config.contains("save_to_clipboard"));
    }

    #[test]
    fn test_generate_all_features() {
        let features = vec![
            TerminalFeature::Multiline,
            TerminalFeature::CopyPaste,
            TerminalFeature::ShellIntegration,
        ];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("keyboard.bindings"));
        assert!(config.contains("save_to_clipboard"));
        assert!(config.contains("[shell]"));
    }

    #[test]
    fn test_merge_with_existing() {
        let existing = "# User config\n[font]\nsize = 12\n";
        let new_config = "[[keyboard.bindings]]\nkey = \"Return\"\nmods = \"Shift\"\nchars = \"\\n\"";

        let merged = merge_with_existing(existing, new_config).unwrap();

        assert!(merged.contains("size = 12"));
        assert!(merged.contains("BEGIN VTCODE CONFIGURATION"));
        assert!(merged.contains("keyboard.bindings"));
    }
}
