use std::collections::HashMap;

use anyhow::Result;

use vtcode_core::config::constants::ui;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::dynamic_models::DynamicModelRegistry;
use super::options::{ModelOption, picker_provider_order};

mod prompts;
pub(super) use prompts::{
    prompt_api_key_plain, prompt_custom_model_entry, prompt_reasoning_plain,
    render_reasoning_inline, show_secure_api_modal,
};

pub(super) const CLOSE_THEME_MESSAGE: &str =
    "Close the active model picker before selecting a theme.";
const STEP_ONE_TITLE: &str = "Model picker #1";
const STEP_TWO_TITLE: &str = "Model picker #2";

pub(super) const CUSTOM_PROVIDER_TITLE: &str = "Custom provider + model";
pub(super) const CUSTOM_PROVIDER_SUBTITLE: &str =
    "Provide the provider name and model identifier manually.";
const CUSTOM_PROVIDER_BADGE: &str = "Manual";
const PROVIDER_BADGE: &str = "Provider";
const REASONING_BADGE: &str = "Reasoning";
const REASONING_OFF_BADGE: &str = "No reasoning";
const CURRENT_BADGE: &str = "Current";

pub(super) const KEEP_CURRENT_DESCRIPTION: &str = "Retain the existing reasoning configuration.";

pub(super) fn render_step_one_inline(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    _current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
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

        items.push(InlineListItem {
            title: provider.label().to_string(),
            subtitle: None,
            badge: Some(PROVIDER_BADGE.to_string()),
            indent: 0,
            selection: None,
            search_value: Some(provider.label().to_string()),
        });

        for (idx, option) in &provider_models {
            let badge = option
                .supports_reasoning
                .then(|| REASONING_BADGE.to_string());
            items.push(InlineListItem {
                title: option.display.to_string(),
                subtitle: Some(option.description.to_string()),
                badge,
                indent: 1,
                selection: Some(InlineListSelection::Model(*idx)),
                search_value: Some(format!("{} {}", provider.label(), option.display)),
            });
        }

        if provider.is_dynamic() {
            let subtitle = if provider.is_local() {
                format!("Locally available {} model", provider.label())
            } else {
                format!("Available {} model", provider.label())
            };
            for entry_index in &dynamic_indexes {
                if let Some(detail) = dynamic_models.detail(*entry_index) {
                    items.push(InlineListItem {
                        title: detail.model_display.clone(),
                        subtitle: Some(subtitle.to_string()),
                        badge: if provider.is_local() {
                            Some("Local".to_string())
                        } else {
                            None
                        },
                        indent: 1,
                        selection: Some(InlineListSelection::DynamicModel(*entry_index)),
                        search_value: Some(format!(
                            "{} {}",
                            provider.label(),
                            detail.model_display
                        )),
                    });
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                items.push(InlineListItem {
                    title: format!("{} cache notice", provider.label()),
                    subtitle: Some(warning.to_string()),
                    badge: Some("Info".to_string()),
                    indent: 1,
                    selection: Some(InlineListSelection::RefreshDynamicModels),
                    search_value: Some(format!("{} cache", provider.label())),
                });
            }

            if dynamic_indexes.is_empty()
                && let Some(error) = dynamic_models.error_for(provider)
            {
                let instructions = provider.local_install_instructions().unwrap_or("");
                items.push(InlineListItem {
                    title: format!("{} server unreachable", provider.label()),
                    subtitle: Some(format!("{error}\n{instructions}")),
                    badge: Some("Info".to_string()),
                    indent: 1,
                    selection: Some(InlineListSelection::CustomModel),
                    search_value: Some(format!("{} setup", provider.label().to_ascii_lowercase())),
                });
            }
        } else if provider == Provider::HuggingFace {
            items.push(InlineListItem {
                title: "Hugging Face Inference Providers".to_string(),
                subtitle: Some(
                    "OpenAI-compatible router. Docs: https://huggingface.co/docs/inference-providers"
                        .to_string(),
                ),
                badge: Some("Docs".to_string()),
                indent: 1,
                selection: None,
                search_value: Some("huggingface docs".to_string()),
            });
            items.push(InlineListItem {
                title: "Set HF_TOKEN in environment".to_string(),
                subtitle: Some("Required for Hugging Face router authentication".to_string()),
                badge: Some("Tip".to_string()),
                indent: 1,
                selection: None,
                search_value: Some("huggingface hf_token".to_string()),
            });
            items.push(InlineListItem {
                title: "Custom Hugging Face model".to_string(),
                subtitle: Some(
                    "Enter any HF model id (e.g., huggingface <org>/<model>)".to_string(),
                ),
                badge: Some("Custom".to_string()),
                indent: 1,
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

    let lines = vec![];

    let search = InlineListSearchConfig {
        label: "Search models or providers".to_string(),
        placeholder: Some("Filter models/providers".to_string()),
    };
    renderer.show_list_modal(STEP_ONE_TITLE, lines, items, None, Some(search));

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
                    let reasoning_marker = if option.supports_reasoning {
                        " [reasoning]"
                    } else {
                        ""
                    };
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
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
                        renderer.line(
                            MessageStyle::Info,
                            &format!("  {} • {} (dynamic)", detail.model_display, detail.model_id),
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
                    let reasoning_marker = if option.supports_reasoning {
                        " [reasoning]"
                    } else {
                        ""
                    };
                    renderer.line(
                        MessageStyle::Info,
                        &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
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
                let reasoning_marker = if option.supports_reasoning {
                    " [reasoning]"
                } else {
                    ""
                };
                renderer.line(
                    MessageStyle::Info,
                    &format!("  {} • {}{}", option.display, option.id, reasoning_marker),
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
