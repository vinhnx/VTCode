use std::path::Path;

use anyhow::Result;
use vtcode_config::OpenAIServiceTier;
use vtcode_core::config::models::Provider;
use vtcode_core::config::types::ReasoningEffortLevel;
use vtcode_core::ui::{InlineListItem, InlineListSelection, OpenAIServiceTierChoice, reasoning_to_selection_string};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::super::selection::{
    SelectionDetail, available_service_tiers, reasoning_level_description, reasoning_level_label,
    service_tier_choice_meta, service_tier_label,
};
use super::{KEEP_CURRENT_DESCRIPTION, STEP_FOUR_TITLE, STEP_THREE_TITLE, STEP_TWO_TITLE};
use vtcode_commons::modal_hints::MODEL_PICKER_FOLLOW_UP_HINT;

pub(in crate::agent::runloop::model_picker) fn back_to_model_list_row() -> InlineListItem {
    vtcode_ui::design::list::action(
        "← Back to model list",
        "Return to step 1 without cancelling the picker.",
        None,
        vtcode_commons::ui_protocol::InlineTone::Neutral,
        Some(InlineListSelection::ConfigAction(super::PICKER_BACK_ACTION.to_string())),
    )
    .with_search_value("back model list")
}

pub(crate) fn render_reasoning_inline(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    let mut items = Vec::new();
    items.push(back_to_model_list_row());
    items.push(vtcode_ui::design::list::current_choice(
        format!("Keep current ({})", reasoning_level_label(current)),
        Some(KEEP_CURRENT_DESCRIPTION.to_string()),
        Some(InlineListSelection::Reasoning(reasoning_to_selection_string(current))),
    ));

    let levels = selection.reasoning_effort_levels();
    if levels.contains(&ReasoningEffortLevel::None) {
        items.push(vtcode_ui::design::list::choice(
            reasoning_level_label(ReasoningEffortLevel::None),
            Some(reasoning_level_description(ReasoningEffortLevel::None).to_string()),
            Some(InlineListSelection::Reasoning(reasoning_to_selection_string(ReasoningEffortLevel::None))),
        ));
    }

    for level in levels.into_iter().filter(|level| *level != ReasoningEffortLevel::None) {
        items.push(vtcode_ui::design::list::choice(
            reasoning_level_label(level),
            Some(reasoning_level_description(level).to_string()),
            Some(InlineListSelection::Reasoning(reasoning_to_selection_string(level))),
        ));
    }

    if let Some(alternative) = selection.reasoning_off_model.as_ref() {
        items.push(vtcode_ui::design::list::choice(
            format!("Use {} (reasoning off)", alternative.display_name()),
            Some(format!(
                "Switch to {} ({}) without enabling structured reasoning.",
                alternative.display_name(),
                alternative.as_str()
            )),
            Some(InlineListSelection::DisableReasoning),
        ));
    }
    let mut lines = vec![
        "Step 2 · Reasoning".to_string(),
        format!("Selected: {}", selection.model_display),
        format!("Current reasoning: {}", reasoning_level_label(current)),
    ];
    if let Some(alternative) = selection.reasoning_off_model.as_ref() {
        lines.push(format!(
            "Select \"Use {} (reasoning off)\" to switch to {}.",
            alternative.display_name(),
            alternative.as_str()
        ));
    }
    renderer.show_list_modal_with_footer(
        STEP_TWO_TITLE,
        lines,
        items,
        Some(InlineListSelection::Reasoning(reasoning_to_selection_string(current))),
        None,
        Some(MODEL_PICKER_FOLLOW_UP_HINT.to_string()),
    );
    Ok(())
}

pub(crate) fn prompt_reasoning_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: ReasoningEffortLevel,
) -> Result<()> {
    let is_responses_flagship = selection.reasoning_effort_levels().contains(&ReasoningEffortLevel::None);
    let effort_choices = selection
        .reasoning_effort_levels()
        .into_iter()
        .map(reasoning_level_input)
        .collect::<Vec<_>>()
        .join("/");
    let effort_choices = if effort_choices.is_empty() {
        "skip".to_string()
    } else {
        effort_choices
    };

    if selection.reasoning_optional {
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – reasoning effort (current: {current}). Choose {effort_choices} or type 'skip' if the model does not expose configurable reasoning."
            ),
        )?;
    } else if let Some(alternative) = selection.reasoning_off_model.as_ref() {
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
                effort_choices,
                "",
                alternative.display_name(),
                alternative.as_str(),
                alternative.display_name(),
                gpt5_hint
            ),
        )?;
    } else {
        let gpt5_hint = if is_responses_flagship {
            " For GPT-5.x, 'none' provides lowest latency."
        } else {
            ""
        };
        renderer.line(
            MessageStyle::Info,
            &format!(
                "Step 2 – select reasoning effort for {} ({}{}). Type 'skip' to keep {}. Current: {}.{}",
                selection.model_display, effort_choices, "", current, current, gpt5_hint
            ),
        )?;
    }
    Ok(())
}

