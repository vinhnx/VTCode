//! Copy/paste integration feature configuration generator.
//!
//! Generates terminal-specific configuration for enhanced copy/paste features:
//! - Copy-on-select (automatically copy selected text)
//! - Bracketed paste mode (safe paste with escape sequences)
//! - Middle-click paste integration

use crate::terminal_setup::detector::TerminalType;
use anyhow::Result;

/// Generate copy/paste configuration for the specified terminal
pub fn generate_config(terminal: TerminalType) -> Result<String> {
    let config = match terminal {
        TerminalType::Ghostty => r#"# Copy on select
copy-on-select = true

# Clipboard integration
clipboard-read = allow
clipboard-write = allow
"#
        .to_string(),

        TerminalType::Kitty => r#"# Enhanced copy/paste
enable_bracketed_paste yes
copy_on_select clipboard

# Clipboard integration
clipboard_control write-clipboard write-primary read-clipboard read-primary
"#
        .to_string(),

        TerminalType::Alacritty => r#"[selection]
save_to_clipboard = true

[mouse]
# Middle-click paste
bindings = [
    { mouse = "Middle", action = "PasteSelection" }
]
"#
        .to_string(),

        TerminalType::WezTerm => r#"-- WezTerm copy/paste is built-in.
-- Optional:
--   enable_kitty_keyboard = true
"#
        .to_string(),

        TerminalType::TerminalApp => r#"Terminal.app copy/paste is built-in.
Configure profile behavior in Terminal → Settings.
"#
        .to_string(),

        TerminalType::Xterm => {
            r#"xterm copy/paste behavior is controlled via X resources and selection settings.
"#
            .to_string()
        }

        TerminalType::Zed => r#"// Copy/paste is built into Zed
// No additional configuration needed
"#
        .to_string(),

        TerminalType::Warp => {
            "# Warp has built-in copy/paste support\n# No additional configuration needed\n"
                .to_string()
        }

        TerminalType::WindowsTerminal => r#"{
  "copyOnSelect": true,
  "copyFormatting": "none",
  "actions": [
    {
      "command": { "action": "copy", "singleLine": false },
      "keys": "ctrl+c"
    },
    {
      "command": "paste",
      "keys": "ctrl+v"
    }
  ]
}
"#
        .to_string(),

        TerminalType::Hyper => r#"config: {
  copyOnSelect: true,
  quickEdit: true,
}
"#
        .to_string(),

        TerminalType::Tabby => r#"terminal:
  copyOnSelect: true
  pasteOnMiddleClick: true
  rightClick: menu
"#
        .to_string(),

        TerminalType::ITerm2 => r#"Manual iTerm2 Copy/Paste Setup:

1. Open iTerm2 Preferences (Cmd+,)
2. Go to General → Selection
3. Enable "Copy to pasteboard on selection"
4. Go to Pointer tab
5. Set middle-click action to "Paste from Clipboard"
6. Under Advanced, search for "paste" to customize paste behavior
"#
        .to_string(),

        TerminalType::VSCode => r#"VS Code Copy/Paste Configuration:

Copy/paste is built-in to VS Code terminal.
No additional configuration needed.

Default shortcuts:
  - Copy: Cmd+C (macOS) / Ctrl+C (Windows/Linux)
  - Paste: Cmd+V (macOS) / Ctrl+V (Windows/Linux)

To customize, edit settings.json:
  "terminal.integrated.copyOnSelection": true
"#
        .to_string(),

        TerminalType::Unknown => {
            anyhow::bail!("Cannot generate copy/paste config for unknown terminal type");
        }
    };

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ghostty_config() {
        let config = generate_config(TerminalType::Ghostty).unwrap();
        assert!(config.contains("copy-on-select"));
        assert!(config.contains("clipboard"));
    }

    #[test]
    fn test_generate_kitty_config() {
        let config = generate_config(TerminalType::Kitty).unwrap();
        assert!(config.contains("copy_on_select"));
        assert!(config.contains("bracketed_paste"));
    }

    #[test]
    fn test_generate_alacritty_config() {
        let config = generate_config(TerminalType::Alacritty).unwrap();
        assert!(config.contains("save_to_clipboard"));
        assert!(config.contains("PasteSelection"));
    }

    #[test]
    fn test_generate_windows_terminal_config() {
        let config = generate_config(TerminalType::WindowsTerminal).unwrap();
        assert!(config.contains("copyOnSelect"));
        assert!(config.contains("paste"));
    }

    #[test]
    fn test_generate_iterm2_instructions() {
        let config = generate_config(TerminalType::ITerm2).unwrap();
        assert!(config.contains("iTerm2"));
        assert!(config.contains("Preferences"));
    }

    #[test]
    fn test_generate_vscode_instructions() {
        let config = generate_config(TerminalType::VSCode).unwrap();
        assert!(config.contains("VS Code"));
        assert!(config.contains("copyOnSelection"));
    }

    #[test]
    fn test_unknown_terminal_error() {
        let result = generate_config(TerminalType::Unknown);
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_config() {
        // This test exists for backward compatibility with the stub
        assert!(generate_config(TerminalType::Kitty).is_ok());
    }
}
