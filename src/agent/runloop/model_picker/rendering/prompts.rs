use std::path::Path;

use crate::agent::runloop::tui_compat::to_tui_reasoning;
use anyhow::Result;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSelection};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::super::selection::{
    SelectionDetail, reasoning_level_description, reasoning_level_label,
};
use super::{CURRENT_BADGE, KEEP_CURRENT_DESCRIPTION, REASONING_OFF_BADGE, STEP_TWO_TITLE};

pub(crate) fn render_reasoning_inline(
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
        selection: Some(InlineListSelection::Reasoning(to_tui_reasoning(current))),
        search_value: None,
    });

    // For GPT-5.2 and GPT-5.3 Codex models, show "None" first as the default option (fastest)
    let is_gpt5_responses =
        selection.model_id.starts_with("gpt-5.2") || selection.model_id.starts_with("gpt-5.3");
    if is_gpt5_responses {
        items.push(InlineListItem {
            title: reasoning_level_label(ReasoningEffortLevel::None).to_string(),
            subtitle: Some(reasoning_level_description(ReasoningEffortLevel::None).to_string()),
            badge: Some("GPT-5.x".to_string()),
            indent: 0,
            selection: Some(InlineListSelection::Reasoning(to_tui_reasoning(
                ReasoningEffortLevel::None,
            ))),
            search_value: None,
        });
    }

    let is_codex_max = selection.model_id.contains("codex-max");

    let mut levels = vec![
        ReasoningEffortLevel::Minimal,
        ReasoningEffortLevel::Low,
        ReasoningEffortLevel::Medium,
        ReasoningEffortLevel::High,
    ];

    if is_codex_max {
        levels.push(ReasoningEffortLevel::XHigh);
    }

    for level in levels {
        items.push(InlineListItem {
            title: reasoning_level_label(level).to_string(),
            subtitle: Some(reasoning_level_description(level).to_string()),
            badge: None,
            indent: 0,
            selection: Some(InlineListSelection::Reasoning(to_tui_reasoning(level))),
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
    let mut lines = vec![format!(
        "Step 2 – select reasoning effort for {}.",
        selection.model_display
    )];
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
        Some(InlineListSelection::Reasoning(to_tui_reasoning(current))),
        None,
    );
    Ok(())
}

pub(crate) fn prompt_reasoning_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    let is_responses_flagship =
        selection.model_id.starts_with("gpt-5.2") || selection.model_id.starts_with("gpt-5.3");
    let is_codex_max = selection.model_id.contains("codex-max");
    let xhigh_suffix = if is_codex_max { "/xhigh" } else { "" };

    if selection.reasoning_optional {
        let prefix = if is_responses_flagship {
            "none/low/medium/high"
        } else {
            "low/medium/high"
        };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – reasoning effort (current: {}). Choose {}{} or type 'skip' if the model does not expose configurable reasoning.",
                current,
                prefix,
                xhigh_suffix
            ),
        )?;
    } else if let Some(alternative) = selection.reasoning_off_model {
        let prefix = if is_responses_flagship {
            "none/low/medium/high"
        } else {
            "low/medium/high"
        };
        let gpt5_hint = if is_responses_flagship {
            " For GPT-5.x, 'none' provides lowest latency."
        } else {
            ""
        };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} ({}{}). Type 'skip' to keep {} or 'off' to use {} ({}).{}",
                selection.model_display,
                prefix,
                xhigh_suffix,
                alternative.display_name(),
                alternative.as_str(),
                alternative.display_name(),
                gpt5_hint
            ),
        )?;
    } else {
        let prefix = if is_responses_flagship {
            "none/low/medium/high"
        } else {
            "low/medium/high"
        };
        let gpt5_hint = if is_responses_flagship {
            " For GPT-5.x, 'none' provides lowest latency."
        } else {
            ""
        };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} ({}{}). Type 'skip' to keep {}. Current: {}.{}",
                selection.model_display,
                prefix,
                xhigh_suffix,
                current,
                current,
                gpt5_hint
            ),
        )?;
    }
    Ok(())
}

pub(crate) fn prompt_api_key_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    _workspace: Option<&Path>,
) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Step 3 – enter an API key for {} (env: {}).",
            selection.provider_label, selection.env_key
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "The key will be saved to secure storage (OS keyring) and your workspace .env file.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "The key will NOT be stored in vtcode.toml for security.",
    )?;

    if matches!(selection.provider_enum, Some(Provider::HuggingFace)) {
        renderer.line(
            MessageStyle::Info,
            "Optional: override base URL with HUGGINGFACE_BASE_URL (default https://router.huggingface.co/v1).",
        )?;
    }
    renderer.line(
        MessageStyle::Info,
        "Paste the API key now or type 'skip' to reuse a stored credential.",
    )?;
    Ok(())
}

pub(crate) fn show_secure_api_modal(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    workspace: Option<&Path>,
) {
    let storage_line = workspace
        .map(|root| {
            let env_path = root.join(".env");
            format!("Saved to keyring and {}.", env_path.display())
        })
        .unwrap_or_else(|| "Saved to keyring and workspace .env file.".to_string());
    let mask_preview = "●●●●●●";
    let lines = vec![
        format!(
            "Bring your own key (BYOK) for {}.",
            selection.provider_label
        ),
        format!("Secure display hint: {}", mask_preview),
        storage_line,
        "Key will NOT be stored in vtcode.toml.".to_string(),
        "Paste the key and press Enter when ready.".to_string(),
    ];
    let prompt_label = format!("{} API key", selection.provider_label);
    renderer.show_secure_prompt_modal("Secure API key setup", lines, prompt_label);
}

pub(crate) fn prompt_custom_model_entry(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Enter a provider and model identifier (examples: 'openai gpt-5-nano', 'huggingface meta-llama/Meta-Llama-3-70B-Instruct', 'ollama qwen3:1.7b').",
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
