use once_cell::sync::Lazy;

use crate::terminal_setup::detector::TerminalType;
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
            description: "Browse settings sections in vtcode.toml",
        },
        SlashCommandInfo {
            name: "permissions",
            description: "Open the permissions settings section and effective summary",
        },
        SlashCommandInfo {
            name: "vim",
            description: "Toggle Vim-style prompt editing (usage: /vim [on|off|toggle])",
        },
        SlashCommandInfo {
            name: "model",
            description: "Launch the interactive model picker",
        },
        SlashCommandInfo {
            name: "ide",
            description: "Toggle IDE context for this session",
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
            description: "Open file in external editor (tools.editor config, then VISUAL/EDITOR) (usage: /edit [file])",
        },
        SlashCommandInfo {
            name: "git",
            description: "Launch git interface (lazygit or interactive git)",
        },
        SlashCommandInfo {
            name: "analyze",
            description: "Perform comprehensive codebase analysis and generate reports (usage: /analyze [full|security|performance])",
        },
        SlashCommandInfo {
            name: "review",
            description: "Review the current diff or selected files (usage: /review [--last-diff|--target <expr>|--file <path>|files...] [--style <style>])",
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
            name: "suggest",
            description: "Suggest follow-up prompts from the current session context",
        },
        SlashCommandInfo {
            name: "tasks",
            description: "Toggle the dedicated TODO panel fed by task_tracker output",
        },
        SlashCommandInfo {
            name: "jobs",
            description: "Inspect active/background command sessions",
        },
        SlashCommandInfo {
            name: "skills",
            description: "Open interactive skills manager (usage: /skills, /skills manager)",
        },
        SlashCommandInfo {
            name: "agents",
            description: "Manage subagents and delegated child threads (usage: /agents [list|create|edit|delete|threads])",
        },
        SlashCommandInfo {
            name: "agent",
            description: "Show delegated child threads for the current session",
        },
        // Status and diagnostics
        SlashCommandInfo {
            name: "status",
            description: "Show model, provider, workspace, and tool status",
        },
        SlashCommandInfo {
            name: "stop",
            description: "Stop the active turn immediately",
        },
        SlashCommandInfo {
            name: "pause",
            description: "Pause the active turn at the next safe boundary",
        },
        SlashCommandInfo {
            name: "doctor",
            description: "Run installation and configuration diagnostics (interactive in inline UI; usage: /doctor [--quick|--full])",
        },
        SlashCommandInfo {
            name: "update",
            description: "Check for new VT Code releases and install updates (usage: /update [check|install] [--force])",
        },
        // Integrations
        SlashCommandInfo {
            name: "mcp",
            description: "Open interactive MCP manager (usage: /mcp, optional subcommands still supported)",
        },
        // Session management
        SlashCommandInfo {
            name: "resume",
            description: "List archived sessions when idle; resume the active turn while it is paused",
        },
        SlashCommandInfo {
            name: "fork",
            description: "Fork an archived session into a new thread (usage: /fork [limit] [--all])",
        },
        SlashCommandInfo {
            name: "history",
            description: "Open command history picker (usage: /history, same as Ctrl+R)",
        },
        SlashCommandInfo {
            name: "clear",
            description: "Clear visible screen (usage: /clear [new])",
        },
        SlashCommandInfo {
            name: "compact",
            description: "Compact current conversation history using provider-native or local fallback compaction (usage: /compact)",
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
            description: "Open the rewind picker or restore a specific checkpoint (usage: /rewind [turn] [conversation|code|both])",
        },
        SlashCommandInfo {
            name: "plan",
            description: "Plan Mode: read-only planning with optional prompt (usage: /plan [on|off] [task])",
        },
        SlashCommandInfo {
            name: "mode",
            description: "Open the session mode picker or switch directly (usage: /mode [edit|auto|plan|cycle])",
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
        SlashCommandInfo {
            name: "statusline",
            description: "Set up a custom status line with target selection (usage: /statusline [instructions...])",
        },
        // Provider authentication
        SlashCommandInfo {
            name: "login",
            description: "Authenticate with OpenAI, OpenRouter, or GitHub Copilot (usage: /login [provider])",
        },
        SlashCommandInfo {
            name: "logout",
            description: "Clear stored provider authentication (usage: /logout [provider])",
        },
        SlashCommandInfo {
            name: "auth",
            description: "Show authentication status for providers (usage: /auth [provider])",
        },
        SlashCommandInfo {
            name: "refresh-oauth",
            description: "Refresh stored provider credentials when supported (usage: /refresh-oauth [provider])",
        },
    ]
});

