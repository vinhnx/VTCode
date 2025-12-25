//! Hyper terminal configuration generator.
//!
//! Generates JavaScript configuration for Hyper terminal.

use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::detector::TerminalType;
use crate::terminal_setup::features::multiline;
use anyhow::Result;

/// Generate complete Hyper configuration with all features
pub fn generate_config(
    features: &[crate::terminal_setup::detector::TerminalFeature],
) -> Result<String> {
    let mut config_sections = Vec::new();

    // Add header comment
    config_sections.push("// VT Code Terminal Configuration for Hyper".to_string());
    config_sections.push("// Add this to your ~/.hyper.js config file".to_string());
    config_sections.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_sections.push("// Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::Hyper)?;
                config_sections.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                config_sections.push("// Enhanced copy/paste".to_string());
                config_sections.push(
                    r#"config: {
  copyOnSelect: true,
  quickEdit: true,
}
"#
                    .to_string(),
                );
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                config_sections.push("// Shell integration: Use Hyper plugins".to_string());
                config_sections.push("// Install: hyper-statusline, hyper-search".to_string());
                config_sections.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_sections.push("// Theme colors will be configured separately".to_string());
                config_sections.push(String::new());
            }
        }
    }

    Ok(config_sections.join("\n"))
}

/// Merge Hyper configuration with existing JavaScript config file
pub fn merge_with_existing(existing_content: &str, new_config: &str) -> Result<String> {
    ConfigWriter::merge_with_markers(existing_content, new_config, ConfigFormat::JavaScript)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_multiline_config() {
        let features = vec![TerminalFeature::Multiline];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("shift+enter") || config.contains("Multiline"));
    }

    #[test]
    fn test_generate_copy_paste_config() {
        let features = vec![TerminalFeature::CopyPaste];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("copyOnSelect"));
    }
}
