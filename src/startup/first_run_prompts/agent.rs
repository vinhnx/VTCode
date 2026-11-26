use anyhow::Result;
use vtcode_config::SubagentSpec;
use vtcode_tui_runtime::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection, select_numbered_option};

/// Prompt for the default agent mode, offering the same primary-agent catalog
/// the desktop mode picker uses. Returns the selected agent name.
pub(crate) fn prompt_agent_mode(
    renderer: &mut AnsiRenderer,
    specs: &[SubagentSpec],
    default_name: &str,
) -> Result<String> {
    if specs.is_empty() {
        return Ok(default_name.to_owned());
    }
    renderer.line(MessageStyle::Status, "Agent mode controls the default working style.")?;
    for (index, spec) in specs.iter().enumerate() {
        renderer.line(MessageStyle::Info, &format!("  [{}] {} – {}", index + 1, spec.name, spec.description))?;
    }

    match select_agent_mode_with_ratatui(specs, default_name) {
        Ok(name) => Ok(name),
        Err(error) => {
            renderer.line(
                MessageStyle::Warning,
                &format!("Interactive setup could not continue. Enter the value manually.\n{error}"),
            )?;
            prompt_agent_mode_text(renderer, specs, default_name)
        }
    }
}

fn prompt_agent_mode_text(renderer: &mut AnsiRenderer, specs: &[SubagentSpec], default_name: &str) -> Result<String> {
    let default_choice = specs
        .iter()
        .position(|spec| spec.name == default_name)
        .map_or(1, |index| index + 1)
        .to_string();

    loop {
        let input = prompt_with_placeholder(&format!("Agent mode [{default_choice}]"))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default_name.to_owned());
        }

        if let Some(name) = resolve_agent_choice(specs, trimmed) {
            return Ok(name);
        }
        renderer.line(MessageStyle::Error, "Please choose a listed number or agent name.")?;
    }
}

/// Resolve free-text input to an agent name: 1-based number first, then
/// case-insensitive name match.
fn resolve_agent_choice(specs: &[SubagentSpec], trimmed: &str) -> Option<String> {
    if let Some(spec) = select_numbered_option(specs, trimmed) {
        return Some(spec.name.clone());
    }
    specs
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(trimmed))
        .map(|spec| spec.name.clone())
}

fn agent_entries(specs: &[SubagentSpec]) -> Vec<(String, SelectionEntry)> {
    specs
        .iter()
        .map(|spec| (spec.name.clone(), SelectionEntry::new(format!("{} – {}", spec.name, spec.description), None)))
        .collect()
}

fn select_agent_mode_with_ratatui(specs: &[SubagentSpec], default_name: &str) -> Result<String> {
    let entries = agent_entries(specs);
    let default_index = specs.iter().position(|spec| spec.name == default_name).unwrap_or(0);

    let selection_entries: Vec<SelectionEntry> = entries.iter().map(|(_name, entry)| entry.clone()).collect();
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        selection_entries[default_index].title
    );
    let selected_index = run_selection("Agent mode", &instructions, &selection_entries, default_index)?;
    Ok(entries[selected_index].0.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(name: &str) -> SubagentSpec {
        let mut spec = vtcode_config::builtin_primary_build_agent();
        spec.name = name.to_owned();
        spec.description = format!("{name} agent");
        spec
    }

    #[test]
    fn agent_choice_resolves_number_then_name() {
        let specs = [spec("build"), spec("plan")];

        assert_eq!(resolve_agent_choice(&specs, "1"), Some("build".to_string()));
        assert_eq!(resolve_agent_choice(&specs, "2"), Some("plan".to_string()));
        assert_eq!(resolve_agent_choice(&specs, "PLAN"), Some("plan".to_string()));
        assert_eq!(resolve_agent_choice(&specs, "duck"), None);
        assert_eq!(resolve_agent_choice(&specs, "9"), None);
    }

    #[test]
    fn agent_entries_pair_names_with_labeled_rows() {
        let entries = agent_entries(&[spec("build")]);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, "build");
        assert!(entries[0].1.title.starts_with("build – "));
    }
}