fn reasoning_level_input(level: ReasoningEffortLevel) -> &'static str {
    match level {
        ReasoningEffortLevel::None => "none",
        ReasoningEffortLevel::Minimal => "minimal",
        ReasoningEffortLevel::Low => "low",
        ReasoningEffortLevel::Medium => "medium",
        ReasoningEffortLevel::High => "high",
        ReasoningEffortLevel::XHigh => "xhigh",
        ReasoningEffortLevel::Max => "max",
        ReasoningEffortLevel::Unknown => "unknown",
    }
}

pub(crate) fn prompt_api_key_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    _workspace: Option<&Path>,
) -> Result<()> {
    if matches!(selection.provider_enum, Some(Provider::OpenAI)) {
        renderer.line(
            MessageStyle::Info,
            "Authentication – type 'login' to sign in with your ChatGPT subscription, paste an API key, or type 'skip' to reuse a stored credential.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "ChatGPT subscription auth will be stored securely and will not be written to your workspace environment.",
        )?;
        return Ok(());
    }

    renderer.line(
        MessageStyle::Info,
        &format!("API key – enter a key for {} (env: {}).", selection.provider_label, selection.env_key),
    )?;
    renderer.line(MessageStyle::Info, "The key will be saved to secure storage (OS keyring or encrypted file).")?;
    renderer.line(MessageStyle::Info, "The key will NOT be stored in vtcode.toml for security.")?;
    renderer.line(MessageStyle::Info, "Or run `/secret add <provider>` to manage keys separately.")?;

    if matches!(selection.provider_enum, Some(Provider::HuggingFace)) {
        renderer.line(
            MessageStyle::Info,
            "Optional: override base URL with HUGGINGFACE_BASE_URL (default https://router.huggingface.co/v1).",
        )?;
    }
    renderer.line(MessageStyle::Info, "Paste the API key now or type 'skip' to reuse a stored credential.")?;
    Ok(())
}

pub(crate) fn render_service_tier_inline(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: Option<OpenAIServiceTier>,
) -> Result<()> {
    fn to_choice(tier: Option<OpenAIServiceTier>) -> OpenAIServiceTierChoice {
        match tier {
            Some(OpenAIServiceTier::Flex) => OpenAIServiceTierChoice::Flex,
            Some(OpenAIServiceTier::Priority) => OpenAIServiceTierChoice::Priority,
            Some(OpenAIServiceTier::Ultrafast) => OpenAIServiceTierChoice::Ultrafast,
            None => OpenAIServiceTierChoice::ProjectDefault,
        }
    }

    let mut items = vec![
        back_to_model_list_row(),
        vtcode_ui::design::list::current_choice(
            format!("Keep current ({})", service_tier_label(current)),
            Some("Retain the existing service tier configuration.".to_string()),
            Some(InlineListSelection::OpenAIServiceTier(to_choice(current))),
        ),
    ];
    for tier in available_service_tiers(selection) {
        let (title, subtitle) = service_tier_choice_meta(tier);
        items.push(vtcode_ui::design::list::choice(
            title,
            Some(subtitle.to_string()),
            Some(InlineListSelection::OpenAIServiceTier(to_choice(tier))),
        ));
    }

    renderer.show_list_modal_with_footer(
        STEP_THREE_TITLE,
        vec![
            format!("Selected: {}", selection.model_display),
            "Applies only to OpenAI-compatible models that support service tiers.".to_string(),
        ],
        items,
        Some(InlineListSelection::OpenAIServiceTier(to_choice(current))),
        None,
        Some(MODEL_PICKER_FOLLOW_UP_HINT.to_string()),
    );
    Ok(())
}

pub(crate) fn prompt_service_tier_plain(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    current: Option<OpenAIServiceTier>,
) -> Result<()> {
    let mut options: Vec<&str> = available_service_tiers(selection)
        .into_iter()
        .map(|tier| match tier {
            None => "default",
            Some(OpenAIServiceTier::Flex) => "flex",
            Some(OpenAIServiceTier::Priority) => "priority",
            Some(OpenAIServiceTier::Ultrafast) => "ultrafast",
        })
        .collect();
    if options.is_empty() {
        options.push("default");
    }
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Service tier – choose {} for {}. Type 'skip' to keep {}.",
            options
                .iter()
                .map(|option| format!("'{option}'"))
                .collect::<Vec<_>>()
                .join(", "),
            selection.model_display,
            service_tier_label(current)
        ),
    )?;
    renderer.line(MessageStyle::Info, "This applies only to OpenAI-compatible models that support service tiers.")?;
    Ok(())
}

pub(crate) fn show_secure_api_modal(
    renderer: &mut AnsiRenderer,
    selection: &SelectionDetail,
    _workspace: Option<&Path>,
) {
    let lines = vec![
        "## Provider".to_string(),
        format!("Bring your own key (BYOK) for {}.", selection.provider_label),
        format!("Expected env: {}", selection.env_key),
        "## Storage".to_string(),
        "Saved to secure storage (OS keyring or encrypted file).".to_string(),
        "**Key will NOT be stored in vtcode.toml.**".to_string(),
        "Paste the key — it will be auto-detected and saved securely.".to_string(),
    ];
    let prompt_label = format!("{} API key ({})", selection.provider_label, selection.env_key);
    renderer.show_secure_prompt_modal("Secure API key • Final step", lines, prompt_label);
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
    renderer.line(MessageStyle::Info, "Type 'cancel' to exit the picker at any time.")?;
    renderer.line(MessageStyle::Info, "Type 'refresh' to reload LM Studio and Ollama model lists.")?;
    Ok(())
}

