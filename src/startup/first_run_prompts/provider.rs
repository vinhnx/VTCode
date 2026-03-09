use std::str::FromStr;

use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::Provider;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

pub(crate) fn resolve_initial_provider(config: &VTCodeConfig) -> Provider {
    let configured = config.agent.provider.trim();
    let fallback = Provider::from_str(vtcode_core::config::constants::defaults::DEFAULT_PROVIDER)
        .unwrap_or(Provider::OpenAI);

    if configured.is_empty() {
        fallback
    } else {
        Provider::from_str(configured).unwrap_or(fallback)
    }
}

pub(crate) fn prompt_provider(renderer: &mut AnsiRenderer, default: Provider) -> Result<Provider> {
    renderer.line(MessageStyle::Status, "Choose your default provider:")?;
    let providers = Provider::all_providers();

    match select_provider_with_ratatui(&providers, default) {
        Ok(provider) => Ok(provider),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_provider_text(renderer, &providers, default)
        }
    }
}

fn prompt_provider_text(
    renderer: &mut AnsiRenderer,
    providers: &[Provider],
    default: Provider,
) -> Result<Provider> {
    for (index, provider) in providers.iter().enumerate() {
        renderer.line(
            MessageStyle::Info,
            &format!("  {}) {}", index + 1, provider.label()),
        )?;
    }

    let default_label = default.to_string();

    loop {
        let input = prompt_with_placeholder(&format!("Provider [{}]", default_label))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        if let Ok(index) = trimmed.parse::<usize>()
            && let Some(provider) = providers.get(index - 1)
        {
            return Ok(*provider);
        }

        match Provider::from_str(trimmed) {
            Ok(provider) => return Ok(provider),
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("{err}. Please choose a valid provider."),
                )?;
            }
        }
    }
}

fn select_provider_with_ratatui(providers: &[Provider], default: Provider) -> Result<Provider> {
    let entries: Vec<SelectionEntry> = providers
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            SelectionEntry::new(format!("{:>2}. {}", index + 1, provider.label()), None)
        })
        .collect();

    let default_index = providers
        .iter()
        .position(|provider| *provider == default)
        .unwrap_or(0);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default.label()
    );
    let selected_index = run_selection("Providers", &instructions, &entries, default_index)?;
    Ok(providers[selected_index])
}
