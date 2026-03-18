use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

const TOOLS_POLICY_LABEL: &str =
    "Tools policy – prompts before running elevated actions (recommended)";
const FULL_AUTO_LABEL: &str = "Full auto – allow unattended execution without prompts";

pub(crate) fn prompt_trust(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    renderer.line(
        MessageStyle::Status,
        "Workspace trust determines which actions are allowed.",
    )?;
    renderer.line(MessageStyle::Info, &format!("  [1] {TOOLS_POLICY_LABEL}"))?;
    renderer.line(MessageStyle::Info, &format!("  [2] {FULL_AUTO_LABEL}"))?;

    match select_trust_with_ratatui(default) {
        Ok(level) => Ok(level),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_trust_text(renderer, default)
        }
    }
}

fn prompt_trust_text(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    let default_choice = match default {
        WorkspaceTrustLevel::ToolsPolicy => "1",
        WorkspaceTrustLevel::FullAuto => "2",
    };

    loop {
        let input = prompt_with_placeholder(&format!("Trust level [{}]", default_choice))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        match trimmed {
            "1" | "tools" | "tool" => return Ok(WorkspaceTrustLevel::ToolsPolicy),
            "2" | "full" | "auto" | "full-auto" => return Ok(WorkspaceTrustLevel::FullAuto),
            _ => {
                renderer.line(
                    MessageStyle::Error,
                    "Please choose 1 for Tools policy or 2 for Full auto.",
                )?;
            }
        }
    }
}

fn trust_entries() -> [(WorkspaceTrustLevel, SelectionEntry); 2] {
    [
        (
            WorkspaceTrustLevel::ToolsPolicy,
            SelectionEntry::new(TOOLS_POLICY_LABEL.to_owned(), None),
        ),
        (
            WorkspaceTrustLevel::FullAuto,
            SelectionEntry::new(FULL_AUTO_LABEL.to_owned(), None),
        ),
    ]
}

fn select_trust_with_ratatui(default: WorkspaceTrustLevel) -> Result<WorkspaceTrustLevel> {
    let entries = trust_entries();

    let default_index = match default {
        WorkspaceTrustLevel::ToolsPolicy => 0,
        WorkspaceTrustLevel::FullAuto => 1,
    };

    let selection_entries: Vec<SelectionEntry> = entries
        .iter()
        .map(|(_level, entry)| entry.clone())
        .collect();
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        selection_entries[default_index].title
    );
    let selected_index = run_selection(
        "Workspace trust",
        &instructions,
        &selection_entries,
        default_index,
    )?;
    Ok(entries[selected_index].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_entries_are_unnumbered() {
        let entries = trust_entries();

        assert_eq!(entries[0].1.title, TOOLS_POLICY_LABEL);
        assert_eq!(entries[1].1.title, FULL_AUTO_LABEL);
    }
}
