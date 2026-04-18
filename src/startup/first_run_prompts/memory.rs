use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

pub(crate) fn resolve_initial_persistent_memory_enabled(config: &VTCodeConfig) -> bool {
    config.persistent_memory_enabled()
}

pub(crate) fn prompt_persistent_memory(
    renderer: &mut AnsiRenderer,
    default_enabled: bool,
) -> Result<bool> {
    renderer.line(
        MessageStyle::Status,
        "Persistent memory controls whether VT Code keeps durable repository notes between sessions.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Leave it off if you want a lighter first-run setup. You can enable it later in `vtcode.toml`.",
    )?;

    match select_persistent_memory_with_ratatui(default_enabled) {
        Ok(enabled) => Ok(enabled),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_persistent_memory_text(renderer, default_enabled)
        }
    }
}

fn persistent_memory_entries() -> [(bool, SelectionEntry); 2] {
    [
        (
            false,
            SelectionEntry::new(
                "Off (recommended)".to_string(),
                Some("Do not write durable repository memory yet.".to_string()),
            ),
        ),
        (
            true,
            SelectionEntry::new(
                "On".to_string(),
                Some("Keep opt-in repository memory summaries for future sessions.".to_string()),
            ),
        ),
    ]
}

fn select_persistent_memory_with_ratatui(default_enabled: bool) -> Result<bool> {
    let entries = persistent_memory_entries();
    let default_index = usize::from(default_enabled);
    let selection_entries: Vec<SelectionEntry> = entries
        .iter()
        .map(|(_enabled, entry)| entry.clone())
        .collect();
    let instructions = if default_enabled {
        "Default: On. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default."
    } else {
        "Default: Off. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default."
    };
    let selected_index = run_selection(
        "Persistent memory",
        instructions,
        &selection_entries,
        default_index,
    )?;
    Ok(entries[selected_index].0)
}

fn prompt_persistent_memory_text(
    renderer: &mut AnsiRenderer,
    default_enabled: bool,
) -> Result<bool> {
    let entries = persistent_memory_entries();
    for (index, (enabled, _entry)) in entries.iter().enumerate() {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "  {}) {}",
                index + 1,
                if *enabled { "On" } else { "Off (recommended)" }
            ),
        )?;
    }

    loop {
        let default_label = if default_enabled { "on" } else { "off" };
        let input = prompt_with_placeholder(&format!("Persistent memory [{}]", default_label))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default_enabled);
        }

        match trimmed.to_ascii_lowercase().as_str() {
            "1" | "off" | "disable" | "disabled" | "no" | "n" => return Ok(false),
            "2" | "on" | "enable" | "enabled" | "yes" | "y" => return Ok(true),
            _ => renderer.line(
                MessageStyle::Error,
                "Please choose Off or On for persistent memory.",
            )?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::loader::VTCodeConfig;

    #[test]
    fn persistent_memory_entries_are_unnumbered() {
        let entries = persistent_memory_entries();

        assert_eq!(entries[0].1.title, "Off (recommended)");
        assert_eq!(entries[1].1.title, "On");
    }

    #[test]
    fn resolve_initial_persistent_memory_enabled_reads_config() {
        let mut config = VTCodeConfig::default();
        config.features.memories = true;
        config.agent.persistent_memory.enabled = true;

        assert!(resolve_initial_persistent_memory_enabled(&config));
    }
}
