use anyhow::Result;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

pub(crate) fn prompt_trust(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    renderer.line(
        MessageStyle::Status,
        "Workspace trust determines which actions are allowed.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [1] Tools policy – prompts before running elevated actions (recommended)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [2] Full auto – allow unattended execution without prompts",
    )?;

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

fn select_trust_with_ratatui(default: WorkspaceTrustLevel) -> Result<WorkspaceTrustLevel> {
    let entries = [
        (
            WorkspaceTrustLevel::ToolsPolicy,
            SelectionEntry::new(
                " 1. Tools policy – prompts before running elevated actions (recommended)"
                    .to_owned(),
                Some(
                    "Tools policy – prompts before running elevated actions (recommended)"
                        .to_owned(),
                ),
            ),
        ),
        (
            WorkspaceTrustLevel::FullAuto,
            SelectionEntry::new(
                " 2. Full auto – allow unattended execution without prompts".to_owned(),
                Some("Full auto – allow unattended execution without prompts".to_owned()),
            ),
        ),
    ];

    let default_index = match default {
        WorkspaceTrustLevel::ToolsPolicy => 0,
        WorkspaceTrustLevel::FullAuto => 1,
    };

    let selection_entries: Vec<SelectionEntry> = entries
        .iter()
        .map(|(_level, entry)| entry.clone())
        .collect();
    let default_entry = &selection_entries[default_index];
    let default_summary = default_entry
        .description
        .as_deref()
        .unwrap_or(default_entry.title.as_str());
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default_summary
    );
    let selected_index = run_selection(
        "Workspace trust",
        &instructions,
        &selection_entries,
        default_index,
    )?;
    Ok(entries[selected_index].0)
}
