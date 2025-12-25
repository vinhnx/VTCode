//! VS Code terminal configuration instruction generator.
//!
//! Generates instructions for configuring VS Code's integrated terminal.

use anyhow::Result;
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::detector::TerminalType;

/// Generate VS Code setup instructions
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut instructions = vec![
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        "  VS Code Terminal Manual Configuration".to_string(),
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        String::new(),
        "VS Code requires adding keybindings to keybindings.json".to_string(),
        "Follow these steps:".to_string(),
        String::new(),
    ];

    for (i, feature) in features.iter().enumerate() {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                instructions.push(format!("{}. MULTILINE INPUT (Shift+Enter)", i + 1));
                instructions.push(String::new());
                let multiline_instructions = multiline::generate_config(TerminalType::VSCode)?;
                instructions.push(multiline_instructions);
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                instructions.push(format!("{}. COPY/PASTE INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("Copy/paste is built-in to VS Code terminal.".to_string());
                instructions.push("No additional configuration needed.".to_string());
                instructions.push(String::new());
                instructions.push("Default shortcuts:".to_string());
                instructions.push("  - Copy: Cmd+C (macOS) / Ctrl+C (Windows/Linux)".to_string());
                instructions.push("  - Paste: Cmd+V (macOS) / Ctrl+V (Windows/Linux)".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                instructions.push(format!("{}. SHELL INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("VS Code has built-in shell integration.".to_string());
                instructions.push("To enable:".to_string());
                instructions.push("1. Open Settings (Cmd+, or Ctrl+,)".to_string());
                instructions.push("2. Search for 'terminal.integrated.shellIntegration.enabled'".to_string());
                instructions.push("3. Check the box to enable".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                instructions.push(format!("{}. THEME SYNCHRONIZATION", i + 1));
                instructions.push(String::new());
                instructions.push("VS Code terminal automatically matches your editor theme.".to_string());
                instructions.push("To customize:".to_string());
                instructions.push("1. Open Settings (Cmd+, or Ctrl+,)".to_string());
                instructions.push("2. Search for 'workbench.colorCustomizations'".to_string());
                instructions.push("3. Add terminal color overrides in settings.json".to_string());
                instructions.push(String::new());
            }
        }
    }

    instructions.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    instructions.push("After configuration, reload VS Code for changes to take effect.".to_string());
    instructions.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());

    Ok(instructions.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_setup::detector::TerminalFeature;

    #[test]
    fn test_generate_instructions() {
        let features = vec![
            TerminalFeature::Multiline,
            TerminalFeature::ShellIntegration,
        ];
        let instructions = generate_config(&features).unwrap();
        assert!(instructions.contains("VS Code"));
        assert!(instructions.contains("keybindings.json") || instructions.contains("Settings"));
        assert!(instructions.contains("MULTILINE"));
    }
}
