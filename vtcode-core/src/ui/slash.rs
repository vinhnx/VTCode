use once_cell::sync::Lazy;

use crate::skills::command_skill_specs;
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
