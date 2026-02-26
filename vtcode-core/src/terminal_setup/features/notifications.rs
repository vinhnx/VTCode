//! Notification system configuration for terminal setup
//!
//! Generates terminal-specific configuration for system notifications
//! when tasks complete, including iTerm2 alerts and bell settings.

use crate::terminal_setup::detector::TerminalType;

/// Generate notification configuration for a terminal
pub fn generate_notification_config(
    terminal_type: TerminalType,
    _features: &[crate::terminal_setup::detector::TerminalFeature],
) -> Result<String, anyhow::Error> {
    let mut config_lines = Vec::new();

    // Add notification-specific configuration based on terminal type
    match terminal_type {
        TerminalType::ITerm2 => {
            config_lines.push("# iTerm2 Notification Configuration".to_string());
            config_lines.push("# To enable system notifications for task completion:".to_string());
            config_lines.push("# 1. Open iTerm2 Preferences".to_string());
            config_lines.push("# 2. Navigate to Profiles → Terminal".to_string());
            config_lines.push("# 3. Enable 'Silence bell' and Filter Alerts → 'Send escape sequence-generated alerts'".to_string());
            config_lines.push("# 4. Set your preferred notification delay".to_string());
            config_lines.push("#".to_string());
            config_lines.push(
                "# For shell integration notifications, add to your shell profile:".to_string(),
            );
            config_lines.push("# export ITERM2_SHELL_INTEGRATION_INSTALLED=1".to_string());
        }
        TerminalType::VSCode => {
            config_lines.push("# VS Code Terminal Notification Configuration".to_string());
            config_lines
                .push("# VS Code terminal supports system notifications through:".to_string());
            config_lines.push("# 1. Settings → Terminal → Integrated → Bell Duration".to_string());
            config_lines.push("# 2. Enable 'Terminal > Integrated: Enable Bell'".to_string());
            config_lines.push("#".to_string());
            config_lines.push("# For shell integration with notifications, consider:".to_string());
            config_lines
                .push("# - Using 'oh-my-zsh' or 'bash-it' with notification plugins".to_string());
        }
        TerminalType::Hyper => {
            config_lines.push("# Hyper Terminal Notification Configuration".to_string());
            config_lines.push("# Hyper supports notifications via plugins:".to_string());
            config_lines
                .push("# 1. Install hyper-statusline plugin for enhanced integration".to_string());
            config_lines.push("# 2. Add to ~/.hyper.js plugins array:".to_string());
            config_lines.push("#    plugins: [\"hyper-statusline\", \"hyper-search\"]".to_string());
        }
        TerminalType::WindowsTerminal => {
            config_lines.push("# Windows Terminal Notification Configuration".to_string());
            config_lines.push("# Windows Terminal supports notifications through:".to_string());
            config_lines.push("# 1. Settings → Profiles → Advanced → Bell style".to_string());
            config_lines
                .push("# 2. Enable 'Show terminal bell alert' in appearance settings".to_string());
        }
        TerminalType::Ghostty => {
            config_lines.push("# Ghostty Notification Configuration".to_string());
            config_lines.push("# Ghostty supports system notifications through:".to_string());
            config_lines.push("# 1. Settings → Terminal → Bell".to_string());
            config_lines
                .push("# 2. Enable 'Visual Bell' or 'Audible Bell' as preferred".to_string());
            config_lines
                .push("# 3. Configure 'Bell Duration' for visual notifications".to_string());
            config_lines.push("#".to_string());
            config_lines
                .push("# Ghostty also supports shell integration notifications via:".to_string());
            config_lines.push("# - Terminal bell escape sequences (\\a)".to_string());
        }
        TerminalType::WezTerm => {
            config_lines.push("# WezTerm Notification Configuration".to_string());
            config_lines.push("# WezTerm supports terminal bell notifications and can".to_string());
            config_lines.push("# surface desktop alerts depending on OS integration.".to_string());
            config_lines.push("#".to_string());
            config_lines.push("# Recommended: keep bell enabled and test with:".to_string());
            config_lines.push("# echo -e \"\\a\"".to_string());
        }
        TerminalType::TerminalApp => {
            config_lines.push("# Terminal.app Notification Configuration".to_string());
            config_lines
                .push("# macOS Terminal supports audible alerts via terminal bell.".to_string());
            config_lines.push("#".to_string());
            config_lines.push("# 1. Open Terminal → Settings → Profiles → Advanced".to_string());
            config_lines.push("# 2. Configure bell/alerts according to preference".to_string());
            config_lines.push("# 3. Test with: echo -e \"\\a\"".to_string());
        }
        TerminalType::Xterm => {
            config_lines.push("# xterm Notification Configuration".to_string());
            config_lines
                .push("# xterm provides reliable bell notifications (audible/visual).".to_string());
            config_lines.push("#".to_string());
            config_lines.push("# Ensure bell is enabled in your X resources.".to_string());
            config_lines.push("# Test with: echo -e \"\\a\"".to_string());
        }
        _ => {
            config_lines.push(format!("# {:?} Notification Configuration", terminal_type));
            config_lines
                .push("# This terminal supports standard ANSI bell notifications.".to_string());
            config_lines.push(
                "# Check your terminal's documentation for specific notification settings."
                    .to_string(),
            );
        }
    }

    Ok(config_lines.join("\n"))
}

