//! Zed terminal configuration generator.
//!
//! Generates JSON configuration for Zed editor's integrated terminal.

use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::detector::TerminalType;
use crate::terminal_setup::features::multiline;
use anyhow::Result;

/// Generate complete Zed configuration with all features
pub fn generate_config(
    features: &[crate::terminal_setup::detector::TerminalFeature],
) -> Result<String> {
    let mut config_sections = Vec::new();

    // Add header comment
    config_sections.push("// VT Code Terminal Configuration for Zed".to_string());
    config_sections.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_sections.push("// Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::Zed)?;
                config_sections.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                // Zed doesn't support custom copy/paste config for terminal
                config_sections
                    .push("// Copy/paste: Built-in to Zed, no custom config needed".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                // Zed doesn't support custom shell integration
                config_sections.push("// Shell integration: Not supported".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_sections.push("// Theme colors will be configured separately".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::Notifications => {
                config_sections
                    .push("// System notifications: Not supported in Zed terminal".to_string());
                config_sections.push("// Use Zed's notification system instead".to_string());
                config_sections.push(String::new());
            }
        }
    }

    Ok(config_sections.join("\n"))
}

/// Merge Zed configuration with existing JSON config file
pub fn merge_with_existing(existing_content: &str, new_config: &str) -> Result<String> {
    ConfigWriter::merge_with_markers(existing_content, new_config, ConfigFormat::Json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_multiline_config() {
        let features = vec![TerminalFeature::Multiline];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("shift-enter") || config.contains("Multiline"));
    }

    #[test]
    fn test_generate_theme_config() {
        let features = vec![TerminalFeature::ThemeSync];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("Theme"));
    }
}