fn detected_terminal_for_visibility() -> TerminalType {
    TerminalType::detect().unwrap_or(TerminalType::Unknown)
}

fn command_visible_for_terminal(command: &SlashCommandInfo, terminal: TerminalType) -> bool {
    command.name != "terminal-setup" || terminal.should_offer_terminal_setup()
}

pub fn visible_commands() -> Vec<&'static SlashCommandInfo> {
    visible_commands_for_terminal(detected_terminal_for_visibility())
}

pub fn visible_commands_for_terminal(terminal: TerminalType) -> Vec<&'static SlashCommandInfo> {
    SLASH_COMMANDS
        .iter()
        .filter(|command| command_visible_for_terminal(command, terminal))
        .collect()
}

pub fn find_visible_command(name: &str) -> Option<&'static SlashCommandInfo> {
    let terminal = detected_terminal_for_visibility();
    SLASH_COMMANDS
        .iter()
        .find(|command| command.name == name && command_visible_for_terminal(command, terminal))
}

pub fn find_command(name: &str) -> Option<&'static SlashCommandInfo> {
    SLASH_COMMANDS.iter().find(|command| command.name == name)
}

pub fn suggestions_for_terminal(
    prefix: &str,
    terminal: TerminalType,
) -> Vec<&'static SlashCommandInfo> {
    let visible = visible_commands_for_terminal(terminal);
    suggestions_for_commands(prefix, &visible)
}

fn suggestions_for_commands(
    prefix: &str,
    commands: &[&'static SlashCommandInfo],
) -> Vec<&'static SlashCommandInfo> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return commands.to_vec();
    }

    let query = trimmed.to_ascii_lowercase();

    let mut prefix_matches: Vec<&SlashCommandInfo> = commands
        .iter()
        .copied()
        .filter(|info| info.name.starts_with(&query))
        .collect();

    if !prefix_matches.is_empty() {
        prefix_matches.sort_by(|a, b| a.name.cmp(b.name));
        return prefix_matches;
    }

    let mut substring_matches: Vec<(&SlashCommandInfo, usize)> = commands
        .iter()
        .copied()
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
        return commands.to_vec();
    }

    let mut scored: Vec<(&SlashCommandInfo, usize, usize)> = commands
        .iter()
        .copied()
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
        return commands.to_vec();
    }

    scored.sort_by(|(a, name_pos_a, desc_pos_a), (b, name_pos_b, desc_pos_b)| {
        let score_a = (*name_pos_a == usize::MAX, *name_pos_a, *desc_pos_a, a.name);
        let score_b = (*name_pos_b == usize::MAX, *name_pos_b, *desc_pos_b, b.name);
        score_a.cmp(&score_b)
    });

    scored.into_iter().map(|(info, _, _)| info).collect()
}

/// Returns slash command metadata that match the provided prefix (case insensitive).
pub fn suggestions_for(prefix: &str) -> Vec<&'static SlashCommandInfo> {
    suggestions_for_terminal(prefix, detected_terminal_for_visibility())
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
        assert_eq!(names, vec!["clear", "command", "compact", "config", "copy"]);
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

    #[test]
    fn prefix_matches_include_history_command() {
        let names = names_for("his");
        assert_eq!(names, vec!["history"]);
    }

    #[test]
    fn prefix_matches_include_review_command() {
        let names = names_for("rev");
        assert_eq!(names, vec!["review"]);
    }

    #[test]
    fn suggestions_include_new_interactive_mode_commands() {
        let names = names_for("sug");
        assert_eq!(names, vec!["suggest"]);

        let names = names_for("task");
        assert_eq!(names, vec!["tasks"]);

        let names = names_for("job");
        assert_eq!(names, vec!["jobs"]);
    }

    #[test]
    fn terminal_setup_hidden_for_native_terminals() {
        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::WezTerm)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(!names.contains(&"terminal-setup"));

        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::ITerm2)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(!names.contains(&"terminal-setup"));

        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::WindowsTerminal)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(!names.contains(&"terminal-setup"));
    }

    #[test]
    fn terminal_setup_visible_for_supported_setup_terminals() {
        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::VSCode)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(names.contains(&"terminal-setup"));

        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::Alacritty)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(names.contains(&"terminal-setup"));

        let names: Vec<&str> = visible_commands_for_terminal(TerminalType::Zed)
            .into_iter()
            .map(|info| info.name)
            .collect();
        assert!(names.contains(&"terminal-setup"));
    }

    #[test]
    fn permissions_command_is_registered() {
        let command = find_command("permissions").expect("permissions command");
        assert_eq!(command.name, "permissions");
    }
}
