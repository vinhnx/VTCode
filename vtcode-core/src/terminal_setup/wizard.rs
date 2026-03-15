//! Interactive terminal setup wizard.
//!
//! Guides users through configuring their terminal emulator for VT Code.

use crate::VTCodeConfig;
use crate::utils::ansi::{AnsiRenderer, MessageStyle};
use crate::utils::file_utils::read_file_with_context_sync;
use anyhow::Result;

use super::backup::ConfigBackupManager;
use super::detector::{TerminalFeature, TerminalSetupAvailability, TerminalType};

/// Run the interactive terminal setup wizard
pub async fn run_terminal_setup_wizard(
    renderer: &mut AnsiRenderer,
    _config: &VTCodeConfig,
) -> Result<()> {
    // Step 1: Welcome and Detection
    display_welcome(renderer)?;

    let terminal_type = TerminalType::detect()?;

    renderer.line(
        MessageStyle::Status,
        &format!("Detected terminal: {}", terminal_type.name()),
    )?;

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

    match terminal_type.terminal_setup_availability() {
        TerminalSetupAvailability::NativeSupport => {
            render_guidance_messages(renderer, &native_terminal_setup_messages(terminal_type))?;
            return Ok(());
        }
        TerminalSetupAvailability::GuidanceOnly => {
            render_guidance_messages(renderer, &guidance_only_messages(terminal_type))?;
            return Ok(());
        }
        TerminalSetupAvailability::Offered => {}
    }

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
        TerminalType::Ghostty => unreachable!("native-support terminals return before config"),
        TerminalType::Kitty => unreachable!("native-support terminals return before config"),
        TerminalType::Alacritty => {
            crate::terminal_setup::terminals::alacritty::generate_config(&enabled_features)?
        }
        TerminalType::WezTerm => unreachable!("native-support terminals return before config"),
        TerminalType::TerminalApp => unreachable!("guidance-only terminals return before config"),
        TerminalType::Xterm => unreachable!("guidance-only terminals return before config"),
        TerminalType::Zed => {
            crate::terminal_setup::terminals::zed::generate_config(&enabled_features)?
        }
        TerminalType::Warp => unreachable!("native-support terminals return before config"),
        TerminalType::WindowsTerminal => {
            unreachable!("guidance-only terminals return before config")
        }
        TerminalType::Hyper => unreachable!("guidance-only terminals return before config"),
        TerminalType::Tabby => unreachable!("guidance-only terminals return before config"),
        TerminalType::ITerm2 => unreachable!("native-support terminals return before config"),
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
        TerminalType::Unknown => unreachable!("guidance-only terminals return before config"),
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
        "This wizard helps you verify or configure your terminal for VT Code.",
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

fn render_guidance_messages(renderer: &mut AnsiRenderer, messages: &[String]) -> Result<()> {
    renderer.line_if_not_empty(MessageStyle::Info)?;
    for line in messages {
        renderer.line(MessageStyle::Info, line)?;
    }
    Ok(())
}

fn native_terminal_setup_messages(terminal_type: TerminalType) -> Vec<String> {
    let mut lines = vec![
        format!(
            "{} already supports multiline input without VT Code editing your terminal config.",
            terminal_type.name()
        ),
        "Shift+Enter should work natively in this terminal.".to_string(),
    ];

    match terminal_type {
        TerminalType::ITerm2 => {
            lines.push(
                "Optional macOS shortcut: set Left/Right Option to \"Esc+\" in Profiles -> Keys."
                    .to_string(),
            );
            lines.extend(
                crate::terminal_setup::features::notifications::get_notification_instructions(
                    terminal_type,
                ),
            );
        }
        TerminalType::Ghostty | TerminalType::Kitty | TerminalType::WezTerm => {
            lines.extend(
                crate::terminal_setup::features::notifications::get_notification_instructions(
                    terminal_type,
                ),
            );
        }
        TerminalType::Warp => {
            lines.push(
                "Warp already provides multiline input and terminal notifications.".to_string(),
            );
        }
        _ => {}
    }

    lines
}

fn guidance_only_messages(terminal_type: TerminalType) -> Vec<String> {
    match terminal_type {
        TerminalType::TerminalApp => vec![
            "VT Code does not auto-configure Terminal.app.".to_string(),
            "Use Settings -> Profiles -> Keyboard and enable \"Use Option as Meta Key\" for Option+Enter workflows.".to_string(),
            "Configure notifications from Terminal -> Settings -> Profiles -> Advanced.".to_string(),
        ],
        TerminalType::Xterm => vec![
            "VT Code does not auto-configure xterm.".to_string(),
            "Configure Shift+Enter or newline shortcuts through X resources or your window manager.".to_string(),
            "Use your terminal bell settings if you want completion alerts.".to_string(),
        ],
        TerminalType::WindowsTerminal => vec![
            "VT Code does not currently advertise guided setup for Windows Terminal.".to_string(),
            "Configure Shift+Enter or multiline bindings in Windows Terminal settings if you need them.".to_string(),
            "Use the terminal bell or profile alert settings for notifications.".to_string(),
        ],
        TerminalType::Hyper => vec![
            "VT Code does not currently advertise guided setup for Hyper.".to_string(),
            "Configure multiline bindings or plugins directly in `.hyper.js`.".to_string(),
            "Use Hyper plugins or bell settings if you want notifications.".to_string(),
        ],
        TerminalType::Tabby => vec![
            "VT Code does not currently advertise guided setup for Tabby.".to_string(),
            "Configure multiline bindings in Tabby's terminal settings or config file.".to_string(),
            "Use Tabby's built-in notification or bell settings if needed.".to_string(),
        ],
        TerminalType::Unknown => vec![
            "Could not detect a supported terminal profile for automatic VT Code setup.".to_string(),
            "Use \\ + Enter for multiline input, or configure your terminal to send a newline on Shift+Enter.".to_string(),
            "On macOS, Option+Enter is often the simplest fallback once Option is configured as Meta.".to_string(),
        ],
        _ => vec![format!(
            "VT Code does not currently offer guided setup for {}.",
            terminal_type.name()
        )],
    }
}

#[cfg(test)]
mod tests {
    use super::{guidance_only_messages, native_terminal_setup_messages};
    use crate::terminal_setup::detector::TerminalType;

    #[test]
    fn test_wizard_module() {
        // Placeholder test - actual wizard tests would need mocked terminal I/O
    }

    #[test]
    fn native_setup_messages_are_noop_guidance() {
        let lines = native_terminal_setup_messages(TerminalType::WezTerm);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("already supports multiline"))
        );
        assert!(lines.iter().any(|line| line.contains("Shift+Enter")));
    }

    #[test]
    fn guidance_only_messages_cover_terminal_app() {
        let lines = guidance_only_messages(TerminalType::TerminalApp);
        assert!(
            lines
                .iter()
                .any(|line| line.contains("does not auto-configure"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Use Option as Meta Key"))
        );
    }
}
