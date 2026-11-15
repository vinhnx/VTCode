use std::io::{self, Write};

use anyhow::{Context, Result, anyhow};
use vtcode_ui::tui::ui::interactive_list::{SelectionEntry, SelectionInterrupted, run_interactive_selection};

#[derive(Debug, thiserror::Error)]
#[error("setup interrupted by Ctrl+C")]
pub(super) struct SetupInterrupted;

pub(super) fn prompt_with_placeholder(prompt: &str) -> Result<String> {
    print!("{prompt}: ");
    io::stdout().flush().context("Failed to flush prompt to stdout")?;

    let mut input = String::new();
    io::stdin().read_line(&mut input).context("Failed to read setup input")?;
    Ok(input)
}

/// Select a 1-based numbered option from a text prompt.
///
/// Returns `None` for non-numeric input or an out-of-range number. Zero
/// saturates to the first option, matching the model/init prompts.
pub(super) fn select_numbered_option<'a, T>(options: &'a [T], trimmed: &str) -> Option<&'a T> {
    let index = trimmed.parse::<usize>().ok()?;
    options.get(index.saturating_sub(1))
}

pub(super) fn run_selection(
    title: &str,
    instructions: &str,
    entries: &[SelectionEntry],
    default_index: usize,
) -> Result<usize> {
    if entries.is_empty() {
        return Err(anyhow!("No entries available for selection"));
    }

    let safe_default = default_index.min(entries.len() - 1);
    match run_interactive_selection(title, instructions, entries, safe_default) {
        Ok(Some(index)) => Ok(index),
        Ok(None) => Ok(safe_default),
        Err(err) => {
            if err.is::<SelectionInterrupted>() {
                return Err(SetupInterrupted.into());
            }
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbered_selection_covers_zero_valid_and_invalid_input() {
        let options = ["low", "medium", "high"];
        assert_eq!(select_numbered_option(&options, "1"), Some(&"low"));
        assert_eq!(select_numbered_option(&options, "3"), Some(&"high"));
        // Zero saturates to the first option, matching the model/init prompts.
        assert_eq!(select_numbered_option(&options, "0"), Some(&"low"));
        assert_eq!(select_numbered_option(&options, "4"), None);
        assert_eq!(select_numbered_option(&options, "abc"), None);
        assert_eq!(select_numbered_option(&options, ""), None);
        assert_eq!(select_numbered_option(&[] as &[&str], "1"), None);
    }
}
