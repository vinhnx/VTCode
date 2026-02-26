//! Interactive terminal setup wizard.
//!
//! Guides users through configuring their terminal emulator for VT Code.

use crate::VTCodeConfig;
use crate::utils::ansi::{AnsiRenderer, MessageStyle};
use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::Result;

use super::backup::ConfigBackupManager;
use super::detector::{TerminalFeature, TerminalType};

/// Run the interactive terminal setup wizard
pub async fn run_terminal_setup_wizard(
    renderer: &mut AnsiRenderer,
    _config: &VTCodeConfig,
) -> Result<()> {
    // Step 1: Welcome and Detection
    display_welcome(renderer)?;

    let terminal_type = TerminalType::detect()?;

    if terminal_type == TerminalType::Unknown {
        renderer.line(
            MessageStyle::Error,
            "Could not detect your terminal emulator.",
        )?;
        renderer.line(MessageStyle::Info, "Supported terminals: Ghostty, Kitty, Alacritty, Zed, Warp, iTerm2, VS Code, Windows Terminal, Hyper, Tabby")?;
        return Ok(());
    }

    renderer.line(
        MessageStyle::Status,
        &format!("Detected terminal: {}", terminal_type.name()),
    )?;

    // Get config path
    let config_path = match terminal_type.config_path() {
        Ok(path) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Config file: {}", path.display()),
            )?;
            path
        }
        Err(e) => {
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to determine config path: {}", e),
            )?;
            return Ok(());
        }
    };

    // Step 2: Feature Selection (for now, show what will be configured)
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(MessageStyle::Info, "Features to configure:")?;

    let features = vec![
        TerminalFeature::Multiline,
        TerminalFeature::CopyPaste,
        TerminalFeature::ShellIntegration,
        TerminalFeature::ThemeSync,
        TerminalFeature::Notifications,
    ];

    for feature in &features {
        let supported = terminal_type.supports_feature(*feature);
        let status = if supported {
            "✓"
        } else {
            "✗ (not supported)"
        };
        renderer.line(
            if supported {
                MessageStyle::Status
            } else {
                MessageStyle::Info
            },
            &format!("  {} {}", status, feature.name()),
        )?;
    }

    // Check if terminal requires manual setup
    if terminal_type.requires_manual_setup() {
        renderer.line_if_not_empty(MessageStyle::Info)?;
        renderer.line(
            MessageStyle::Info,
            &format!(
                "{} requires manual configuration. Instructions will be provided.",
                terminal_type.name()
            ),
        )?;

        // For now, just show a placeholder message
        renderer.line_if_not_empty(MessageStyle::Info)?;
        renderer.line(
            MessageStyle::Info,
            "Manual configuration instructions will be generated in a future update.",
        )?;

        return Ok(());
    }

    // Step 3: Backup existing config
    renderer.line_if_not_empty(MessageStyle::Info)?;

    if config_path.exists() {
        renderer.line(
            MessageStyle::Info,
            &format!("Creating backup of {}...", config_path.display()),
        )?;

        let backup_manager = ConfigBackupManager::new(terminal_type);
        match backup_manager.backup_config(&config_path) {
            Ok(backup_path) => {
                renderer.line(
                    MessageStyle::Status,
                    &format!("  → Backup created: {}", backup_path.display()),
                )?;
            }
            Err(e) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to create backup: {}", e),
                )?;
                return Ok(());
            }
        }
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!("Config file does not exist yet: {}", config_path.display()),
        )?;
        renderer.line(MessageStyle::Info, "A new config file will be created.")?;
    }

    // Step 4: Generate and apply configuration
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(MessageStyle::Info, "Generating configuration...")?;

    // Collect enabled features
    let enabled_features: Vec<TerminalFeature> = features
        .iter()
        .filter(|f| terminal_type.supports_feature(**f))
        .copied()
        .collect();

    // Generate terminal-specific configuration
    let new_config = match terminal_type {
        TerminalType::Ghostty => {
            crate::terminal_setup::terminals::ghostty::generate_config(&enabled_features)?
        }
        TerminalType::Kitty => {
            crate::terminal_setup::terminals::kitty::generate_config(&enabled_features)?
        }
        TerminalType::Alacritty => {
            crate::terminal_setup::terminals::alacritty::generate_config(&enabled_features)?
        }
        TerminalType::WezTerm => {
            renderer.line_if_not_empty(MessageStyle::Info)?;
            renderer.line(
                MessageStyle::Info,
                "WezTerm detected. Apply settings in ~/.wezterm.lua (manual setup).",
            )?;
            for line in
                crate::terminal_setup::features::notifications::get_notification_instructions(
                    terminal_type,
                )
            {
                renderer.line(MessageStyle::Info, &line)?;
            }
            return Ok(());
        }
        TerminalType::TerminalApp => {
            renderer.line_if_not_empty(MessageStyle::Info)?;
            renderer.line(
                MessageStyle::Info,
                "Terminal.app detected. Configure profile settings manually.",
            )?;
            for line in
                crate::terminal_setup::features::notifications::get_notification_instructions(
                    terminal_type,
                )
            {
                renderer.line(MessageStyle::Info, &line)?;
            }
            return Ok(());
        }
        TerminalType::Xterm => {
            renderer.line_if_not_empty(MessageStyle::Info)?;
            renderer.line(
                MessageStyle::Info,
                "xterm detected. Configure via X resources manually.",
            )?;
            for line in
                crate::terminal_setup::features::notifications::get_notification_instructions(
                    terminal_type,
                )
            {
                renderer.line(MessageStyle::Info, &line)?;
            }
            return Ok(());
        }
        TerminalType::Zed => {
            crate::terminal_setup::terminals::zed::generate_config(&enabled_features)?
        }
        TerminalType::Warp => {
            crate::terminal_setup::terminals::warp::generate_config(&enabled_features)?
        }
        TerminalType::WindowsTerminal => {
            crate::terminal_setup::terminals::windows_terminal::generate_config(&enabled_features)?
        }
        TerminalType::Hyper => {
            crate::terminal_setup::terminals::hyper::generate_config(&enabled_features)?
        }
        TerminalType::Tabby => {
            crate::terminal_setup::terminals::tabby::generate_config(&enabled_features)?
        }
        TerminalType::ITerm2 => {
            // iTerm2 requires manual setup - display instructions
            let instructions =
                crate::terminal_setup::terminals::iterm2::generate_config(&enabled_features)?;
            renderer.line_if_not_empty(MessageStyle::Info)?;
            for line in instructions.lines() {
                renderer.line(MessageStyle::Info, line)?;
            }
            return Ok(());
        }
        TerminalType::VSCode => {
            // VS Code requires manual setup - display instructions
            let instructions =
                crate::terminal_setup::terminals::vscode::generate_config(&enabled_features)?;
            renderer.line_if_not_empty(MessageStyle::Info)?;
            for line in instructions.lines() {
                renderer.line(MessageStyle::Info, line)?;
            }
            return Ok(());
        }
        TerminalType::Unknown => {
            renderer.line(
                MessageStyle::Error,
                "Cannot generate configuration for unknown terminal.",
            )?;
            return Ok(());
        }
    };

    // Read existing config if it exists
    let existing_content = if config_path.exists() {
        read_file_with_context_sync(&config_path, "terminal config file")?
    } else {
        String::new()
    };

    // Merge with existing configuration
    use crate::terminal_setup::config_writer::ConfigWriter;
    let format = ConfigWriter::detect_format(&config_path);
    let merged_config = ConfigWriter::merge_with_markers(&existing_content, &new_config, format)?;

    // Write the configuration
    ConfigWriter::write_atomic(&config_path, &merged_config)?;

    renderer.line(
        MessageStyle::Status,
        &format!("✓ Configuration written to {}", config_path.display()),
    )?;

    // Step 5: Show completion message
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(
        MessageStyle::Status,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line(MessageStyle::Status, "  Setup Complete!")?;
    renderer.line(
        MessageStyle::Status,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(
        MessageStyle::Info,
        "Restart your terminal for changes to take effect.",
    )?;

    if config_path.exists() {
        let backup_manager = ConfigBackupManager::new(terminal_type);
        let backups = backup_manager.list_backups(&config_path)?;
        if let Some(latest_backup) = backups.first() {
            renderer.line_if_not_empty(MessageStyle::Info)?;
            renderer.line(
                MessageStyle::Info,
                &format!("Backup saved to: {}", latest_backup.display()),
            )?;
            renderer.line(
                MessageStyle::Info,
                "To restore: Run /terminal-setup --restore (coming soon)",
            )?;
        }
    }

    Ok(())
}

/// Display welcome message
fn display_welcome(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line(MessageStyle::Info, "  VT Code Terminal Setup Wizard")?;
    renderer.line(
        MessageStyle::Info,
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
    )?;
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(
        MessageStyle::Info,
        "This wizard will configure your terminal for optimal VT Code experience.",
    )?;
    renderer.line_if_not_empty(MessageStyle::Info)?;
    renderer.line(MessageStyle::Info, "Features:")?;
    renderer.line(MessageStyle::Info, "  • Shift+Enter for multiline input")?;
    renderer.line(MessageStyle::Info, "  • Enhanced copy/paste integration")?;
    renderer.line(
        MessageStyle::Info,
        "  • Shell integration (working directory, command status)",
    )?;
    renderer.line(MessageStyle::Info, "  • Theme synchronization")?;
    renderer.line_if_not_empty(MessageStyle::Info)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_wizard_module() {
        // Placeholder test - actual wizard tests would need mocked terminal I/O
    }
}
