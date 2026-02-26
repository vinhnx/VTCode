//! Multiline input feature - Shift+Enter keybinding configuration.
//!
//! Generates terminal-specific configuration for binding Shift+Enter to insert a newline
//! character, enabling multiline input in VT Code.

use crate::terminal_setup::detector::TerminalType;
use anyhow::Result;

/// Generate Shift+Enter multiline configuration for a terminal
pub fn generate_config(terminal_type: TerminalType) -> Result<String> {
    let config = match terminal_type {
        TerminalType::Ghostty => generate_ghostty_config(),
        TerminalType::Kitty => generate_kitty_config(),
        TerminalType::Alacritty => generate_alacritty_config(),
        TerminalType::WezTerm => generate_wezterm_config(),
        TerminalType::TerminalApp => generate_terminal_app_instructions(),
        TerminalType::Xterm => generate_xterm_instructions(),
        TerminalType::Zed => generate_zed_config(),
        TerminalType::Warp => {
            // Warp has built-in multiline support, no config needed
            String::new()
        }
        TerminalType::ITerm2 => generate_iterm2_instructions(),
        TerminalType::VSCode => generate_vscode_instructions(),
        TerminalType::WindowsTerminal => generate_windows_terminal_config(),
        TerminalType::Hyper => generate_hyper_config(),
        TerminalType::Tabby => generate_tabby_config(),
        TerminalType::Unknown => {
            anyhow::bail!("Cannot generate multiline config for unknown terminal")
        }
    };

    Ok(config)
}

/// WezTerm: Lua keybinding example.
fn generate_wezterm_config() -> String {
    r#"keys = {
  { key = "Enter", mods = "SHIFT", action = wezterm.action.SendString("\n") },
}
"#
    .to_string()
}

/// Terminal.app: manual key mapping guidance.
fn generate_terminal_app_instructions() -> String {
    r#"Terminal.app uses profile key mappings.
Add Shift+Enter mapping to send \n in your active profile."#
        .to_string()
}

/// xterm: baseline guidance.
fn generate_xterm_instructions() -> String {
    r#"xterm multiline can be configured through X resources or window manager key mapping.
Ensure Shift+Enter sends a newline sequence."#
        .to_string()
}

/// Ghostty: keybind = shift+enter=text:\n
fn generate_ghostty_config() -> String {
    "keybind = shift+enter=text:\\n".to_string()
}

/// Kitty: map shift+enter send_text all \n
fn generate_kitty_config() -> String {
    "map shift+enter send_text all \\n".to_string()
}

/// Alacritty: TOML keyboard binding
fn generate_alacritty_config() -> String {
    r#"[[keyboard.bindings]]
key = "Return"
mods = "Shift"
chars = "\n"
"#
    .to_string()
}

/// Zed: JSON keybinding configuration
fn generate_zed_config() -> String {
    r#"{
  "bindings": {
    "shift-enter": "editor::Newline"
  }
}
"#
    .to_string()
}

/// Windows Terminal: JSON action binding
fn generate_windows_terminal_config() -> String {
    r#"{
  "actions": [
    {
      "command": {
        "action": "sendInput",
        "input": "\n"
      },
      "keys": "shift+enter"
    }
  ]
}
"#
    .to_string()
}

/// Hyper: JavaScript plugin configuration
fn generate_hyper_config() -> String {
    r#"// In your .hyper.js config:
module.exports = {
  config: {
    // ... other config
  },
  keymaps: {
    'window:devtools': 'cmd+alt+i',
    'window:reload': 'cmd+shift+r',
    'tab:new': 'cmd+t',
    'shift+enter': 'editor:newline'
  }
}
"#
    .to_string()
}

/// Tabby: YAML keybinding configuration
fn generate_tabby_config() -> String {
    r#"hotkeys:
  multiline-input:
    - Shift-Enter
terminal:
  sendInputOnEnter: true
"#
    .to_string()
}

/// iTerm2: Manual setup instructions (plist modification is complex)
fn generate_iterm2_instructions() -> String {
    r#"iTerm2 Manual Setup Instructions:

1. Open iTerm2 Preferences (Cmd+,)
2. Go to Profiles → Keys
3. Click the "+" button to add a new key mapping
4. Press Shift+Enter when prompted
5. Set Action to "Send Text"
6. Enter "\n" (without quotes) in the text field
7. Click OK to save

This will bind Shift+Enter to insert a newline character.
"#
    .to_string()
}

/// VS Code: JSON settings configuration
fn generate_vscode_instructions() -> String {
    r#"VS Code Terminal Manual Setup:

Add this to your keybindings.json (Cmd+K Cmd+S to open):

{
  "key": "shift+enter",
  "command": "workbench.action.terminal.sendSequence",
  "when": "terminalFocus",
  "args": { "text": "\n" }
}

Or use the UI:
1. Open Command Palette (Cmd+Shift+P)
2. Search for "Preferences: Open Keyboard Shortcuts (JSON)"
3. Add the keybinding above to the array
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ghostty_config() {
        let config = generate_config(TerminalType::Ghostty).unwrap();
        assert!(config.contains("keybind"));
        assert!(config.contains("shift+enter"));
    }

    #[test]
    fn test_generate_kitty_config() {
        let config = generate_config(TerminalType::Kitty).unwrap();
        assert!(config.contains("map shift+enter"));
        assert!(config.contains("send_text"));
    }

    #[test]
    fn test_generate_alacritty_config() {
        let config = generate_config(TerminalType::Alacritty).unwrap();
        assert!(config.contains("keyboard.bindings"));
        assert!(config.contains("Return"));
        assert!(config.contains("Shift"));
    }

    #[test]
    fn test_generate_windows_terminal_config() {
        let config = generate_config(TerminalType::WindowsTerminal).unwrap();
        assert!(config.contains("sendInput"));
        assert!(config.contains("shift+enter"));
    }

    #[test]
    fn test_warp_no_config_needed() {
        let config = generate_config(TerminalType::Warp).unwrap();
        assert!(config.is_empty());
    }

    #[test]
    fn test_iterm2_instructions() {
        let config = generate_config(TerminalType::ITerm2).unwrap();
        assert!(config.contains("Manual Setup"));
        assert!(config.contains("Preferences"));
    }

    #[test]
    fn test_unknown_terminal_error() {
        let result = generate_config(TerminalType::Unknown);
        assert!(result.is_err());
    }
}
