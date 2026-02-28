use std::collections::HashSet;
use std::fmt;
use std::io::{self, Write};
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use vtcode_core::config::constants::{model_helpers, models};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::dot_config::WorkspaceTrustLevel;

use crate::interactive_list::{SelectionEntry, SelectionInterrupted, run_interactive_selection};

pub(super) fn resolve_initial_provider(config: &VTCodeConfig) -> Provider {
    let configured = config.agent.provider.trim();
    let fallback = Provider::from_str(vtcode_core::config::constants::defaults::DEFAULT_PROVIDER)
        .unwrap_or(Provider::OpenAI);

    if configured.is_empty() {
        fallback
    } else {
        Provider::from_str(configured).unwrap_or(fallback)
    }
}

pub(super) fn prompt_provider(renderer: &mut AnsiRenderer, default: Provider) -> Result<Provider> {
    renderer.line(MessageStyle::Status, "Choose your default provider:")?;
    let providers = Provider::all_providers();

    match select_provider_with_ratatui(&providers, default) {
        Ok(provider) => Ok(provider),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

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
    if providers.is_empty() {
        return Err(anyhow!("No providers available for selection"));
    }

    let mut entries: Vec<SelectionEntry> = Vec::with_capacity(providers.len());
    for (index, provider) in providers.iter().enumerate() {
        entries.push(SelectionEntry::new(
            format!("{:>2}. {}", index + 1, provider.label()),
            None,
        ));
    }

    let default_index = providers
        .iter()
        .position(|provider| *provider == default)
        .unwrap_or(0);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default.label()
    );

    let selection = run_interactive_selection("Providers", &instructions, &entries, default_index);
    let selected_index = match selection {
        Ok(Some(index)) => index,
        Ok(None) => default_index.min(entries.len() - 1),
        Err(err) => {
            if err.is::<SelectionInterrupted>() {
                return Err(SetupInterrupted.into());
            }
            return Err(err);
        }
    };
    Ok(providers[selected_index])
}

#[derive(Debug)]
struct SetupInterrupted;

impl fmt::Display for SetupInterrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("setup interrupted by Ctrl+C")
    }
}

impl std::error::Error for SetupInterrupted {}

pub(super) fn prompt_model(
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
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_model_text(renderer, provider, default_model, &options)
        }
    }
}

pub(super) fn prompt_reasoning_effort(
    renderer: &mut AnsiRenderer,
    default: ReasoningEffortLevel,
) -> Result<ReasoningEffortLevel> {
    renderer.line(
        MessageStyle::Status,
        "Choose reasoning effort level for models that support it:",
    )?;

    let levels = [
        (
            ReasoningEffortLevel::Low,
            "Low – faster responses, less reasoning",
        ),
        (
            ReasoningEffortLevel::Medium,
            "Medium – balanced speed and reasoning (recommended)",
        ),
        (
            ReasoningEffortLevel::High,
            "High – deeper reasoning, slower responses",
        ),
    ];

    match select_reasoning_with_ratatui(&levels, default) {
        Ok(level) => Ok(level),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_reasoning_effort_text(renderer, &levels, default)
        }
    }
}

fn select_reasoning_with_ratatui(
    levels: &[(ReasoningEffortLevel, &str)],
    default: ReasoningEffortLevel,
) -> Result<ReasoningEffortLevel> {
    let entries: Vec<SelectionEntry> = levels
        .iter()
        .enumerate()
        .map(|(index, (_level, label))| {
            SelectionEntry::new(format!("{:>2}. {}", index + 1, label), None)
        })
        .collect();

    let default_index = levels
        .iter()
        .position(|(level, _)| *level == default)
        .unwrap_or(1);

    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default.as_str()
    );

    let selection =
        run_interactive_selection("Reasoning Effort", &instructions, &entries, default_index);
    let selected_index = match selection {
        Ok(Some(index)) => index,
        Ok(None) => default_index.min(entries.len() - 1),
        Err(err) => {
            if err.is::<SelectionInterrupted>() {
                return Err(SetupInterrupted.into());
            }
            return Err(err);
        }
    };
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
            "Please choose a valid reasoning effort level (low, medium, high).",
        )?;
    }
}

