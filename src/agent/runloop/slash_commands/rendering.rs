use std::path::Path;

use anyhow::Result;

use vtcode_core::prompts::find_prompt_template;
use vtcode_core::skills::find_command_skill_by_slash_name;
use vtcode_core::ui::slash::{SlashCommandInfo, find_command, visible_commands};
use vtcode_core::ui::theme;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_terminal_detection::TerminalType;

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

pub(super) fn render_loop_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Usage: /loop [interval] <prompt>  or  /loop <prompt> every <interval>",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Examples: /loop 5m check the deployment, /loop /review-pr 1234 every 20m",
    )?;
    Ok(())
}

pub(super) fn render_schedule_usage(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Usage: /schedule                      (open interactive manager)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "       /schedule list                 (browse tasks interactively)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "       /schedule create               (interactive create flow)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "       /schedule delete               (interactive delete picker)",
    )?;
    renderer.line(MessageStyle::Info, "       /schedule delete <task-id>")?;
    renderer.line(
        MessageStyle::Info,
        "       /schedule create --prompt <text>|--reminder <text> --every <dur>|--cron <expr>|--at <time> [--name <label>] [--workspace <path>]",
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

pub(super) async fn render_help(
    renderer: &mut AnsiRenderer,
    specific_command: Option<&str>,
    workspace: &Path,
) -> Result<()> {
    if let Some(cmd_name) = specific_command {
        if let Some(cmd) = resolve_help_command(cmd_name) {
            renderer.line(MessageStyle::Info, &format!("Help for /{}:", cmd.name))?;
            renderer.line(
                MessageStyle::Info,
                &format!("  Description: {}", cmd.description),
            )?;
            if let Some(spec) = find_command_skill_by_slash_name(cmd.name) {
                renderer.line(MessageStyle::Info, &format!("  Usage: {}", spec.usage))?;
            }
        } else if let Some(template) = find_prompt_template(workspace, cmd_name).await {
            renderer.line(MessageStyle::Info, &format!("Help for /{}:", template.name))?;
            renderer.line(
                MessageStyle::Info,
                &format!("  Description: {}", template.description),
            )?;
            renderer.line(
                MessageStyle::Info,
                "  Type `/name [args...]` to expand this prompt template into the editor.",
            )?;
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
        let prompt_templates = vtcode_core::prompts::discover_prompt_templates(workspace)
            .await
            .into_iter()
            .filter(|template| find_command(&template.name).is_none())
            .collect::<Vec<_>>();
        if !prompt_templates.is_empty() {
            renderer.line(MessageStyle::Info, "")?;
            renderer.line(MessageStyle::Info, "Prompt templates:")?;
            for template in prompt_templates {
                renderer.line(
                    MessageStyle::Info,
                    &format!("  /{} – {}", template.name, template.description),
                )?;
            }
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
            "  Shift+Tab – Open the Edit/Auto/Plan mode picker",
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

fn resolve_help_command(name: &str) -> Option<&'static SlashCommandInfo> {
    let trimmed = name.trim();
    find_command(trimmed).or_else(|| {
        let lower = trimmed.to_ascii_lowercase();
        let canonical = super::normalize_command_key(&lower);
        find_command(canonical)
    })
}

#[cfg(test)]
mod tests {
    use super::resolve_help_command;

    #[test]
    fn help_resolution_supports_aliases() {
        assert_eq!(
            resolve_help_command("settings").map(|command| command.name),
            Some("config")
        );
        assert_eq!(
            resolve_help_command("comman").map(|command| command.name),
            Some("command")
        );
    }

    #[test]
    fn help_resolution_does_not_keep_removed_share_log_alias() {
        assert!(resolve_help_command("share-log").is_none());
        assert!(resolve_help_command("sharelog").is_none());
        assert!(resolve_help_command("export-log").is_none());
    }

    #[test]
    fn help_resolution_is_case_insensitive() {
        assert_eq!(
            resolve_help_command("Review").map(|command| command.name),
            Some("review")
        );
    }
}
