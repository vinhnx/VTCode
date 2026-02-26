//! Shell integration feature configuration generator.
//!
//! Generates terminal-specific configuration for shell integration:
//! - Working directory tracking (OSC 7 sequences)
//! - Command status tracking (exit codes)
//! - Prompt integration
//! - Command duration tracking

use crate::terminal_setup::detector::TerminalType;
use anyhow::Result;

/// Generate shell integration configuration for the specified terminal
pub fn generate_config(terminal: TerminalType) -> Result<String> {
    let config = match terminal {
        TerminalType::Ghostty => r#"# Shell integration
shell-integration = detect
shell-integration-features = cursor,sudo,title

# Working directory tracking
working-directory = inherit
"#
        .to_string(),

        TerminalType::Kitty => r#"# Shell integration
shell_integration enabled

# Features
shell_integration_features title cwd

# Automatically inject shell integration
shell_integration_startup_mode enabled
"#
        .to_string(),

        TerminalType::Alacritty => r#"# Shell integration via OSC sequences
# Add this to your shell RC file (~/.bashrc, ~/.zshrc, etc.):
#
# For Bash:
# if [ -n "$ALACRITTY_SOCKET" ]; then
#     PROMPT_COMMAND='printf "\033]7;file://%s%s\033\\" "$HOSTNAME" "$PWD"'
# fi
#
# For Zsh:
# if [ -n "$ALACRITTY_SOCKET" ]; then
#     precmd() { printf "\033]7;file://%s%s\033\\" "$HOST" "$PWD"; }
# fi
#
# For Fish:
# if set -q ALACRITTY_SOCKET
#     function __alacritty_osc7_helper --on-variable PWD
#         printf "\033]7;file://%s%s\033\\" $hostname $PWD
#     end
# end

# Note: Alacritty doesn't have built-in shell integration
# The configuration above enables working directory tracking
"#
        .to_string(),

        TerminalType::WezTerm => {
            r#"-- WezTerm shell integration is available via shell integration scripts.
-- See: https://wezfurlong.org/wezterm/shell-integration.html
"#
            .to_string()
        }

        TerminalType::TerminalApp => {
            r#"Terminal.app supports shell integration through shell startup files.
Use OSC 7 sequences in your prompt hooks for directory tracking.
"#
            .to_string()
        }

        TerminalType::Xterm => {
            r#"xterm shell integration relies on OSC escape sequences in shell prompt hooks.
"#
            .to_string()
        }

        TerminalType::Zed => r#"// Zed terminal has built-in shell integration
// Working directory and command tracking enabled by default
{
  "terminal": {
    "shell": {
      "with_arguments": {
        "program": "zsh",
        "args": ["-l"]
      }
    },
    "working_directory": "current_project_directory"
  }
}
"#
        .to_string(),

        TerminalType::Warp => r#"# Warp has advanced built-in shell integration
# Features automatically enabled:
# - Working directory tracking
# - Command history
# - Command status (success/failure)
# - Git status integration
# - AI command suggestions

# No additional configuration needed
# Shell integration is automatic
"#
        .to_string(),

        TerminalType::WindowsTerminal => r#"{
  "profiles": {
    "defaults": {
      "startingDirectory": "%USERPROFILE%"
    }
  },
  "experimental.rendering.forceFullRepaint": true
}

// Note: Windows Terminal supports shell integration via:
// 1. PowerShell: Built-in PSReadLine module
// 2. WSL: Add OSC sequences to .bashrc/.zshrc
// 3. Git Bash: Configure prompt in .bash_profile
"#
        .to_string(),

        TerminalType::Hyper => r#"// Shell integration for Hyper
// Install hyper-statusline plugin for enhanced integration

config: {
  // Working directory in tab title
  showWorkingDirectory: true,

  // Command execution feedback
  showCommandFeedback: true,
}

// Install recommended plugins:
// hyper install hyper-statusline
// hyper install hyper-search
"#
        .to_string(),

        TerminalType::Tabby => r#"terminal:
  shellIntegration: true
  workingDirectory: auto

  # Shell-specific integration
  shell:
    command: auto  # Auto-detect shell
    args: ['-l']   # Login shell

  # Command tracking
  trackCommands: true
  showCommandStatus: true
"#
        .to_string(),

        TerminalType::ITerm2 => r#"Manual iTerm2 Shell Integration Setup:

METHOD 1: Automatic Installation (Recommended)
1. Open iTerm2
2. Go to iTerm2 → Install Shell Integration
3. Select your shell (bash/zsh/fish)
4. Restart your terminal

METHOD 2: Manual Installation
1. Download: curl -L https://iterm2.com/shell_integration/install_shell_integration.sh | bash
2. Restart your shell
3. Verify installation: echo $ITERM_SESSION_ID

Features enabled:
- Working directory tracking
- Command history sync
- Command status badges
- Shell prompt marks
- Automatic profile switching

Configuration in Preferences:
1. Profiles → General → Working Directory
2. Set to "Reuse previous session's directory"
3. Profiles → Terminal → Enable "Shell Integration"
"#
        .to_string(),

        TerminalType::VSCode => r#"VS Code Shell Integration Configuration:

Shell integration is built-in and enabled by default.

To configure in settings.json:
{
  "terminal.integrated.shellIntegration.enabled": true,
  "terminal.integrated.shellIntegration.decorationsEnabled": "both",
  "terminal.integrated.shellIntegration.history": 100,
  "terminal.integrated.enablePersistentSessions": true,
  "terminal.integrated.cwd": "${workspaceFolder}"
}

Features:
- Automatic working directory detection
- Command navigation (Ctrl+Up/Down)
- Command status decorations
- Re-run command in new terminal
- Sticky scroll for command output

Keyboard shortcuts:
- Ctrl+Up/Down: Navigate between commands
- Ctrl+Shift+G: Go to recent directory
"#
        .to_string(),

        TerminalType::Unknown => {
            anyhow::bail!("Cannot generate shell integration config for unknown terminal type");
        }
    };

    Ok(config)
}

