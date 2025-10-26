use anyhow::{Result, anyhow};

use vtcode::interactive_list::{SelectionEntry, run_interactive_selection};
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;

use super::dynamic_models::DynamicModelRegistry;
use super::options::{ModelOption, picker_provider_order};
use super::rendering::{
    CUSTOM_PROVIDER_SUBTITLE, CUSTOM_PROVIDER_TITLE, KEEP_CURRENT_DESCRIPTION,
    get_lmstudio_setup_instructions, get_ollama_setup_instructions,
};
use super::selection::{
    ReasoningChoice, SelectionDetail, reasoning_level_description, reasoning_level_label,
    selection_from_option,
};

pub(super) const REFRESH_ENTRY_LABEL: &str = "Refresh local LM Studio/Ollama models";

#[derive(Clone)]
pub(super) struct ModelSelectionChoice {
    pub(super) entry: SelectionEntry,
    pub(super) outcome: ModelSelectionChoiceOutcome,
}

#[derive(Clone)]
pub(super) enum ModelSelectionChoiceOutcome {
    Predefined(SelectionDetail),
    Manual,
    Refresh,
}

pub(super) enum ModelSelectionListOutcome {
    Predefined(SelectionDetail),
    Manual,
    Refresh,
    Cancelled,
}

pub(super) fn select_model_with_ratatui_list(
    options: &[ModelOption],
    current_reasoning: ReasoningEffortLevel,
    dynamic_models: &DynamicModelRegistry,
) -> Result<ModelSelectionListOutcome> {
    if options.is_empty() {
        return Err(anyhow!("No models available for selection"));
    }

    let mut choices = Vec::new();
    for provider in picker_provider_order() {
        if provider == Provider::LmStudio {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();

            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    choices.push(ModelSelectionChoice {
                        entry: SelectionEntry::new(
                            "LM Studio server not running - Setup instructions",
                            Some(format!("{error}\n{}", get_lmstudio_setup_instructions())),
                        ),
                        outcome: ModelSelectionChoiceOutcome::Manual,
                    });
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        let title =
                            format!("{} • {} (dynamic)", provider.label(), detail.model_display);
                        let description = format!(
                            "ID: {} — Locally available LM Studio model",
                            detail.model_id
                        );
                        choices.push(ModelSelectionChoice {
                            entry: SelectionEntry::new(title, Some(description)),
                            outcome: ModelSelectionChoiceOutcome::Predefined(detail.clone()),
                        });
                    }
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(
                        format!("{} cache notice", provider.label()),
                        Some(format!(
                            "{warning} Select 'Refresh local models' to re-query."
                        )),
                    ),
                    outcome: ModelSelectionChoiceOutcome::Refresh,
                });
            }
        } else if provider == Provider::Ollama {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();

            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
            let dynamic_indexes = dynamic_models.indexes_for(provider);
            if dynamic_indexes.is_empty() {
                if let Some(error) = dynamic_models.error_for(provider) {
                    choices.push(ModelSelectionChoice {
                        entry: SelectionEntry::new(
                            "Ollama server not running - Setup instructions",
                            Some(format!("{error}\n{}", get_ollama_setup_instructions())),
                        ),
                        outcome: ModelSelectionChoiceOutcome::Manual,
                    });
                }
            } else {
                for entry_index in dynamic_indexes {
                    if let Some(detail) = dynamic_models.detail(entry_index) {
                        let title =
                            format!("{} • {} (local)", provider.label(), detail.model_display);
                        let description =
                            format!("ID: {} — Locally available Ollama model", detail.model_id);
                        choices.push(ModelSelectionChoice {
                            entry: SelectionEntry::new(title, Some(description)),
                            outcome: ModelSelectionChoiceOutcome::Predefined(detail.clone()),
                        });
                    }
                }
            }

            if let Some(warning) = dynamic_models.warning_for(provider) {
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(
                        format!("{} cache notice", provider.label()),
                        Some(format!(
                            "{warning} Select 'Refresh local models' to re-query."
                        )),
                    ),
                    outcome: ModelSelectionChoiceOutcome::Refresh,
                });
            }
        } else {
            let provider_models: Vec<&ModelOption> = options
                .iter()
                .filter(|option| option.provider == provider)
                .collect();
            for option in &provider_models {
                let mut title = format!("{} • {}", provider.label(), option.display);
                if option.supports_reasoning {
                    title.push_str(" [Reasoning]");
                }
                let description = format!("ID: {} — {}", option.id, option.description);
                choices.push(ModelSelectionChoice {
                    entry: SelectionEntry::new(title, Some(description)),
                    outcome: ModelSelectionChoiceOutcome::Predefined(selection_from_option(option)),
                });
            }
        }
    }

    choices.push(ModelSelectionChoice {
        entry: SelectionEntry::new(
            REFRESH_ENTRY_LABEL,
            Some("Re-query local servers without closing the picker.".to_string()),
        ),
        outcome: ModelSelectionChoiceOutcome::Refresh,
    });

    choices.push(ModelSelectionChoice {
        entry: SelectionEntry::new(
            CUSTOM_PROVIDER_TITLE,
            Some(CUSTOM_PROVIDER_SUBTITLE.to_string()),
        ),
        outcome: ModelSelectionChoiceOutcome::Manual,
    });

    let entries: Vec<SelectionEntry> = choices.iter().map(|choice| choice.entry.clone()).collect();

    let instructions = format!(
        "Current reasoning effort: {}. Models marked with [Reasoning] support adjustable reasoning.",
        current_reasoning
    );

    let selection = run_interactive_selection("Models", &instructions, &entries, 0)?;
    let selected_index = match selection {
        Some(index) => index,
        None => return Ok(ModelSelectionListOutcome::Cancelled),
    };

    match &choices[selected_index].outcome {
        ModelSelectionChoiceOutcome::Predefined(detail) => {
            Ok(ModelSelectionListOutcome::Predefined(detail.clone()))
        }
        ModelSelectionChoiceOutcome::Manual => Ok(ModelSelectionListOutcome::Manual),
        ModelSelectionChoiceOutcome::Refresh => Ok(ModelSelectionListOutcome::Refresh),
    }
}

