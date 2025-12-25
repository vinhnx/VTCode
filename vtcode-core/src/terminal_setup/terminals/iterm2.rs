//! iTerm2 configuration instruction generator.
//!
//! iTerm2 uses plist files which are complex to modify programmatically.
//! This module generates manual setup instructions instead.

use anyhow::Result;
use crate::terminal_setup::features::multiline;
use crate::terminal_setup::detector::TerminalType;

/// Generate iTerm2 setup instructions (manual configuration required)
pub fn generate_config(features: &[crate::terminal_setup::detector::TerminalFeature]) -> Result<String> {
    let mut instructions = vec![
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        "  iTerm2 Manual Configuration Instructions".to_string(),
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string(),
        String::new(),
        "iTerm2 requires manual configuration via the GUI.".to_string(),
        "Follow these steps to configure each feature:".to_string(),
        String::new(),
    ];

    for (i, feature) in features.iter().enumerate() {
        match feature {
            crate::terminal_setup::detector::TerminalFeature::Multiline => {
                instructions.push(format!("{}. MULTILINE INPUT (Shift+Enter)", i + 1));
                instructions.push(String::new());
                let multiline_instructions = multiline::generate_config(TerminalType::ITerm2)?;
                instructions.push(multiline_instructions);
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::CopyPaste => {
                instructions.push(format!("{}. COPY/PASTE INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to General → Selection".to_string());
                instructions.push("3. Enable 'Copy to pasteboard on selection'".to_string());
                instructions.push("4. Go to Pointer tab".to_string());
                instructions.push("5. Set middle-click action to 'Paste from Clipboard'".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ShellIntegration => {
                instructions.push(format!("{}. SHELL INTEGRATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to Profiles → General".to_string());
                instructions.push("3. Under 'Command', select your shell".to_string());
                instructions.push("4. iTerm2's shell integration will auto-install on first launch".to_string());
                instructions.push("5. Or manually install: curl -L https://iterm2.com/shell_integration/install_shell_integration.sh | bash".to_string());
                instructions.push(String::new());
            }
            crate::terminal_setup::detector::TerminalFeature::ThemeSync => {
                instructions.push(format!("{}. THEME SYNCHRONIZATION", i + 1));
                instructions.push(String::new());
                instructions.push("1. Open iTerm2 Preferences (Cmd+,)".to_string());
                instructions.push("2. Go to Profiles → Colors".to_string());
                instructions.push("3. Choose a color preset or customize manually".to_string());
                instructions.push("4. VTCode theme colors can be manually configured here".to_string());
                instructions.push(String::new());
            }
        }
    }

    instructions.push("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━".to_string());
    instructions.push("After configuration, restart iTerm2 for changes to take effect.".to_string());
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
            TerminalFeature::CopyPaste,
        ];
        let instructions = generate_config(&features).unwrap();
        assert!(instructions.contains("iTerm2"));
        assert!(instructions.contains("Preferences"));
        assert!(instructions.contains("MULTILINE"));
        assert!(instructions.contains("COPY/PASTE"));
    }
}
