use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;

use vtcode_core::config::constants::ui;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSearchConfig, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::dynamic_models::DynamicModelRegistry;
use super::options::{ModelOption, picker_provider_order};
use super::selection::{SelectionDetail, reasoning_level_description, reasoning_level_label};

pub(super) const CLOSE_THEME_MESSAGE: &str =
    "Close the active model picker before selecting a theme.";
const STEP_ONE_TITLE: &str = "Model picker – Step 1";
const STEP_TWO_TITLE: &str = "Model picker – Step 2";
const STEP_ONE_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to select, or Esc to cancel.";
const STEP_TWO_NAVIGATION_HINT: &str = "Use ↑/↓ to navigate, Enter to choose, or Esc to cancel.";
pub(super) const CUSTOM_PROVIDER_TITLE: &str = "Custom provider + model";
pub(super) const CUSTOM_PROVIDER_SUBTITLE: &str =
    "Provide the provider name and model identifier manually.";
const CUSTOM_PROVIDER_BADGE: &str = "Manual";
const REASONING_BADGE: &str = "Reasoning";
const REASONING_OFF_BADGE: &str = "No reasoning";
const CURRENT_BADGE: &str = "Current";
const CURRENT_REASONING_PREFIX: &str = "Current reasoning effort: ";
pub(super) const KEEP_CURRENT_DESCRIPTION: &str = "Retain the existing reasoning configuration.";

pub(super) fn render_step_one_inline(
    renderer: &mut AnsiRenderer,
    options: &[ModelOption],
    current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
) -> Result<()> {
    let mut items = Vec::new();
    let mut first_section = true;
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

        if !first_section {
            items.push(provider_group_divider_item());
        }
        first_section = false;
        items.push(InlineListItem {
            title: provider.label().to_string(),
            subtitle: None,
            badge: None,
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
                indent: 2,
                selection: Some(InlineListSelection::Model(*idx)),
                search_value: Some(format!("{} {}", provider.label(), option.display)),
            });
        }

        if matches!(provider, Provider::LmStudio | Provider::Ollama) {
            let subtitle = if provider == Provider::LmStudio {
                "Locally available LM Studio model"
            } else {
                "Locally available Ollama model"
            };
            for entry_index in &dynamic_indexes {
                if let Some(detail) = dynamic_models.detail(*entry_index) {
                    items.push(InlineListItem {
                        title: detail.model_display.clone(),
                        subtitle: Some(subtitle.to_string()),
                        badge: Some("Local".to_string()),
                        indent: 2,
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
                    indent: 2,
                    selection: Some(InlineListSelection::RefreshDynamicModels),
                    search_value: Some(format!("{} cache", provider.label())),
                });
            }

            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    let instructions = if provider == Provider::LmStudio {
                        get_lmstudio_setup_instructions()
                    } else {
                        get_ollama_setup_instructions()
                    };
                    items.push(InlineListItem {
                        title: format!("{} server unreachable", provider.label()),
                        subtitle: Some(format!("{error}\n{instructions}")),
                        badge: Some("Info".to_string()),
                        indent: 2,
                        selection: Some(InlineListSelection::CustomModel),
                        search_value: Some(format!(
                            "{} setup",
                            provider.label().to_ascii_lowercase()
                        )),
                    });
                }
            }
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

    let mut lines = vec![
        "Step 1 – choose a model".to_string(),
        STEP_ONE_NAVIGATION_HINT.to_string(),
    ];
    lines.push(format!(
        "{}{}",
        CURRENT_REASONING_PREFIX,
        reasoning_level_label(current_reasoning)
    ));

    let search = InlineListSearchConfig {
        label: "Search models or providers".to_string(),
        placeholder: Some("Type to filter models".to_string()),
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
        "Model picker – Step 1: select the model you want to use.",
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
        if provider == Provider::LmStudio {
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
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("LM Studio server not reachable ({error}) • Setup instructions:"),
                    )?;
                    for line in get_lmstudio_setup_instructions().lines() {
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
                            "      Locally available LM Studio model",
                        )?;
                    }
                }
            }
        } else if provider == Provider::Ollama {
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
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Ollama server not reachable ({error}) • Setup instructions:"),
                    )?;
                    for line in get_ollama_setup_instructions().lines() {
                        renderer.line(MessageStyle::Info, &format!("      {}", line))?;
                    }
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        renderer.line(
                            MessageStyle::Info,
                            &format!("  {} • {} (local)", detail.model_display, detail.model_id),
                        )?;
                        renderer
                            .line(MessageStyle::Info, "      Locally available Ollama model")?;
                    }
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