/// Generate shell RC file snippet for manual integration
pub fn generate_shell_rc_snippet(shell: &str) -> Result<String> {
    let snippet = match shell {
        "bash" => {
            r#"# VT Code Shell Integration for Bash
if [ -n "$VTCODE_SESSION" ]; then
    # Working directory tracking via OSC 7
    __vtcode_osc7() {
        printf '\033]7;file://%s%s\033\\' "$HOSTNAME" "$PWD"
    }

    # Add to PROMPT_COMMAND
    if [[ "$PROMPT_COMMAND" != *__vtcode_osc7* ]]; then
        PROMPT_COMMAND="__vtcode_osc7${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
    fi

    # Command status tracking
    __vtcode_command_status() {
        local status=$?
        printf '\033]133;D;%s\033\\' "$status"
        return $status
    }

    trap '__vtcode_command_status' DEBUG
fi
"#
        }

        "zsh" => {
            r#"# VT Code Shell Integration for Zsh
if [ -n "$VTCODE_SESSION" ]; then
    # Working directory tracking via OSC 7
    __vtcode_osc7() {
        printf '\033]7;file://%s%s\033\\' "$HOST" "$PWD"
    }

    # Add to precmd hook
    if ! (( ${precmd_functions[(I)__vtcode_osc7]} )); then
        precmd_functions+=(__vtcode_osc7)
    fi

    # Command status tracking
    __vtcode_command_status() {
        printf '\033]133;D;%s\033\\' "$?"
    }

    if ! (( ${preexec_functions[(I)__vtcode_command_status]} )); then
        preexec_functions+=(__vtcode_command_status)
    fi
fi
"#
        }

        "fish" => {
            r#"# VT Code Shell Integration for Fish
if set -q VTCODE_SESSION
    # Working directory tracking via OSC 7
    function __vtcode_osc7_helper --on-variable PWD
        printf '\033]7;file://%s%s\033\\' $hostname $PWD
    end

    # Command status tracking
    function __vtcode_command_status --on-event fish_postexec
        printf '\033]133;D;%s\033\\' $status
    end

    __vtcode_osc7_helper
end
"#
        }

        _ => {
            anyhow::bail!("Unsupported shell type: {}", shell);
        }
    };

    Ok(snippet.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_ghostty_config() {
        let config = generate_config(TerminalType::Ghostty).unwrap();
        assert!(config.contains("shell-integration"));
        assert!(config.contains("working-directory"));
    }

    #[test]
    fn test_generate_kitty_config() {
        let config = generate_config(TerminalType::Kitty).unwrap();
        assert!(config.contains("shell_integration"));
        assert!(config.contains("cwd"));
    }

    #[test]
    fn test_generate_alacritty_config() {
        let config = generate_config(TerminalType::Alacritty).unwrap();
        assert!(config.contains("OSC"));
        assert!(config.contains("PROMPT_COMMAND") || config.contains("precmd"));
    }

    #[test]
    fn test_generate_vscode_config() {
        let config = generate_config(TerminalType::VSCode).unwrap();
        assert!(config.contains("shellIntegration"));
        assert!(config.contains("settings.json"));
    }

    #[test]
    fn test_generate_iterm2_instructions() {
        let config = generate_config(TerminalType::ITerm2).unwrap();
        assert!(config.contains("iTerm2"));
        assert!(config.contains("Install Shell Integration"));
    }

    #[test]
    fn test_generate_bash_snippet() {
        let snippet = generate_shell_rc_snippet("bash").unwrap();
        assert!(snippet.contains("PROMPT_COMMAND"));
        assert!(snippet.contains("__vtcode_osc7"));
    }

    #[test]
    fn test_generate_zsh_snippet() {
        let snippet = generate_shell_rc_snippet("zsh").unwrap();
        assert!(snippet.contains("precmd"));
        assert!(snippet.contains("__vtcode_osc7"));
    }

    #[test]
    fn test_generate_fish_snippet() {
        let snippet = generate_shell_rc_snippet("fish").unwrap();
        assert!(snippet.contains("on-variable PWD"));
        assert!(snippet.contains("__vtcode_osc7_helper"));
    }

    #[test]
    fn test_unknown_terminal_error() {
        let result = generate_config(TerminalType::Unknown);
        assert!(result.is_err());
    }

    #[test]
    fn test_unsupported_shell_error() {
        let result = generate_shell_rc_snippet("tcsh");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_config() {
        // This test exists for backward compatibility with the stub
        assert!(generate_config(TerminalType::Kitty).is_ok());
    }
}
