use anyhow::Result;
use vtcode_core::config::PermissionMode;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StartupMode {
    Edit,
    Auto,
    Plan,
}

impl StartupMode {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit",
            Self::Auto => "Auto",
            Self::Plan => "Plan",
        }
    }

    const fn description(self) -> &'static str {
        match self {
            Self::Edit => "Standard interactive mode with normal confirmations.",
            Self::Auto => {
                "Classifier-backed autonomous mode inside the normal interactive session."
            }
            Self::Plan => "Read-only planning mode for research, specs, and architecture work.",
        }
    }
}

pub(crate) fn resolve_initial_startup_mode(config: &VTCodeConfig) -> StartupMode {
    if config.permissions.default_mode == PermissionMode::Plan {
        StartupMode::Plan
    } else if config.permissions.default_mode == PermissionMode::Auto {
        StartupMode::Auto
    } else {
        StartupMode::Edit
    }
}

pub(crate) fn prompt_startup_mode(
    renderer: &mut AnsiRenderer,
    default: StartupMode,
) -> Result<StartupMode> {
    renderer.line(
        MessageStyle::Status,
        "Choose the startup mode VT Code should use for new sessions:",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Edit is the standard interactive mode. Auto keeps the normal session flow but routes riskier actions through the classifier.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Plan starts in read-only planning mode. `--full-auto` is separate and remains an advanced explicit workflow.",
    )?;

    match select_startup_mode_with_ratatui(default) {
        Ok(mode) => Ok(mode),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_startup_mode_text(renderer, default)
        }
    }
}

fn startup_mode_entries() -> [(StartupMode, SelectionEntry); 3] {
    [
        (
            StartupMode::Edit,
            SelectionEntry::new(
                "Edit (recommended)".to_string(),
                Some(StartupMode::Edit.description().to_string()),
            ),
        ),
        (
            StartupMode::Auto,
            SelectionEntry::new(
                StartupMode::Auto.label().to_string(),
                Some(StartupMode::Auto.description().to_string()),
            ),
        ),
        (
            StartupMode::Plan,
            SelectionEntry::new(
                StartupMode::Plan.label().to_string(),
                Some(StartupMode::Plan.description().to_string()),
            ),
        ),
    ]
}

fn select_startup_mode_with_ratatui(default: StartupMode) -> Result<StartupMode> {
    let entries = startup_mode_entries();
    let default_index = match default {
        StartupMode::Edit => 0,
        StartupMode::Auto => 1,
        StartupMode::Plan => 2,
    };
    let selection_entries: Vec<SelectionEntry> =
        entries.iter().map(|(_mode, entry)| entry.clone()).collect();
    let instructions =
        "Default: Edit. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.";
    let selected_index = run_selection(
        "Startup mode",
        instructions,
        &selection_entries,
        default_index,
    )?;
    Ok(entries[selected_index].0)
}

fn prompt_startup_mode_text(
    renderer: &mut AnsiRenderer,
    default: StartupMode,
) -> Result<StartupMode> {
    let entries = startup_mode_entries();
    for (index, (mode, _entry)) in entries.iter().enumerate() {
        renderer.line(
            MessageStyle::Info,
            &format!("  {}) {} — {}", index + 1, mode.label(), mode.description()),
        )?;
    }

    loop {
        let input = prompt_with_placeholder(&format!("Startup mode [{}]", default.label()))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        if let Ok(index) = trimmed.parse::<usize>()
            && let Some((mode, _entry)) = entries.get(index.saturating_sub(1))
        {
            return Ok(*mode);
        }

        match trimmed.to_ascii_lowercase().as_str() {
            "edit" => return Ok(StartupMode::Edit),
            "auto" => return Ok(StartupMode::Auto),
            "plan" => return Ok(StartupMode::Plan),
            _ => renderer.line(MessageStyle::Error, "Please choose Edit, Auto, or Plan.")?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_core::config::loader::VTCodeConfig;

    #[test]
    fn startup_mode_entries_are_unnumbered() {
        let entries = startup_mode_entries();

        assert_eq!(entries[0].1.title, "Edit (recommended)");
        assert_eq!(entries[1].1.title, "Auto");
        assert_eq!(entries[2].1.title, "Plan");
    }

    #[test]
    fn resolve_initial_startup_mode_prefers_plan() {
        let mut config = VTCodeConfig::default();
        config.permissions.default_mode = PermissionMode::Plan;

        assert_eq!(resolve_initial_startup_mode(&config), StartupMode::Plan);
    }

    #[test]
    fn resolve_initial_startup_mode_maps_auto_defaults() {
        let mut config = VTCodeConfig::default();
        config.permissions.default_mode = PermissionMode::Auto;

        assert_eq!(resolve_initial_startup_mode(&config), StartupMode::Auto);
    }
}
