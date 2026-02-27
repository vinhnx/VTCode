use once_cell::sync::Lazy;

use crate::ui::search::{fuzzy_match, normalize_query};

/// Metadata describing a slash command supported by the chat interface.
#[derive(Clone, Copy, Debug)]
pub struct SlashCommandInfo {
    pub name: &'static str,
    pub description: &'static str,
}

/// Collection of slash command definitions in the order they should be displayed.
pub static SLASH_COMMANDS: Lazy<Vec<SlashCommandInfo>> = Lazy::new(|| {
    vec![
        // Workspace setup
        SlashCommandInfo {
            name: "init",
            description: "Create vtcode.toml and index the workspace (usage: /init [--force])",
        },
        SlashCommandInfo {
            name: "generate-agent-file",
            description: "Generate AGENTS.md for the workspace (usage: /generate-agent-file [--force])",
        },
        SlashCommandInfo {
            name: "add-dir",
            description: "Link external directories or manage links (usage: /add-dir <path>|--list|--remove)",
        },
        // Configuration and settings
        SlashCommandInfo {
            name: "config",
            description: "View the effective vtcode.toml configuration",
        },
        SlashCommandInfo {
            name: "settings",
            description: "View the effective vtcode.toml configuration (alias for /config)",
        },
        SlashCommandInfo {
            name: "model",
            description: "Launch the interactive model picker",
        },
        SlashCommandInfo {
            name: "theme",
            description: "Switch UI theme (usage: /theme <theme-id>)",
        },
        // Tools and utilities
        SlashCommandInfo {
            name: "command",
            description: "Run a terminal command (usage: /command <program> [args...])",
        },
        SlashCommandInfo {
            name: "edit",
            description: "Open file in preferred editor from EDITOR/VISUAL env vars (usage: /edit [file])",
        },
        SlashCommandInfo {
            name: "git",
            description: "Launch git interface (lazygit or interactive git)",
        },
        SlashCommandInfo {
            name: "debug",
            description: "Analyze and debug code issues in the workspace (usage: /debug [file|directory|problem])",
        },
        SlashCommandInfo {
            name: "analyze",
            description: "Perform comprehensive codebase analysis and generate reports (usage: /analyze [full|security|performance])",
        },
        SlashCommandInfo {
            name: "files",
            description: "Browse and select files from workspace (usage: /files [filter])",
        },
        SlashCommandInfo {
            name: "copy",
            description: "Copy the latest complete assistant reply to clipboard",
        },
        SlashCommandInfo {
            name: "skills",
            description: "Manage skills and plugins (usage: /skills list|load|unload|use)",
        },
        SlashCommandInfo {
            name: "agents",
            description: "Create, edit, and manage subagents (usage: /agents [--create|--edit|--delete])",
        },
        SlashCommandInfo {
            name: "team",
            description: "Manage agent teams (usage: /team start|task|assign|message|teammates|stop)",
        },
        SlashCommandInfo {
            name: "subagent",
            description: "Configure subagent defaults (usage: /subagent model)",
        },
        // Status and diagnostics
        SlashCommandInfo {
            name: "status",
            description: "Show model, provider, workspace, and tool status",
        },
        SlashCommandInfo {
            name: "doctor",
            description: "Run installation and configuration diagnostics",
        },
        // Integrations
        SlashCommandInfo {
            name: "mcp",
            description: "Manage MCP providers (/mcp status|list|tools|config|repair|diagnose)",
        },
        // Session management
        SlashCommandInfo {
            name: "sessions",
            description: "List recent archived sessions (usage: /sessions [limit])",
        },
        SlashCommandInfo {
            name: "clear",
            description: "Clear visible screen (usage: /clear [new])",
        },
        SlashCommandInfo {
            name: "new",
            description: "Start a new session",
        },
        SlashCommandInfo {
            name: "share-log",
            description: "Export current session log as JSON or Markdown (usage: /share-log [json|markdown], alias: /export-log)",
        },
        SlashCommandInfo {
            name: "rewind",
            description: "Rewind to a previous checkpoint (usage: /rewind [turn] or /rewind [conversation|code|both])",
        },
        SlashCommandInfo {
            name: "plan",
            description: "Plan Mode: read-only planning with optional prompt (usage: /plan [on|off] [task])",
        },
        SlashCommandInfo {
            name: "agent",
            description: "Toggle Agent Mode: autonomous with reduced confirmation prompts (usage: /agent [on|off])",
        },
        SlashCommandInfo {
            name: "mode",
            description: "Cycle through Edit -> Plan -> Agent modes",
        },
        SlashCommandInfo {
            name: "docs",
            description: "Open vtcode documentation in web browser",
        },
        SlashCommandInfo {
            name: "help",
            description: "Show slash command help",
        },
        SlashCommandInfo {
            name: "exit",
            description: "Exit the session",
        },
        // Support
        SlashCommandInfo {
            name: "donate",
            description: "Support the project by buying the author a coffee",
        },
        // Terminal setup
        SlashCommandInfo {
            name: "terminal-setup",
            description: "Configure terminal for VT Code (multiline, copy/paste, shell, themes)",
        },
        // OAuth authentication
        SlashCommandInfo {
            name: "login",
            description: "Authenticate with OpenRouter via OAuth (usage: /login openrouter)",
        },
        SlashCommandInfo {
            name: "logout",
            description: "Clear OAuth authentication (usage: /logout openrouter)",
        },
        SlashCommandInfo {
            name: "auth",
            description: "Show authentication status for providers (usage: /auth [provider])",
        },
    ]
});