pub(super) fn render_reasoning_inline(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    let mut items = Vec::new();
    items.push(InlineListItem {
        title: format!("Keep current ({})", reasoning_level_label(current)),
        subtitle: Some(KEEP_CURRENT_DESCRIPTION.to_string()),
        badge: Some(CURRENT_BADGE.to_string()),
        indent: 0,
        selection: Some(InlineListSelection::Reasoning(current)),
        search_value: None,
    });
    for level in [
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
    ] {
        items.push(InlineListItem {
            title: reasoning_level_label(level).to_string(),
            subtitle: Some(reasoning_level_description(level).to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Reasoning(level)),
            search_value: None,
        });
    }
    if let Some(alternative) = selection.reasoning_off_model {
        items.push(InlineListItem {
            title: format!("Use {} (reasoning off)", alternative.display_name()),
            subtitle: Some(format!(
                "Switch to {} ({}) without enabling structured reasoning.",
                alternative.display_name(),
                alternative.as_str()
            )),
            badge: Some(REASONING_OFF_BADGE.to_string()),
            indent: 0,
            selection: Some(InlineListSelection::DisableReasoning),
            search_value: None,
        });
    }
    let mut lines = vec![
        format!(
            "Step 2 – select reasoning effort for {}.",
            selection.model_display
        ),
        STEP_TWO_NAVIGATION_HINT.to_string(),
    ];
    if let Some(alternative) = selection.reasoning_off_model {
        lines.push(format!(
            "Select \"Use {} (reasoning off)\" to switch to {}.",
            alternative.display_name(),
            alternative.as_str()
        ));
    }
    renderer.show_list_modal(
        STEP_TWO_TITLE,
        lines,
        items,
        Some(InlineListSelection::Reasoning(current)),
        None,
    );
    Ok(())
}

pub(super) fn prompt_reasoning_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    if selection.reasoning_optional {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – reasoning effort (current: {}). Choose easy/medium/hard or type 'skip' if the model does not expose configurable reasoning.",
                current
            ),
        )?;
    } else if let Some(alternative) = selection.reasoning_off_model {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} (easy/medium/hard). Type 'skip' to keep {} or 'off' to use {} ({}).",
                selection.model_display,
                reasoning_level_label(current),
                alternative.display_name(),
                alternative.as_str()
            ),
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} (easy/medium/hard). Type 'skip' to keep {}. Current: {}.",
                selection.model_display,
                reasoning_level_label(current),
                current
            ),
        )?;
    }
    Ok(())
}

pub(super) fn prompt_api_key_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    workspace: Option<&Path>,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Step 3 – enter an API key for {} (env: {}).",
            selection.provider_label, selection.env_key
        ),
    )?;
    if let Some(root) = workspace {
        let env_path = root.join(".env");
        renderer.line(
            MessageStyle::Info,
            &format!("The key will be stored in {}.", env_path.display()),
        )?;
    } else {
        renderer.line(
            MessageStyle::Info,
            "The key will be stored in your workspace .env file.",
        )?;
    }
    renderer.line(
        MessageStyle::Info,
        "Paste the API key now or type 'skip' to reuse a stored credential.",
    )?;
    Ok(())
}

pub(super) fn show_secure_api_modal(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    workspace: Option<&Path>,
) {
    let storage_line = workspace
        .map(|root| {
            let env_path = root.join(".env");
            format!("Stored in {}.", env_path.display())
        })
        .unwrap_or_else(|| "Stored in workspace .env file.".to_string());
    let mask_preview = "●●●●●●";
    let lines = vec![
        format!(
            "Bring your own key (BYOK) for {}.",
            selection.provider_label
        ),
        format!("Secure display hint: {}", mask_preview),
        storage_line,
        "Paste the key and press Enter when ready.".to_string(),
    ];
    let prompt_label = format!("{} API key", selection.provider_label);
    renderer.show_secure_prompt_modal("Secure API key setup", lines, prompt_label);
}

pub(super) fn prompt_custom_model_entry(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Enter a provider and model identifier (examples: 'openai gpt-5-nano', 'ollama qwen3:1.7b').",
    )?;
    renderer.line(
        MessageStyle::Info,
        "For Ollama, you can use any locally available model like 'llama3:8b', 'mistral:7b', etc.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'cancel' to exit the picker at any time.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "Type 'refresh' to reload LM Studio and Ollama model lists.",
    )?;
    Ok(())
}

fn provider_group_divider_item() -> InlineListItem {
    InlineListItem {
        title: provider_group_divider_line(),
        subtitle: None,
        badge: None,
        indent: 0,
        selection: None,
        search_value: None,
    }
}

fn provider_group_divider_line() -> String {
    let modal_width = usize::from(ui::MODAL_MIN_WIDTH);
    let title_width = STEP_ONE_TITLE.chars().count();
    let divider_width = modal_width.max(title_width);
    ui::INLINE_USER_MESSAGE_DIVIDER_SYMBOL.repeat(divider_width)
}

pub(super) fn get_lmstudio_setup_instructions() -> String {
    "LM Studio server is not running. To start:\n  1. Download and install LM Studio from https://lmstudio.ai\n  2. Launch LM Studio\n  3. Click the 'Local Server' toggle to start the server\n  4. Select and load a model in the 'Local Server' tab\n  5. Make sure the server runs on port 1234 (default)"
        .to_string()
}

pub(super) fn get_ollama_setup_instructions() -> String {
    "Ollama server is not running. To start:\n  1. Install Ollama from https://ollama.com\n  2. Run 'ollama serve' in a terminal\n  3. Pull models using 'ollama pull <model-name>' (e.g., 'ollama pull llama3:8b')"
        .to_string()
}
