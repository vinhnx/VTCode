use hashbrown::HashSet;

use anyhow::Result;
use vtcode_core::config::constants::{model_helpers, models};
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::llm::lightweight_model_choices;
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
        Provider::Copilot => models::copilot::DEFAULT_MODEL,
        Provider::DeepSeek => models::deepseek::DEFAULT_MODEL,
        Provider::HuggingFace => models::huggingface::DEFAULT_MODEL,
        Provider::OpenRouter => models::openrouter::DEFAULT_MODEL,
        Provider::Ollama => models::ollama::DEFAULT_MODEL,
        Provider::LmStudio => models::lmstudio::DEFAULT_MODEL,
        Provider::Moonshot => models::moonshot::DEFAULT_MODEL,
        Provider::ZAI => models::zai::DEFAULT_MODEL,
        Provider::Minimax => models::minimax::MINIMAX_M2_5,
        Provider::OpenCodeZen => models::opencode_zen::DEFAULT_MODEL,
        Provider::OpenCodeGo => models::opencode_go::DEFAULT_MODEL,
    }
}

pub(crate) fn prompt_lightweight_model(
    renderer: &mut AnsiRenderer,
    provider: Provider,
    main_model: &str,
) -> Result<String> {
    renderer.line(
        MessageStyle::Status,
        "Choose the lightweight model VT Code should prefer for memory triage, prompt suggestions, and smaller delegated tasks.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Automatic picks a lighter sibling from the same provider when available, or the nearest cheaper fallback, and falls back to the main model if needed.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Use the main model if you want accuracy-first memory extraction and do not mind higher cost or latency.",
    )?;

    let options = lightweight_model_options(provider, main_model);
    match select_lightweight_model_with_ratatui(&options) {
        Ok(selected) => Ok(selected),
        Err(error) => {
            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_lightweight_model_text(renderer, &options)
        }
    }
}

fn model_options(provider: Provider, default_model: &'static str) -> Vec<String> {
    let mut options: Vec<String> = model_helpers::supported_for(provider.as_ref())
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

fn model_entries(options: &[String]) -> Vec<SelectionEntry> {
    options
        .iter()
        .map(|model| SelectionEntry::new(model.clone(), None))
        .collect()
}

#[derive(Clone)]
struct LightweightModelOption {
    label: String,
    subtitle: String,
    value: String,
}

fn lightweight_model_options(provider: Provider, main_model: &str) -> Vec<LightweightModelOption> {
    let auto_model = vtcode_core::llm::auto_lightweight_model(provider.as_ref(), main_model);
    let provider_models = lightweight_model_choices(provider.as_ref(), main_model);

    let mut options = vec![
        LightweightModelOption {
            label: "Automatic (recommended)".to_string(),
            subtitle: format!(
                "Use {} for lower-cost memory triage and fall back to {}.",
                display_model_label(&auto_model),
                display_model_label(main_model)
            ),
            value: String::new(),
        },
        LightweightModelOption {
            label: "Use main model".to_string(),
            subtitle: format!(
                "Keep memory extraction on {} for accuracy-first behavior.",
                display_model_label(main_model)
            ),
            value: main_model.to_string(),
        },
    ];

    for model in provider_models {
        if model.eq_ignore_ascii_case(main_model) {
            continue;
        }
        options.push(LightweightModelOption {
            label: display_model_label(&model),
            subtitle: explicit_lightweight_subtitle(provider, &model, &auto_model),
            value: model,
        });
    }

    options
}

fn select_model_with_ratatui(options: &[String], default_model: &'static str) -> Result<String> {
    let entries = model_entries(options);

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

fn select_lightweight_model_with_ratatui(options: &[LightweightModelOption]) -> Result<String> {
    let entries = options
        .iter()
        .map(|option| SelectionEntry::new(option.label.clone(), Some(option.subtitle.clone())))
        .collect::<Vec<_>>();
    let instructions = "Automatic is recommended. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep Automatic.";
    let selected_index = run_selection("Lightweight model", instructions, &entries, 0)?;
    Ok(options[selected_index].value.clone())
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

fn prompt_lightweight_model_text(
    renderer: &mut AnsiRenderer,
    options: &[LightweightModelOption],
) -> Result<String> {
    renderer.line(MessageStyle::Info, "Lightweight model options:")?;
    for (index, option) in options.iter().enumerate() {
        renderer.line(
            MessageStyle::Info,
            &format!("  {:>2}. {} — {}", index + 1, option.label, option.subtitle),
        )?;
    }

    let input = prompt_with_placeholder("Lightweight model [automatic]")?;
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Ok(String::new());
    }
    if trimmed.eq_ignore_ascii_case("main") {
        return Ok(options
            .iter()
            .find(|option| option.label == "Use main model")
            .map(|option| option.value.clone())
            .unwrap_or_default());
    }
    if let Ok(index) = trimmed.parse::<usize>()
        && let Some(option) = options.get(index.saturating_sub(1))
    {
        return Ok(option.value.clone());
    }

    renderer.line(
        MessageStyle::Info,
        "Unrecognized choice. It will be saved as entered.",
    )?;
    Ok(trimmed.to_string())
}

fn display_model_label(model: &str) -> String {
    model
        .parse::<ModelId>()
        .map(|model_id| model_id.display_name().to_string())
        .unwrap_or_else(|_| model.to_string())
}

fn explicit_lightweight_subtitle(provider: Provider, model: &str, auto_model: &str) -> String {
    if model.eq_ignore_ascii_case(auto_model) {
        return "Recommended lower-cost default for the active provider.".to_string();
    }

    let provider_label = provider.label();
    format!(
        "Explicit {} lightweight route for faster, cheaper memory and suggestion tasks.",
        provider_label
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_entries_use_raw_model_ids() {
        let entries = model_entries(&["gpt-5.4".to_owned(), "claude-sonnet-4".to_owned()]);

        assert_eq!(entries[0].title, "gpt-5.4");
        assert_eq!(entries[1].title, "claude-sonnet-4");
    }
}