pub(super) fn select_reasoning_with_ratatui(
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<Option<ReasoningChoice>> {
    let mut entries = vec![
        SelectionEntry::new(
            format!("Keep current ({})", reasoning_level_label(current)),
            Some(KEEP_CURRENT_DESCRIPTION.to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::Low),
            Some(reasoning_level_description(ReasoningEffortLevel::Low).to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::Medium),
            Some(reasoning_level_description(ReasoningEffortLevel::Medium).to_string()),
        ),
        SelectionEntry::new(
            reasoning_level_label(ReasoningEffortLevel::High),
            Some(reasoning_level_description(ReasoningEffortLevel::High).to_string()),
        ),
    ];

    let mut disable_index = None;
    if let Some(alternative) = selection.reasoning_off_model {
        entries.push(SelectionEntry::new(
            format!("Use {} (reasoning off)", alternative.display_name()),
            Some(format!(
                "Switch to {} ({}) without enabling structured reasoning.",
                alternative.display_name(),
                alternative.as_str()
            )),
        ));
        disable_index = Some(entries.len() - 1);
    }

    let mut instructions = format!(
        "Select reasoning effort for {}. Esc keeps the current level ({}).",
        selection.model_display,
        reasoning_level_label(current),
    );
    if let Some(alternative) = selection.reasoning_off_model {
        instructions.push(' ');
        instructions.push_str(&format!(
            "Choose \"Use {} (reasoning off)\" to switch to {}.",
            alternative.display_name(),
            alternative.as_str()
        ));
    }

    let selection_index =
        run_interactive_selection("Reasoning effort", &instructions, &entries, 0)?;

    let Some(index) = selection_index else {
        return Ok(None);
    };

    if disable_index == Some(index) {
        return Ok(Some(ReasoningChoice::Disable));
    }

    let choice = match index {
        0 => current,
        1 => ReasoningEffortLevel::Low,
        2 => ReasoningEffortLevel::Medium,
        3 => ReasoningEffortLevel::High,
        _ => current,
    };
    Ok(Some(ReasoningChoice::Level(choice)))
}
