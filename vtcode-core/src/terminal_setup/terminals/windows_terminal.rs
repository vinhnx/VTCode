//! Windows Terminal configuration generator.
//!
//! Generates JSON configuration for Windows Terminal.

use anyhow::Result;
use crate::terminal_setup::config_writer::{ConfigFormat, ConfigWriter};
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::detector::TerminalType;

/// Generate complete Windows Terminal configuration with all features
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut config_sections = Vec::new();

    // Add header comment
    config_sections.push("// VTCode Terminal Configuration for Windows Terminal".to_string());
    config_sections.push(String::new());

    // Generate feature-specific configurations
    for feature in features {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                config_sections.push("// Multiline input: Shift+Enter".to_string());
                let multiline_config = multiline::generate_config(TerminalType::WindowsTerminal)?;
                config_sections.push(multiline_config);
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                config_sections.push("// Enhanced copy/paste".to_string());
                config_sections.push(r#"{
  "copyOnSelect": true,
  "copyFormatting": false
}
"#.to_string());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                config_sections.push("// Shell integration".to_string());
                config_sections.push(r#"{
  "experimental.rendering.forceFullRepaint": false,
  "experimental.rendering.software": false
}
"#.to_string());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                config_sections.push("// Theme colors will be configured separately".to_string());
                config_sections.push(String::new());
            }
        }
    }

    Ok(config_sections.join("\n"))
}

/// Merge Windows Terminal configuration with existing JSON config file
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
        assert!(config.contains("sendInput") || config.contains("shift+enter"));
    }

    #[test]
    fn test_generate_copy_paste_config() {
        let features = vec![TerminalFeature::CopyPaste];
        let config = generate_config(&features).unwrap();
        assert!(config.contains("copyOnSelect"));
    }
}