pub(crate) fn render_mimo_auth_method_inline(renderer: &mut AnsiRenderer) -> Result<()> {
    let items = vec![
        vtcode_ui::design::list::action(
            "Pay-as-you-go",
            "Standard API access. Uses sk- key with api-key header.",
            Some("Default".to_string()),
            vtcode_commons::ui_protocol::InlineTone::Accent,
            Some(InlineListSelection::ConfigAction("mimo-auth:pay-as-you-go".to_string())),
        )
        .with_search_value("mimo payg pay-as-you-go sk api key"),
        vtcode_ui::design::list::action(
            "Token Plan",
            "Subscription-based access. Uses tp- key with Bearer token. Includes more models.",
            Some("Subscription".to_string()),
            vtcode_commons::ui_protocol::InlineTone::Accent,
            Some(InlineListSelection::ConfigAction("mimo-auth:token-plan".to_string())),
        )
        .with_search_value("mimo token plan subscription tp bearer"),
    ];

    renderer.show_list_modal(
        "MiMo Auth Method",
        vec![
            "Choose an authentication method for Xiaomi MiMo.".to_string(),
            "Token Plan defaults to Europe cluster (token-plan-ams). Set MIMO_TOKEN_PLAN_BASE_URL to override region."
                .to_string(),
        ],
        items,
        None,
        None,
    );
    Ok(())
}

pub(crate) fn prompt_mimo_auth_method_plain(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "MiMo auth method - choose 'pay-as-you-go' or 'token-plan'. Type 'skip' for default (pay-as-you-go).",
    )?;
    renderer.line(MessageStyle::Info, "Pay-as-you-go: standard API access with sk- key.")?;
    renderer.line(MessageStyle::Info, "Token Plan: subscription-based access with tp- key. Includes more models.")?;
    renderer.line(
        MessageStyle::Info,
        "Note: Token Plan defaults to Europe cluster (token-plan-ams). Set MIMO_TOKEN_PLAN_BASE_URL to use China (token-plan-cn) or Singapore (token-plan-sgp).",
    )?;
    Ok(())
}

pub(crate) const DECISIONS_PROBE_ENABLE_ACTION: &str = "decisions-probe:enable";
pub(crate) const DECISIONS_PROBE_DISABLE_ACTION: &str = "decisions-probe:disable";
pub(crate) const DECISIONS_PROBE_SKIP_ACTION: &str = "decisions-probe:skip";

pub(crate) fn render_decisions_probe_inline(renderer: &mut AnsiRenderer, selection: &SelectionDetail) -> Result<()> {
    let items = vec![
        back_to_model_list_row(),
        vtcode_ui::design::list::current_choice(
            "Keep current (disabled)",
            Some("Leave permissions.auto.use_decisions_probe unchanged.".to_string()),
            Some(InlineListSelection::ConfigAction(DECISIONS_PROBE_SKIP_ACTION.to_string())),
        ),
        vtcode_ui::design::list::choice(
            "Enable Decisions probe",
            Some("Experimental OpenAI Decisions classification for suspicious tool output. Billed to your OpenAI API account.".to_string()),
            Some(InlineListSelection::ConfigAction(DECISIONS_PROBE_ENABLE_ACTION.to_string())),
        ),
        vtcode_ui::design::list::choice(
            "Keep disabled",
            Some("Continue with the generation probe only.".to_string()),
            Some(InlineListSelection::ConfigAction(DECISIONS_PROBE_DISABLE_ACTION.to_string())),
        ),
    ];
    renderer.show_list_modal_with_footer(
        STEP_FOUR_TITLE,
        vec![
            format!("Selected: {}", selection.model_display),
            "Experimental, default-off. OpenAI API-key at the standard endpoint only.".to_string(),
            "See docs/guides/decisions-probe.md for costs and limits.".to_string(),
        ],
        items,
        Some(InlineListSelection::ConfigAction(DECISIONS_PROBE_SKIP_ACTION.to_string())),
        None,
        Some(MODEL_PICKER_FOLLOW_UP_HINT.to_string()),
    );
    Ok(())
}

pub(crate) fn prompt_decisions_probe_plain(renderer: &mut AnsiRenderer, selection: &SelectionDetail) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        &format!(
            "Decisions probe for {} ({}): type 'yes' to enable, 'no' to keep disabled, or 'skip' to leave unchanged.",
            selection.model_display, selection.model_id
        ),
    )?;
    renderer.line(
        MessageStyle::Info,
        "Experimental, default-off. OpenAI API-key at the standard endpoint only; billed to your OpenAI API account. See docs/guides/decisions-probe.md.",
    )?;
    Ok(())
}
