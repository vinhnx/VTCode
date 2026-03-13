use hashbrown::HashMap;

use anyhow::Result;

use vtcode_core::config::constants::ui;
use vtcode_core::config::models::{ModelId, Provider};
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::config::validation::effective_model_context_window;
use vtcode_core::ui::{InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::dynamic_models::DynamicModelRegistry;
use super::options::{ModelOption, picker_provider_order};

mod prompts;
pub(super) use prompts::{
    prompt_api_key_plain, prompt_custom_model_entry, prompt_reasoning_plain,
    prompt_service_tier_plain, render_reasoning_inline, render_service_tier_inline,
    show_secure_api_modal,
};

pub(super) const CLOSE_THEME_MESSAGE: &str =
    "Close the active model picker before selecting a theme.";
const STEP_ONE_TITLE: &str = "Model";
const STEP_TWO_TITLE: &str = "Reasoning";
const STEP_THREE_TITLE: &str = "Service Tier";

pub(super) const CUSTOM_PROVIDER_TITLE: &str = "Custom provider + model";
pub(super) const CUSTOM_PROVIDER_SUBTITLE: &str =
    "Provide the provider name and model identifier manually.";
const CUSTOM_PROVIDER_BADGE: &str = "Manual";
const REASONING_OFF_BADGE: &str = "No reasoning";
const CURRENT_BADGE: &str = "Current";
const CONTEXT_LABEL: &str = "Context";
const TOOLS_LABEL: &str = "Tools";
const NO_TOOLS_LABEL: &str = "No tools";

pub(super) const KEEP_CURRENT_DESCRIPTION: &str = "Retain the existing reasoning configuration.";

pub(super) fn model_search_value(
    provider: Provider,
    model_display: &str,
    model_id: &str,
    description: Option<&str>,
    extra_terms: &[String],
) -> String {
    let provider_label = provider.label();
    let provider_key = provider.to_string();
    let provider_model_name = format!("{provider_key} {model_display}");
    let provider_model_id = format!("{provider_key}/{model_id}");
    let mut value = format!(
        "{} {} {} {} {} {}",
        provider_label,
        provider_key,
        model_display,
        model_id,
        provider_model_name,
        provider_model_id
    );
    if let Some(description_text) = description {
        value.push(' ');
        value.push_str(description_text);
    }
    for term in extra_terms {
        if !term.trim().is_empty() {
            value.push(' ');
            value.push_str(term);
        }
    }
    value
}

fn is_current_model(
    provider: Provider,
    model_id: &str,
    current_provider: &str,
    current_model: &str,
) -> bool {
    provider
        .to_string()
        .eq_ignore_ascii_case(current_provider.trim())
        && model_id.eq_ignore_ascii_case(current_model.trim())
}

fn input_modalities_label(input_modalities: &[&str]) -> Option<String> {
    if input_modalities.is_empty() {
        return None;
    }

    Some(format!("Input: {}", input_modalities.join(", ")))
}

fn compact_context_window_label(context_window_size: usize) -> String {
    if context_window_size >= 1_000_000 {
        format!("{}M", context_window_size / 1_000_000)
    } else if context_window_size >= 1_000 {
        format!("{}K", context_window_size / 1_000)
    } else {
        context_window_size.to_string()
    }
}

fn context_window_segment(provider: &str, model_id: &str) -> Option<String> {
    effective_model_context_window(provider, model_id)
        .ok()
        .flatten()
        .filter(|context_window_size| *context_window_size > 0)
        .map(|context_window_size| {
            format!(
                "{}: {}",
                CONTEXT_LABEL,
                compact_context_window_label(context_window_size)
            )
        })
}

fn static_model_capability_segments(option: &ModelOption) -> Vec<String> {
    let mut segments = Vec::new();
    let provider_key = option.provider.to_string();
    if let Some(context_window) = context_window_segment(&provider_key, option.id) {
        segments.push(context_window);
    }

    if option.supports_reasoning {
        segments.push("Reasoning".to_string());
    }

    segments.push(if option.model.supports_tool_calls() {
        TOOLS_LABEL.to_string()
    } else {
        NO_TOOLS_LABEL.to_string()
    });

    if let Some(modalities) = input_modalities_label(option.model.input_modalities()) {
        segments.push(modalities);
    }

    segments
}

pub(super) fn static_model_search_terms(model: ModelId, supports_reasoning: bool) -> Vec<String> {
    let mut terms = Vec::new();
    if supports_reasoning {
        terms.push("reasoning".to_string());
    }

    if model.supports_tool_calls() {
        terms.push("tools".to_string());
        terms.push("tool_call".to_string());
        terms.push("toolcall".to_string());
        terms.push("tool calling".to_string());
    } else {
        terms.push("no tools".to_string());
        terms.push("no-tools".to_string());
        terms.push("tool_call disabled".to_string());
    }

    let modalities = model.input_modalities();
    if !modalities.is_empty() {
        terms.push(format!("input {}", modalities.join(" ")));
        terms.push("modalities".to_string());
        terms.extend(modalities.iter().map(|modality| (*modality).to_string()));
    }

    terms
}

fn subtitle_from_segments(model_id: &str, current: bool, segments: Vec<String>) -> String {
    let mut subtitle = vec![model_id.to_string()];
    if current {
        subtitle.push(CURRENT_BADGE.to_string());
    }
    subtitle.extend(segments);
    subtitle.join(" • ")
}

pub(super) fn static_model_subtitle(
    option: &ModelOption,
    current_provider: &str,
    current_model: &str,
) -> String {
    subtitle_from_segments(
        option.id,
        is_current_model(option.provider, option.id, current_provider, current_model),
        static_model_capability_segments(option),
    )
}

pub(super) fn dynamic_model_subtitle(
    provider: Provider,
    model_id: &str,
    reasoning_supported: bool,
    current_provider: &str,
    current_model: &str,
) -> String {
    let mut segments = Vec::new();
    let provider_key = provider.to_string();
    if let Some(context_window) = context_window_segment(&provider_key, model_id) {
        segments.push(context_window);
    }
    if provider.is_local() {
        segments.push("Local".to_string());
    }
    if reasoning_supported {
        segments.push("Reasoning".to_string());
    }

    subtitle_from_segments(
        model_id,
        is_current_model(provider, model_id, current_provider, current_model),
        segments,
    )
}

pub(super) fn current_model_line(current_provider: &str, current_model: &str) -> String {
    if current_provider.trim().is_empty() || current_model.trim().is_empty() {
        return "Pick a model provider and model id.".to_string();
    }

    let base = format!("Current: {} / {}", current_provider, current_model);
    if let Some(context_window) = context_window_segment(current_provider, current_model) {
        format!("{base} • {context_window}")
    } else {
        base
    }
}

pub(super) fn render_step_one_inline(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    _current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
    selected: Option<InlineListSelection>,
    current_provider: &str,
    current_model: &str,
) -> Result<()> {
    let mut items = Vec::new();
    for provider in picker_provider_order() {
        let provider_models: Vec<(usize, &ModelOption)> = options
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.provider == provider)
            .collect();
        let dynamic_indexes = dynamic_models.indexes_for(provider);
        let has_error = dynamic_models.error_for(provider).is_some();
        let has_warning = dynamic_models.warning_for(provider).is_some();

        if provider_models.is_empty() && dynamic_indexes.is_empty() && !has_error && !has_warning {
            continue;
        }

        for (idx, option) in &provider_models {
            items.push(InlineListItem {
                title: option.display.to_string(),
                subtitle: Some(static_model_subtitle(
                    option,
                    current_provider,
                    current_model,
                )),
                badge: Some(provider.label().to_string()),
                indent: 0,
                selection: Some(InlineListSelection::Model(*idx)),
                search_value: Some(model_search_value(
                    provider,
                    option.display,
                    option.id,
                    Some(option.description),
                    &static_model_search_terms(option.model, option.supports_reasoning),
                )),
            });
        }

        if provider.is_dynamic() {
            for entry_index in &dynamic_indexes {
                if let Some(detail) = dynamic_models.detail(*entry_index) {
                    let extra_terms = {
                        let mut terms = Vec::new();
                        if provider.is_local() {
                            terms.push("local".to_string());
                        }
                        if detail.reasoning_supported {
                            terms.push("reasoning".to_string());
                        }
                        terms
                    };
                    items.push(InlineListItem {
                        title: detail.model_display.clone(),
                        subtitle: Some(dynamic_model_subtitle(
                            provider,
                            &detail.model_id,
                            detail.reasoning_supported,
                            current_provider,
                            current_model,
                        )),
                        badge: Some(provider.label().to_string()),
                        indent: 0,
                        selection: Some(InlineListSelection::DynamicModel(*entry_index)),
                        search_value: Some(model_search_value(
                            provider,
                            &detail.model_display,
                            &detail.model_id,
                            None,
                            &extra_terms,
                        )),
                    });
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                items.push(InlineListItem {
                    title: format!("{} cache notice", provider.label()),
                    subtitle: Some(warning.to_string()),
                    badge: Some("Action".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::RefreshDynamicModels),
                    search_value: Some(format!("{} cache", provider.label())),
                });
            }

            if dynamic_indexes.is_empty()
                && let Some(error) = dynamic_models.error_for(provider)
            {
                items.push(InlineListItem {
                    title: format!("{} unavailable", provider.label()),
                    subtitle: Some(error.to_string()),
                    badge: Some("Action".to_string()),
                    indent: 0,
                    selection: Some(InlineListSelection::RefreshDynamicModels),
                    search_value: Some(format!("{} setup", provider.label().to_ascii_lowercase())),
                });
            }
        } else if provider == Provider::HuggingFace && provider_models.is_empty() {
            items.push(InlineListItem {
                title: "Custom Hugging Face model".to_string(),
                subtitle: Some(
                    "Enter any HF model id (e.g., huggingface <org>/<model>)".to_string(),
                ),
                badge: Some("Custom".to_string()),
                indent: 0,
                selection: Some(InlineListSelection::CustomModel),
                search_value: Some("huggingface custom".to_string()),
            });
        }
    }

    items.push(InlineListItem {
        title: "Refresh local LM Studio/Ollama models".to_string(),
        subtitle: Some(
            "Re-query LM Studio and Ollama servers without closing the picker.".to_string(),
        ),
        badge: Some("Action".to_string()),
        indent: 0,
        selection: Some(InlineListSelection::RefreshDynamicModels),
        search_value: Some("refresh local models".to_string()),
    });

    items.push(InlineListItem {
        title: CUSTOM_PROVIDER_TITLE.to_string(),
        subtitle: Some(CUSTOM_PROVIDER_SUBTITLE.to_string()),
        badge: Some(CUSTOM_PROVIDER_BADGE.to_string()),
        indent: 0,
        selection: Some(InlineListSelection::CustomModel),
        search_value: Some("custom provider".to_string()),
    });

    let lines = vec![
        current_model_line(current_provider, current_model),
        "↑/↓ select • Enter choose • Esc cancel".to_string(),
    ];

    let search = InlineListSearchConfig {
        label: "Search models".to_string(),
        placeholder: Some("provider, name, id, or capability".to_string()),
    };
    renderer.show_list_modal(STEP_ONE_TITLE, lines, items, selected, Some(search));

    Ok(())
}

pub(super) fn render_step_one_plain(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    dynamic_models: &DynamicModelRegistry,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Model picker: select the model you want to use.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type '<provider> <model-id>' to select a model.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'refresh' to re-query LM Studio and Ollama servers.",
    )?;

    let mut grouped: HashMap<Provider, Vec<&ModelOption>> = HashMap::new();
    for option in options {
        grouped.entry(option.provider).or_default().push(option);
    }

    let mut first_section = true;
    for provider in picker_provider_order() {
        if provider.is_local() {
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            if let Some(list) = grouped.get(&provider) {
                for option in list {
                    renderer.line(MessageStyle::Info, &format!("  {}", option.display))?;
                    renderer.line(
                        MessageStyle::Info,
                        &format!("      {}", static_model_subtitle(option, "", "")),
                    )?;
                    renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                renderer.line(MessageStyle::Info, &format!("      note: {}", warning))?;
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            let provider_label = provider.label();
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    renderer.line(
                        MessageStyle::Info,
                        &format!(
                            "{} server not reachable ({error}) • Setup instructions:",
                            provider_label
                        ),
                    )?;
                    for line in provider.local_install_instructions().unwrap_or("").lines() {
                        renderer.line(MessageStyle::Info, &format!("      {}", line))?;
                    }
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        renderer
                            .line(MessageStyle::Info, &format!("  {}", detail.model_display))?;
                        renderer.line(
                            MessageStyle::Info,
                            &format!(
                                "      {}",
                                dynamic_model_subtitle(
                                    provider,
                                    &detail.model_id,
                                    detail.reasoning_supported,
                                    "",
                                    "",
                                )
                            ),
                        )?;
                        renderer.line(
                            MessageStyle::Info,
                            &format!("      Locally available {} model", provider_label),
                        )?;
                    }
                }
            }
        } else if provider == Provider::HuggingFace {
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            renderer.line(
                MessageStyle::Info,
                "      Docs: https://huggingface.co/docs/inference-providers",
            )?;
            if let Some(list) = grouped.get(&provider) {
                for option in list {
                    renderer.line(MessageStyle::Info, &format!("  {}", option.display))?;
                    renderer.line(
                        MessageStyle::Info,
                        &format!("      {}", static_model_subtitle(option, "", "")),
                    )?;
                    renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
                }
            }
        } else {
            let Some(list) = grouped.get(&provider) else {
                continue;
            };
            if !first_section {
                renderer.line(MessageStyle::Info, &provider_group_divider_line())?;
            }
            first_section = false;
            renderer.line(MessageStyle::Info, &format!("[{}]", provider.label()))?;
            for option in list {
                renderer.line(MessageStyle::Info, &format!("  {}", option.display))?;
                renderer.line(
                    MessageStyle::Info,
                    &format!("      {}", static_model_subtitle(option, "", "")),
                )?;
                renderer.line(MessageStyle::Info, &format!("      {}", option.description))?;
            }
        }
    }

    Ok(())
}

fn provider_group_divider_line() -> String {
    let modal_width = usize::from(ui::MODAL_MIN_WIDTH);
    let title_width = STEP_ONE_TITLE.chars().count();
    let divider_width = modal_width.max(title_width);
    ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(divider_width)
}
