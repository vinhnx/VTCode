use hashbrown::HashSet;

use anyhow::Result;
use vtcode_core::config::constants::{model_helpers, models};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::ui::interactive_list::SelectionEntry;

use super::common::{prompt_with_placeholder, run_selection};

pub(crate) fn prompt_model(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    default_model: &'static str,
) -> Result<String> {
    renderer.line(
        MessageStyle::Status,
        &format!(
            "Enter the default model for {} (Enter to accept {}).",
            provider.label(),
            default_model
        ),
    )?;

    let options = model_options(provider, default_model);

    match select_model_with_ratatui(&options, default_model) {
        Ok(model) => Ok(model),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_model_text(renderer, provider, default_model, &options)
        }
    }
}

pub(crate) fn default_model_for_provider(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => models::google::DEFAULT_MODEL,
        Provider::OpenAI => models::openai::DEFAULT_MODEL,
        Provider::Anthropic => models::anthropic::DEFAULT_MODEL,
        Provider::DeepSeek => models::deepseek::DEFAULT_MODEL,
        Provider::HuggingFace => models::huggingface::DEFAULT_MODEL,
        Provider::OpenRouter => models::openrouter::DEFAULT_MODEL,
        Provider::Ollama => models::ollama::DEFAULT_MODEL,
        Provider::LmStudio => models::lmstudio::DEFAULT_MODEL,
        Provider::Moonshot => models::minimax::MINIMAX_M2_5,
        Provider::ZAI => models::zai::DEFAULT_MODEL,
        Provider::Minimax => models::minimax::MINIMAX_M2_5,
    }
}

fn model_options(provider: Provider, default_model: &'static str) -> Vec<String> {
    let mut options: Vec<String> = model_helpers::supported_for(&provider.to_string())
        .map(|list| list.iter().map(|model| (*model).to_owned()).collect())
        .unwrap_or_default();

    if options.is_empty() {
        options.push(default_model.to_owned());
    }

    if !options.iter().any(|model| model == default_model) {
        options.insert(0, default_model.to_owned());
    }

    let mut seen = HashSet::new();
    options.retain(|model| seen.insert(model.clone()));
    options
}

fn select_model_with_ratatui(options: &[String], default_model: &'static str) -> Result<String> {
    let entries: Vec<SelectionEntry> = options
        .iter()
        .enumerate()
        .map(|(index, model)| SelectionEntry::new(format!("{:>2}. {}", index + 1, model), None))
        .collect();

    let default_index = options
        .iter()
        .position(|model| model == default_model)
        .unwrap_or(0);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default_model
    );
    let selected_index = run_selection("Models", &instructions, &entries, default_index)?;
    Ok(options[selected_index].clone())
}

fn prompt_model_text(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    default_model: &'static str,
    options: &[String],
) -> Result<String> {
    if !options.is_empty() {
        renderer.line(
            MessageStyle::Info,
            &format!("Suggested {} models:", provider.label()),
        )?;
        for (index, model) in options.iter().enumerate() {
            renderer.line(
                MessageStyle::Info,
                &format!("  {:>2}. {}", index + 1, model),
            )?;
        }
    }

    let input = prompt_with_placeholder(&format!("Model [{}]", default_model))?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(default_model.to_owned());
    }

    match trimmed.parse::<ModelId>() {
        Ok(id) => Ok(id.as_str().to_owned()),
        Err(_) => {
            renderer.line(
                MessageStyle::Info,
                "Unrecognized model identifier. It will be saved as entered.",
            )?;
            Ok(trimmed.to_owned())
        }
    }
}