/// Get notification setup instructions for a terminal
pub fn get_notification_instructions(terminal_type: TerminalType) -> Vec<String> {
    match terminal_type {
        TerminalType::ITerm2 => vec![
            "1. OPEN: iTerm2 → Preferences → Profiles → Terminal".to_string(),
            "2. ENABLE: 'Silence bell' and 'Send escape sequence-generated alerts'".to_string(),
            "3. CONFIGURE: Set preferred notification delay".to_string(),
            "4. TEST: Run a long command followed by 'echo -e \"\\a\"'".to_string(),
        ],
        TerminalType::VSCode => vec![
            "1. OPEN: VS Code Settings (Cmd/Ctrl + ,)".to_string(),
            "2. SEARCH: 'terminal integrated bell'".to_string(),
            "3. ENABLE: 'Terminal > Integrated: Enable Bell'".to_string(),
            "4. CONFIGURE: Set 'Terminal > Integrated: Bell Duration'".to_string(),
        ],
        TerminalType::WindowsTerminal => vec![
            "1. OPEN: Windows Terminal Settings (Ctrl + ,)".to_string(),
            "2. SELECT: Your profile".to_string(),
            "3. GO TO: Appearance section".to_string(),
            "4. ENABLE: 'Show terminal bell alert'".to_string(),
        ],
        TerminalType::Ghostty => vec![
            "1. OPEN: Ghostty Settings".to_string(),
            "2. GO TO: Terminal → Bell section".to_string(),
            "3. ENABLE: 'Visual Bell' or 'Audible Bell' as preferred".to_string(),
            "4. CONFIGURE: Set 'Bell Duration' for visual notifications".to_string(),
        ],
        TerminalType::WezTerm => vec![
            "1. OPEN: WezTerm settings (~/.wezterm.lua)".to_string(),
            "2. ENABLE: Bell/alert behavior as preferred".to_string(),
            "3. TEST: Run 'echo -e \"\\a\"'".to_string(),
        ],
        TerminalType::TerminalApp => vec![
            "1. OPEN: Terminal → Settings → Profiles → Advanced".to_string(),
            "2. CONFIGURE: Alert/Bell behavior".to_string(),
            "3. TEST: Run 'echo -e \"\\a\"'".to_string(),
        ],
        TerminalType::Xterm => vec![
            "1. CHECK: xterm bell settings in X resources".to_string(),
            "2. ENABLE: Audible or visual bell".to_string(),
            "3. TEST: Run 'echo -e \"\\a\"'".to_string(),
        ],
        _ => vec![
            format!("1. CHECK: Documentation for {:?}", terminal_type),
            "2. CONFIGURE: Terminal bell/notifications in preferences".to_string(),
            "3. TEST: Use 'echo -e \"\\a\"' to test bell notification".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_config_generation() {
        let features = vec![
            crate::terminal_setup::detector::TerminalFeature::Multiline,
            crate::terminal_setup::detector::TerminalFeature::CopyPaste,
        ];

        let config = generate_notification_config(TerminalType::ITerm2, &features).unwrap();
        assert!(config.contains("iTerm2"));
        assert!(config.contains("Notification"));
    }

    #[test]
    fn test_notification_instructions() {
        let instructions = get_notification_instructions(TerminalType::ITerm2);
        assert!(instructions.iter().any(|i| i.contains("iTerm2")));
        assert!(instructions.iter().any(|i| i.contains("Preferences")));
    }
}