pub(super) fn prompt_trust(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    renderer.line(
        MessageStyle::Status,
        "Workspace trust determines which actions are allowed.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [1] Tools policy – prompts before running elevated actions (recommended)",
    )?;
    renderer.line(
        MessageStyle::Info,
        "  [2] Full auto – allow unattended execution without prompts",
    )?;

    match select_trust_with_ratatui(default) {
        Ok(level) => Ok(level),
        Err(error) => {
            if error.is::<SetupInterrupted>() {
                return Err(error);
            }

            renderer.line(
                MessageStyle::Info,
                &format!("Falling back to manual input ({error})."),
            )?;
            prompt_trust_text(renderer, default)
        }
    }
}

fn prompt_with_placeholder(prompt: &str) -> Result<String> {
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
    if options.is_empty() {
        return Err(anyhow!("No models available for selection"));
    }

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

    let selection = run_interactive_selection("Models", &instructions, &entries, default_index);
    let selected_index = match selection {
        Ok(Some(index)) => index,
        Ok(None) => default_index.min(entries.len() - 1),
        Err(err) => {
            if err.is::<SelectionInterrupted>() {
                return Err(SetupInterrupted.into());
            }
            return Err(err);
        }
    };

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

fn prompt_trust_text(
    renderer: &mut AnsiRenderer,
    default: WorkspaceTrustLevel,
) -> Result<WorkspaceTrustLevel> {
    let default_choice = match default {
        WorkspaceTrustLevel::ToolsPolicy => "1",
        WorkspaceTrustLevel::FullAuto => "2",
    };

    loop {
        let input = prompt_with_placeholder(&format!("Trust level [{}]", default_choice))?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default);
        }

        match trimmed {
            "1" | "tools" | "tool" => return Ok(WorkspaceTrustLevel::ToolsPolicy),
            "2" | "full" | "auto" | "full-auto" => return Ok(WorkspaceTrustLevel::FullAuto),
            _ => {
                renderer.line(
                    MessageStyle::Error,
                    "Please choose 1 for Tools policy or 2 for Full auto.",
                )?;
            }
        }
    }
}

fn select_trust_with_ratatui(default: WorkspaceTrustLevel) -> Result<WorkspaceTrustLevel> {
    let entries = Vec::from([
        (
            WorkspaceTrustLevel::ToolsPolicy,
            SelectionEntry::new(
                " 1. Tools policy – prompts before running elevated actions (recommended)"
                    .to_owned(),
                Some(
                    "Tools policy – prompts before running elevated actions (recommended)"
                        .to_owned(),
                ),
            ),
        ),
        (
            WorkspaceTrustLevel::FullAuto,
            SelectionEntry::new(
                " 2. Full auto – allow unattended execution without prompts".to_owned(),
                Some("Full auto – allow unattended execution without prompts".to_owned()),
            ),
        ),
    ]);

    let default_index = match default {
        WorkspaceTrustLevel::ToolsPolicy => 0,
        WorkspaceTrustLevel::FullAuto => 1,
    };

    let mut selection_entries: Vec<SelectionEntry> = Vec::with_capacity(entries.len());
    for (_lvl, entry) in &entries {
        selection_entries.push(entry.clone());
    }
    let default_entry = &selection_entries[default_index];
    let default_summary = default_entry
        .description
        .as_deref()
        .unwrap_or(default_entry.title.as_str());
    let instructions = format!(
        "Default: {}. Use ↑/↓ or j/k to choose, Enter to confirm, Esc to keep the default.",
        default_summary
    );

    let selection = run_interactive_selection(
        "Workspace trust",
        &instructions,
        &selection_entries,
        default_index,
    );
    let selected_index = match selection {
        Ok(Some(index)) => index,
        Ok(None) => default_index,
        Err(err) => {
            if err.is::<SelectionInterrupted>() {
                return Err(SetupInterrupted.into());
            }
            return Err(err);
        }
    };
    Ok(entries[selected_index].0)
}

pub(super) fn default_model_for_provider(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => models::google::DEFAULT_MODEL,
        Provider::OpenAI => models::openai::DEFAULT_MODEL,
        Provider::Anthropic => models::anthropic::DEFAULT_MODEL,
        Provider::DeepSeek => models::deepseek::DEFAULT_MODEL,
        Provider::HuggingFace => models::huggingface::DEFAULT_MODEL,
        Provider::OpenRouter => models::openrouter::DEFAULT_MODEL,
        Provider::Ollama => models::ollama::DEFAULT_MODEL,
        Provider::Moonshot => models::minimax::MINIMAX_M2_5,
        Provider::ZAI => models::zai::DEFAULT_MODEL,
        Provider::Minimax => models::minimax::MINIMAX_M2_5,
    }
}
