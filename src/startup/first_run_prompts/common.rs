use std::io::{self, Write};

use anyhow::{Context, Result, anyhow};
use vtcode_tui::ui::interactive_list::{
    SelectionEntry, SelectionInterrupted, run_interactive_selection,
};

#[derive(Debug, thiserror::Error)]
#[error("setup interrupted by Ctrl+C")]
pub(super) struct SetupInterrupted;

pub(super) fn prompt_with_placeholder(prompt: &str) -> Result<String> {
    print!("{}: ", prompt);
    io::stdout()
        .flush()
        .context("Failed to flush prompt to stdout")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("Failed to read setup input")?;
    Ok(input)
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