/// Returns slash command metadata that match the provided prefix (case insensitive).
pub fn suggestions_for(prefix: &str) -> Vec<&'static SlashCommandInfo> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return SLASH_COMMANDS.iter().collect();
    }

    let query = trimmed.to_ascii_lowercase();

    let mut prefix_matches: Vec<&SlashCommandInfo> = SLASH_COMMANDS
        .iter()
        .filter(|info| info.name.starts_with(&query))
        .collect();

    if !prefix_matches.is_empty() {
        prefix_matches.sort_by(|a, b| a.name.cmp(b.name));
        return prefix_matches;
    }

    let mut substring_matches: Vec<(&SlashCommandInfo, usize)> = SLASH_COMMANDS
        .iter()
        .filter_map(|info| info.name.find(&query).map(|position| (info, position)))
        .collect();

    if !substring_matches.is_empty() {
        substring_matches.sort_by(|(a, pos_a), (b, pos_b)| {
            (*pos_a, a.name.len(), a.name).cmp(&(*pos_b, b.name.len(), b.name))
        });
        return substring_matches
            .into_iter()
            .map(|(info, _)| info)
            .collect();
    }

    let normalized_query = normalize_query(&query);
    if normalized_query.is_empty() {
        return SLASH_COMMANDS.iter().collect();
    }

    let mut scored: Vec<(&SlashCommandInfo, usize, usize)> = SLASH_COMMANDS
        .iter()
        .filter_map(|info| {
            let mut candidate = info.name.to_ascii_lowercase();
            if !info.description.is_empty() {
                candidate.push(' ');
                candidate.push_str(&info.description.to_ascii_lowercase());
            }

            if !fuzzy_match(&normalized_query, &candidate) {
                return None;
            }

            let name_pos = info
                .name
                .to_ascii_lowercase()
                .find(&query)
                .unwrap_or(usize::MAX);
            let desc_pos = info
                .description
                .to_ascii_lowercase()
                .find(&query)
                .unwrap_or(usize::MAX);

            Some((info, name_pos, desc_pos))
        })
        .collect();

    if scored.is_empty() {
        return SLASH_COMMANDS.iter().collect();
    }

    scored.sort_by(|(a, name_pos_a, desc_pos_a), (b, name_pos_b, desc_pos_b)| {
        let score_a = (*name_pos_a == usize::MAX, *name_pos_a, *desc_pos_a, a.name);
        let score_b = (*name_pos_b == usize::MAX, *name_pos_b, *desc_pos_b, b.name);
        score_a.cmp(&score_b)
    });

    scored.into_iter().map(|(info, _, _)| info).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_for(prefix: &str) -> Vec<&'static str> {
        suggestions_for(prefix)
            .into_iter()
            .map(|info| info.name)
            .collect()
    }

    #[test]
    fn prefix_matches_are_sorted_alphabetically() {
        let names = names_for("c");
        assert_eq!(names, vec!["clear", "command", "config", "context", "copy"]);
    }

    #[test]
    fn substring_matches_prioritize_earlier_occurrences() {
        let names = names_for("eme");
        assert_eq!(names, vec!["theme"]);
    }

    #[test]
    fn fuzzy_matches_include_description_keywords() {
        let names = names_for("diagnostic");
        assert!(names.contains(&"doctor"));
    }

    #[test]
    fn fuzzy_matches_handle_non_contiguous_sequences() {
        let names = names_for("sts");
        assert!(names.contains(&"status"));
    }
}
