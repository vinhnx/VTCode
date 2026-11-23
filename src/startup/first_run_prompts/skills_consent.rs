use std::path::Path;

use anyhow::Result;
use vtcode_config::consent::{
    CONSENT_ALLOW_DESCRIPTION, CONSENT_ALLOW_LABEL, CONSENT_BODY_REVERSIBLE, CONSENT_BODY_WHAT,
    CONSENT_DECLINE_DESCRIPTION, CONSENT_DECLINE_LABEL, CONSENT_MCP_DISCLOSURE, CONSENT_QUESTION, CONSENT_REOPEN_HINT,
    CONSENT_TITLE, ConsentAnswer, bundled_skills_answer, install_location_description,
};
use vtcode_config::loader::ConfigManager;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

/// Previously recorded consent answer for `workspace`, if any layer sets it.
///
/// Returns `None` when the key is unset everywhere or the configuration
/// cannot be loaded; both cases mean the wizard must ask.
pub(crate) fn resolve_bundled_skills_answer(workspace: &Path) -> Option<bool> {
    let manager = ConfigManager::load_from_workspace(workspace).ok()?;
    match bundled_skills_answer(&manager) {
        ConsentAnswer::Granted => Some(true),
        ConsentAnswer::Declined => Some(false),
        ConsentAnswer::Unasked => None,
    }
}

pub(crate) fn prompt_bundled_skills_consent(renderer: &mut AnsiRenderer) -> Result<bool> {
    renderer.line(MessageStyle::Status, CONSENT_QUESTION)?;
    renderer.line(MessageStyle::Info, CONSENT_BODY_WHAT)?;
    renderer.line(MessageStyle::Info, &install_location_line())?;
    renderer.line(MessageStyle::Info, CONSENT_BODY_REVERSIBLE)?;
    renderer.line(MessageStyle::Info, CONSENT_MCP_DISCLOSURE)?;
    renderer.line(MessageStyle::Info, CONSENT_REOPEN_HINT)?;

    match select_consent_with_ratatui() {
        Ok(allowed) => Ok(allowed),
        Err(error) => {
            renderer.line(
                MessageStyle::Warning,
                &format!("Interactive setup could not continue. Enter the value manually.\n{error}"),
            )?;
            prompt_consent_text(renderer)
        }
    }
}

fn install_location_line() -> String {
    match vtcode_commons::VtCodePaths::resolve() {
        Ok(paths) => install_location_description(paths.data_dir()),
        Err(_) => "Installs to the VT Code data directory (offline, from the running binary).".to_string(),
    }
}

fn consent_entries() -> [(bool, SelectionEntry); 2] {
    [
        (
            true,
            SelectionEntry::new(CONSENT_ALLOW_LABEL.to_string(), Some(CONSENT_ALLOW_DESCRIPTION.to_string())),
        ),
        (
            false,
            SelectionEntry::new(CONSENT_DECLINE_LABEL.to_string(), Some(CONSENT_DECLINE_DESCRIPTION.to_string())),
        ),
    ]
}

fn select_consent_with_ratatui() -> Result<bool> {
    let entries = consent_entries();
    let selection_entries: Vec<SelectionEntry> = entries.iter().map(|(_allowed, entry)| entry.clone()).collect();
    let selected_index = run_selection(
        CONSENT_TITLE,
        "Default: Allow. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        &selection_entries,
        0,
    )?;
    Ok(entries[selected_index].0)
}

fn prompt_consent_text(renderer: &mut AnsiRenderer) -> Result<bool> {
    renderer.line(MessageStyle::Info, "  1) Allow (recommended)")?;
    renderer.line(MessageStyle::Info, "  2) Decline")?;

    loop {
        let input = prompt_with_placeholder("Bundled skills [allow]")?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(true);
        }

        match trimmed.to_ascii_lowercase().as_str() {
            "1" | "allow" | "yes" | "y" | "enable" | "enabled" => return Ok(true),
            "2" | "decline" | "no" | "n" | "disable" | "disabled" => return Ok(false),
            _ => renderer.line(MessageStyle::Error, "Please choose Allow or Decline for bundled skills.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consent_entries_use_shared_copy() {
        let entries = consent_entries();

        assert_eq!(entries[0].1.title, CONSENT_ALLOW_LABEL);
        assert_eq!(entries[1].1.title, CONSENT_DECLINE_LABEL);
        assert!(entries[0].0);
        assert!(!entries[1].0);
    }
}
