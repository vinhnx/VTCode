//! Shared model policy for Chat Completions and Responses payloads.

use vtcode_config::constants::models::openai as openai_models;
use vtcode_config::types::{ReasoningEffortLevel, VerbosityLevel};

const NONE_REASONING_EFFORT_MODELS: &[&str] = &[openai_models::GPT, openai_models::GPT_5_6, openai_models::GPT_5_6_SOL];

const MEDIUM_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
];

const HIGH_REASONING_EFFORT_MODELS: &[&str] = &[
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_6_TERRA,
    openai_models::GPT_5_6_LUNA,
    openai_models::GPT_5_6,
];

const TEXT_VERBOSITY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
    openai_models::GPT_5_6,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_CODEX,
    openai_models::GPT_5_6_TERRA,
    openai_models::GPT_5_6_LUNA,
];

const LOW_VERBOSITY_MODELS: &[&str] = &[openai_models::GPT, openai_models::GPT_5_6, openai_models::GPT_5_6_SOL];

const PHASE_REPLAY_MODELS: &[&str] = &[
    openai_models::GPT,
    openai_models::GPT_6_ASTRA,
    openai_models::GPT_6_SOL,
    openai_models::GPT_6_LUNA,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_CODEX,
];

const GATED_SAMPLING_MODELS: &[&str] = &[openai_models::GPT, openai_models::GPT_5_6, openai_models::GPT_5_6_SOL];

const SAMPLING_DISABLED_MODELS: &[&str] = &[
    openai_models::GPT_5,
    openai_models::GPT_5_6_SOL,
    openai_models::GPT_5_MINI,
    openai_models::GPT_5_NANO,
];

pub(super) fn is_gpt5_codex_model(model: &str) -> bool {
    model == openai_models::GPT_5_CODEX || (model.starts_with(openai_models::GPT_5) && model.contains("codex"))
}

pub(super) fn is_gpt56_model(model: &str) -> bool {
    matches!(
        model,
        openai_models::GPT_5_6_SOL
            | openai_models::GPT_5_6_TERRA
            | openai_models::GPT_5_6_LUNA
            | openai_models::GPT_5_6
    )
}

pub(super) fn is_gpt6_model(model: &str) -> bool {
    matches!(
        model,
        openai_models::GPT_6_ASTRA | openai_models::GPT_6_SOL | openai_models::GPT_6_1_SOL | openai_models::GPT_6_LUNA
    )
}

pub(super) fn is_openai_gpt_responses_model(model: &str) -> bool {
    openai_models::RESPONSES_API_MODELS.contains(&model)
}

pub(super) fn supports_assistant_phase_replay(model: &str) -> bool {
    PHASE_REPLAY_MODELS.contains(&model)
}

pub(super) fn default_reasoning_effort_for_model(model: &str) -> Option<ReasoningEffortLevel> {
    if NONE_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::None)
    } else if is_gpt5_codex_model(model) {
        Some(ReasoningEffortLevel::High)
    } else if MEDIUM_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::Medium)
    } else if HIGH_REASONING_EFFORT_MODELS.contains(&model) {
        Some(ReasoningEffortLevel::High)
    } else {
        None
    }
}

pub(super) fn supports_text_verbosity(model: &str) -> bool {
    TEXT_VERBOSITY_MODELS.contains(&model)
}

pub(super) fn default_text_verbosity_for_model(model: &str) -> Option<VerbosityLevel> {
    if LOW_VERBOSITY_MODELS.contains(&model) {
        Some(VerbosityLevel::Low)
    } else {
        None
    }
}

pub(super) fn allows_sampling_parameters(model: &str, reasoning_effort: Option<ReasoningEffortLevel>) -> bool {
    let supports_sampling = vtcode_config::models::model_catalog_entry("openai", model)
        .map(|entry| entry.supports_sampling)
        .unwrap_or_else(|| !SAMPLING_DISABLED_MODELS.contains(&model));
    if !supports_sampling {
        false
    } else if GATED_SAMPLING_MODELS.contains(&model) {
        matches!(reasoning_effort.unwrap_or(ReasoningEffortLevel::None), ReasoningEffortLevel::None)
    } else {
        !SAMPLING_DISABLED_MODELS.contains(&model)
    }
}
