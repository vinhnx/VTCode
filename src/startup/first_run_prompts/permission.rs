use anyhow::Result;
use vtcode_config::core::permissions::DefaultPermissionMode;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

const ASK_LABEL: &str = "Ask – prompt before protected actions (recommended)";
const AUTO_LABEL: &str = "Auto-approve – skip confirmations (sandbox and explicit denies still apply)";

pub(crate) fn prompt_permission_mode(
    renderer: &mut AnsiRenderer,
    default: DefaultPermissionMode,
) -> Result<DefaultPermissionMode> {
    renderer.line(MessageStyle::Status, "Permission mode controls when VT Code asks before acting.")?;
    renderer.line(MessageStyle::Info, "  [1] Ask – prompt before protected actions (recommended)")?;
    renderer.line(MessageStyle::Info, "  [2] Auto-approve – skip confirmations")?;
    renderer.line(
        MessageStyle::Info,
        "Full access stays per-session only (--full-access); it is never saved as a default.",
    )?;

    match select_permission_mode_with_ratatui(default) {
        Ok(mode) => Ok(mode),
        Err(error) => {
            renderer.line(
                MessageStyle::Warning,
                &format!("Interactive setup could not continue. Enter the value manually.\n{error}"),
            )?;
            prompt_permission_mode_text(renderer, default)
        }
    }
}

fn prompt_permission_mode_text(
    renderer: &mut AnsiRenderer,
    default: DefaultPermissionMode,
) -> Result<DefaultPermissionMode> {
    let default_choice = match default {
        DefaultPermissionMode::Ask => "1",
        DefaultPermissionMode::AutoApprove => "2",
    };

    loop {
        let input = prompt_with_placeholder(&format!("Permission mode [{default_choice}]"))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        match trimmed {
            "1" | "ask" => return Ok(DefaultPermissionMode::Ask),
            "2" | "auto" | "auto-approve" | "auto_approve" => return Ok(DefaultPermissionMode::AutoApprove),
            _ => {
                renderer.line(MessageStyle::Error, "Please choose 1 for Ask or 2 for Auto-approve.")?;
            }
        }
    }
}

fn permission_entries() -> [(DefaultPermissionMode, SelectionEntry); 2] {
    [
        (DefaultPermissionMode::Ask, SelectionEntry::new(ASK_LABEL.to_owned(), None)),
        (DefaultPermissionMode::AutoApprove, SelectionEntry::new(AUTO_LABEL.to_owned(), None)),
    ]
}

fn select_permission_mode_with_ratatui(default: DefaultPermissionMode) -> Result<DefaultPermissionMode> {
    let entries = permission_entries();

    let default_index = match default {
        DefaultPermissionMode::Ask => 0,
        DefaultPermissionMode::AutoApprove => 1,
    };

    let selection_entries: Vec<SelectionEntry> = entries.iter().map(|(_mode, entry)| entry.clone()).collect();
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        selection_entries[default_index].title
    );
    let selected_index = run_selection("Permission mode", &instructions, &selection_entries, default_index)?;
    Ok(entries[selected_index].0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_entries_are_unnumbered() {
        let entries = permission_entries();

        assert_eq!(entries[0].1.title, ASK_LABEL);
        assert_eq!(entries[1].1.title, AUTO_LABEL);
    }
}
