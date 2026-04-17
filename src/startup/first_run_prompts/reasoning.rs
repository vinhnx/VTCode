use anyhow::Result;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

pub(crate) fn prompt_reasoning_effort(
    renderer: &mut AnsiRenderer,
    default: ReasoningEffortLevel,
) -> Result<ReasoningEffortLevel> {
    renderer.line(
        MessageStyle::Status,
        "Choose reasoning effort level for models that support it:",
    )?;

    let levels = [
        (
            ReasoningEffortLevel::None,
            "None – lowest latency, good default for GPT-5.4",
        ),
        (
            ReasoningEffortLevel::Low,
            "Low – faster responses, less reasoning",
        ),
        (
            ReasoningEffortLevel::Medium,
            "Medium – balanced reasoning for harder multi-step work",
        ),
        (
            ReasoningEffortLevel::High,
            "High – deeper reasoning, slower responses",
        ),
        (
            ReasoningEffortLevel::XHigh,
            "Extra High – best default for advanced coding and agentic work",
        ),
        (
            ReasoningEffortLevel::Max,
            "Max – highest adaptive effort for supported Anthropic models, highest latency and token use",
        ),
    ];

    match select_reasoning_with_ratatui(&levels, default) {
        Ok(level) => Ok(level),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_reasoning_effort_text(renderer, &levels, default)
        }
    }
}

fn reasoning_entries(levels: &[(ReasoningEffortLevel, &str)]) -> Vec<SelectionEntry> {
    levels
        .iter()
        .map(|(_level, label)| SelectionEntry::new((*label).to_owned(), None))
        .collect()
}

fn select_reasoning_with_ratatui(
    levels: &[(ReasoningEffortLevel, &str)],
    default: ReasoningEffortLevel,
) -> Result<ReasoningEffortLevel> {
    let entries = reasoning_entries(levels);

    let default_index = levels
        .iter()
        .position(|(level, _)| *level == default)
        .unwrap_or(1);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default.as_str()
    );
    let selected_index = run_selection("Reasoning Effort", &instructions, &entries, default_index)?;
    Ok(levels[selected_index].0)
}

fn prompt_reasoning_effort_text(
    renderer: &mut AnsiRenderer,
    levels: &[(ReasoningEffortLevel, &str)],
    default: ReasoningEffortLevel,
) -> Result<ReasoningEffortLevel> {
    for (index, (_level, label)) in levels.iter().enumerate() {
        renderer.line(MessageStyle::Info, &format!("  {}) {}", index + 1, label))?;
    }

    loop {
        let input = prompt_with_placeholder(&format!("Reasoning effort [{}]", default.as_str()))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        if let Ok(index) = trimmed.parse::<usize>()
            && let Some((level, _)) = levels.get(index - 1)
        {
            return Ok(*level);
        }

        if let Some(level) = ReasoningEffortLevel::parse(trimmed) {
            return Ok(level);
        }

        renderer.line(
            MessageStyle::Error,
            "Please choose a valid reasoning effort level (none, low, medium, high, xhigh, max).",
        )?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_entries_keep_labels_without_ordinals() {
        let entries = reasoning_entries(&[
            (
                ReasoningEffortLevel::None,
                "None – lowest latency, good default for GPT-5.4",
            ),
            (
                ReasoningEffortLevel::Low,
                "Low – faster responses, less reasoning",
            ),
        ]);

        assert_eq!(
            entries[0].title,
            "None – lowest latency, good default for GPT-5.4"
        );
        assert_eq!(entries[1].title, "Low – faster responses, less reasoning");
    }
}
