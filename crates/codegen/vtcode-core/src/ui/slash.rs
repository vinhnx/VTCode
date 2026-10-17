use once_cell::sync::Lazy;

use crate::skills::command_skill_specs;
use crate::terminal_setup::detector::TerminalType;
use crate::ui::search::{fuzzy_match, normalize_query};

/// Metadata describing a slash command supported by the chat interface.
#[derive(Clone, Copy, Debug)]
pub struct SlashCommandInfo {
    /// Command name typed after the `/` prefix (e.g. `"compact"`).
    pub name: &'static str,
    /// Short human-readable description shown in autocomplete.
    pub description: &'static str,
}

/// Collection of slash command definitions in the order they should be displayed.
pub static SLASH_COMMANDS: Lazy<Vec<SlashCommandInfo>> = Lazy::new(|| {
    command_skill_specs()
        .iter()
        .map(|spec| SlashCommandInfo {
            name: spec.slash_name,
            description: spec.description,
        })
        .collect()
});

fn detected_terminal_for_visibility() -> TerminalType {
    TerminalType::detect().unwrap_or(TerminalType::Unknown)
}

fn command_visible_for_terminal(command: &SlashCommandInfo, terminal: TerminalType) -> bool {
    command.name != "terminal-setup" || terminal.should_offer_terminal_setup()
}

/// Return all slash commands visible for the auto-detected terminal.
pub fn visible_commands() -> Vec<&'static SlashCommandInfo> {
    visible_commands_for_terminal(detected_terminal_for_visibility())
}

/// Return slash commands visible for the given terminal type.
pub fn visible_commands_for_terminal(terminal: TerminalType) -> Vec<&'static SlashCommandInfo> {
    SLASH_COMMANDS
        .iter()
        .filter(|command| command_visible_for_terminal(command, terminal))
        .collect()
}

/// Find a visible slash command by exact name for the auto-detected terminal.
pub fn find_visible_command(name: &str) -> Option<&'static SlashCommandInfo> {
    let terminal = detected_terminal_for_visibility();
    SLASH_COMMANDS
        .iter()
        .find(|command| command.name == name && command_visible_for_terminal(command, terminal))
}

/// Find a slash command by exact name, regardless of terminal visibility.
pub fn find_command(name: &str) -> Option<&'static SlashCommandInfo> {
    SLASH_COMMANDS.iter().find(|command| command.name == name)
}

/// Return fuzzy-matched slash command suggestions for the given terminal.
pub fn suggestions_for_terminal(prefix: &str, terminal: TerminalType) -> Vec<&'static SlashCommandInfo> {
    let visible = visible_commands_for_terminal(terminal);
    suggestions_for_commands(prefix, &visible)
}

fn suggestions_for_commands(prefix: &str, commands: &[&'static SlashCommandInfo]) -> Vec<&'static SlashCommandInfo> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() {
        return commands.to_vec();
    }

    let query = trimmed.to_ascii_lowercase();

    let mut prefix_matches: Vec<&SlashCommandInfo> =
        commands.iter().copied().filter(|info| info.name.starts_with(&query)).collect();

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
        substring_matches
            .sort_by(|(a, pos_a), (b, pos_b)| (*pos_a, a.name.len(), a.name).cmp(&(*pos_b, b.name.len(), b.name)));
        return substring_matches.into_iter().map(|(info, _)| info).collect();
    }

    let normalized_query = normalize_query(&query);
    if normalized_query.is_empty() {
        return commands.to_vec();
    }

    let mut scored: Vec<(&SlashCommandInfo, usize, usize)> = commands
        .iter()
        .copied()
        .filter_map(|info| {
            // The command set is static `&'static str`, but each lowercased
            // form was previously recomputed up to 4x per candidate per
            // keystroke (name + description, once for the fuzzy candidate and
            // again for position lookups). Build the candidate once from the
            // lowercased name, remember where the name portion ends, and reuse
            // that slice plus the lowercased description for positions — two
            // allocations per candidate instead of four.
            let mut candidate = info.name.to_ascii_lowercase();
            let name_portion_end = candidate.len();
            let desc_lower = if info.description.is_empty() {
                String::new()
            } else {
                info.description.to_ascii_lowercase()
            };
            if !desc_lower.is_empty() {
                candidate.push(' ');
                candidate.push_str(&desc_lower);
            }

            if !fuzzy_match(&normalized_query, &candidate) {
                return None;
            }

            let name_pos = candidate[..name_portion_end].find(&query).unwrap_or(usize::MAX);
            let desc_pos = desc_lower.find(&query).unwrap_or(usize::MAX);

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
        suggestions_for(prefix).into_iter().map(|info| info.name).collect()
    }

    #[test]
    fn prefix_matches_are_sorted_alphabetically() {
        let names = names_for("c");
        assert_eq!(names, vec!["clear", "compact", "config", "copy"]);
    }

    #[test]
    fn substring_matches_prioritize_earlier_occurrences() {
        let names = names_for("eme");
        assert_eq!(names, vec!["theme"]);
    }

    #[test]
    fn fuzzy_matches_include_description_keywords() {
        let names = names_for("check");
        assert!(names.contains(&"config"));
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
    fn checkpoint_commands_are_visible_and_suggested() {
        let visible = visible_commands_for_terminal(TerminalType::Unknown);
        let suggestions = suggestions_for_terminal("re", TerminalType::Unknown);
        for name in ["rewind", "redo"] {
            assert!(visible.iter().any(|command| command.name == name), "{name} must be visible");
            assert!(suggestions.iter().any(|command| command.name == name), "{name} must be suggested");
            assert_eq!(find_command(name).expect("checkpoint command").name, name);
        }
        assert!(find_command("rewind-recover").is_none(), "rewind-recover must be removed; /rewind auto-recovers");
        assert_eq!(names_for("redo"), vec!["redo"]);
    }

    #[test]
    fn consolidated_panel_commands_route_via_config() {
        assert!(names_for("task").is_empty() || !names_for("task").contains(&"tasks"));
        assert!(names_for("job").is_empty() || !names_for("job").contains(&"jobs"));
        assert!(find_command("config").is_some());
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
    fn config_consolidated_commands_are_not_registered() {
        for removed in [
            "checkup",
            "permissions",
            "ide",
            "tasks",
            "jobs",
            "log",
            "subprocess",
            "notify",
        ] {
            assert!(find_command(removed).is_none(), "/{removed} must route via /config");
        }
        assert!(find_command("config").is_some());
    }
}
