use anyhow::Result;

use vtcode_core::terminal_setup::detector::TerminalType;
use vtcode_core::ui::slash::{find_command, visible_commands};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(super) fn render_mcp_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Usage: /mcp [status|list|tools|refresh|config|config edit|repair|diagnose|login <name>|logout <name>]",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  status  – Show overall MCP connection health",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  list    – List configured providers from vtcode.toml",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  tools   – Show tools exposed by active providers",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  refresh – Reindex MCP tools without restarting",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  config  – Summarize MCP settings from vtcode.toml",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  config edit – Show the config file path and editing guidance",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  repair  – Restart MCP connections and refresh tool indices",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  diagnose – Validate config and run MCP health checks",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  login/logout <name> – Manage provider authentication (if supported)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Examples: /mcp list, /mcp tools, /mcp login github",
    )?;
    Ok(())
}

pub(super) fn render_add_dir_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "Usage: /add-dir <path> [more paths...]")?;
    renderer.line(MessageStyle::Info, "       /add-dir --list")?;
    renderer.line(
        MessageStyle::Info,
        "       /add-dir --remove <alias|path> [more]",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Linked directories are mounted under .vtcode/external/.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Use quotes if your path contains spaces.",
    )?;
    Ok(())
}

pub(super) fn render_generate_agent_file_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "Usage: /generate-agent-file [--force]")?;
    renderer.line(
        MessageStyle::Info,
        "  --force  Overwrite an existing AGENTS.md with regenerated content.",
    )?;
    Ok(())
}

pub(super) fn render_theme_list(renderer: &mut AnsiRenderer) -> Result<()> {
    let available_themes = theme::available_themes();
    renderer.line(MessageStyle::Info, "Available themes:")?;

    for theme_id in available_themes {
        if let Some(label) = theme::theme_label(theme_id) {
            renderer.line(
                MessageStyle::Info,
                &format!("  /theme {} – {}", theme_id, label),
            )?;
        } else {
            renderer.line(
                MessageStyle::Info,
                &format!("  /theme {} – {}", theme_id, theme_id),
            )?;
        }
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(
        MessageStyle::Info,
        &format!("Current theme: {}", theme::active_theme_label()),
    )?;
    Ok(())
}

pub(super) fn render_help(
    renderer: &mut AnsiRenderer,
    specific_command: Option<&str>,
) -> Result<()> {
    if let Some(cmd_name) = specific_command {
        if let Some(cmd) = find_command(cmd_name) {
            renderer.line(MessageStyle::Info, &format!("Help for /{}:", cmd.name))?;
            renderer.line(
                MessageStyle::Info,
                &format!("  Description: {}", cmd.description),
            )?;
            // Additional usage examples could be added here in the future
        } else {
            renderer.line(
                MessageStyle::Error,
                &format!(
                    "Unknown command '{}'. Use /help without arguments to see all commands.",
                    cmd_name
                ),
            )?;
        }
    } else {
        // Show all commands
        renderer.line(MessageStyle::Info, "Available slash commands:")?;
        for cmd in visible_commands() {
            renderer.line(
                MessageStyle::Info,
                &format!("  /{} – {}", cmd.name, cmd.description),
            )?;
        }

        // Add information about interactive features
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Interactive mode features:")?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+C – Cancel current input or generation",
        )?;
        renderer.line(MessageStyle::Info, "  Ctrl+D – Exit VT Code session")?;
        renderer.line(MessageStyle::Info, "  Ctrl+L – Clear screen (keep context)")?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+R – Reverse search command history",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  /history – Open command history picker",
        )?;
        renderer.line(MessageStyle::Info, "  Ctrl+V – Paste image from clipboard")?;
        renderer.line(
            MessageStyle::Info,
            "  Up/Down arrows – Navigate command history",
        )?;
        renderer.line(MessageStyle::Info, "  Esc+Esc – Open the rewind picker")?;
        renderer.line(
            MessageStyle::Info,
            "  Enter – Submit now (or queue if a turn is active)",
        )?;
        renderer.line(MessageStyle::Info, "  Tab – Queue the current input")?;
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+Enter – Run now / steer the active turn",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Shift+Tab – Cycle Edit, Trusted Auto, and Plan modes",
        )?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Multiline input:")?;
        renderer.line(
            MessageStyle::Info,
            "  \\ + Enter – Quick escape (insert newline without submitting)",
        )?;
        renderer.line(
            MessageStyle::Info,
            "  Shift+Enter – Multiline input (if configured)",
        )?;
        match TerminalType::detect().unwrap_or(TerminalType::Unknown) {
            TerminalType::Ghostty
            | TerminalType::Kitty
            | TerminalType::WezTerm
            | TerminalType::ITerm2
            | TerminalType::Warp => {
                renderer.line(
                    MessageStyle::Info,
                    "  Native support – Shift+Enter works in this terminal without /terminal-setup",
                )?;
            }
            term if term.should_offer_terminal_setup() => {
                renderer.line(
                    MessageStyle::Info,
                    "  /terminal-setup – Install VT Code multiline bindings for this terminal",
                )?;
            }
            _ => {
                renderer.line(
                    MessageStyle::Info,
                    "  Terminal-specific setup – Use Option+Enter on macOS or configure your terminal manually",
                )?;
            }
        }
        renderer.line(
            MessageStyle::Info,
            "  Ctrl+J – Line feed character for multiline",
        )?;
        renderer.line(MessageStyle::Info, "")?;
        renderer.line(MessageStyle::Info, "Shell mode:")?;
        renderer.line(
            MessageStyle::Info,
            "  !command – Run shell commands directly (e.g., !ls -la)",
        )?;
    }
    Ok(())
}
