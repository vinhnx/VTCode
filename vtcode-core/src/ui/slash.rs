use once_cell::sync::Lazy;

/// Metadata describing a slash command supported by the chat interface.
#[derive(Clone, Copy, Debug)]
pub struct SlashCommandInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// Collection of slash command definitions in the order they should be displayed.
pub static SLASH_COMMANDS: Lazy<Vec<SlashCommandInfo>> = Lazy::new(|| {
    vec![
        SlashCommandInfo {
            name: "init",
            description: "Create vtcode.toml and index the workspace (usage: /init [--force])",
        },
        SlashCommandInfo {
            name: "add-dir",
            description: "Link an additional project directory (usage: /add-dir <path> [...])",
        },
        SlashCommandInfo {
            name: "config",
            description: "View the effective vtcode.toml configuration",
        },
        SlashCommandInfo {
            name: "model",
            description: "Launch the interactive model picker",
        },
        SlashCommandInfo {
            name: "theme",
            description: "Switch UI theme (usage: /theme <theme-id>)",
        },
        SlashCommandInfo {
            name: "list-themes",
            description: "List all available UI themes",
        },
        SlashCommandInfo {
            name: "command",
            description: "Run a terminal command (usage: /command <program> [args...])",
        },
        SlashCommandInfo {
            name: "clear",
            description: "Clear the current conversation history",
        },
        SlashCommandInfo {
            name: "doctor",
            description: "Run installation and configuration diagnostics",
        },
        SlashCommandInfo {
            name: "status",
            description: "Show model, provider, workspace, and tool status",
        },
        SlashCommandInfo {
            name: "cost",
            description: "Display token usage and remaining budget",
        },
        SlashCommandInfo {
            name: "mcp",
            description: "Show MCP connection status and management guidance",
        },
        SlashCommandInfo {
            name: "sessions",
            description: "List recent archived sessions (usage: /sessions [limit])",
        },
        SlashCommandInfo {
            name: "help",
            description: "Show slash command help",
        },
        SlashCommandInfo {
            name: "exit",
            description: "Exit the session",
        },
    ]
});

/// Returns slash command metadata that match the provided prefix (case insensitive).
pub fn suggestions_for(prefix: &str) -> Vec<&'static SlashCommandInfo> {
    if prefix.is_empty() {
        return SLASH_COMMANDS.iter().collect();
    }
    let query = prefix.to_ascii_lowercase();
    let mut matches: Vec<&SlashCommandInfo> = SLASH_COMMANDS
        .iter()
        .filter(|info| info.name.starts_with(&query))
        .collect();
    if matches.is_empty() {
        SLASH_COMMANDS.iter().collect()
    } else {
        matches.sort_by(|a, b| a.name.cmp(b.name));
        matches
    }
}
