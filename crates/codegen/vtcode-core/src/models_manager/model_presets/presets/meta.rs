//! Model presets for the official Meta AI provider.

use super::super::{ModelPreset, ReasoningEffortPreset};
use super::reasoning_preset;
use crate::config::constants::models;
use crate::config::models::Provider;
use crate::config::types::ReasoningEffortLevel;

fn reasoning_efforts() -> Vec<ReasoningEffortPreset> {
    [
        (ReasoningEffortLevel::Minimal, "Minimal reasoning"),
        (ReasoningEffortLevel::Low, "Fast reasoning"),
        (ReasoningEffortLevel::Medium, "Balanced reasoning"),
        (ReasoningEffortLevel::High, "Deep reasoning"),
        (ReasoningEffortLevel::XHigh, "Maximum supported reasoning"),
        (ReasoningEffortLevel::Max, "Max reasoning"),
    ]
    .into_iter()
    .map(|(effort, description)| reasoning_preset(effort, description))
    .collect()
}

pub(crate) fn meta_presets() -> Vec<ModelPreset> {
    [
        (
            models::meta::MUSE_WONDER_FULL_1_0,
            "Muse Wonder Full 1.0 (Meta AI)",
            "Meta's most advanced model for critical workflows with SOTA agentic and coding performance and a 1M-token context window (Muse Code subscription)",
            false,
        ),
        (
            models::meta::MUSE_WONDER_1_0,
            "Muse Wonder 1.0 (Meta AI)",
            "Frontier Meta AI model for coding, long-running agents, and advanced computer use with always-on reasoning and a 1M-token context window",
            false,
        ),
        (
            models::meta::MUSE_SPARK_1_3,
            "Muse Spark 1.3 (Meta AI)",
            "Official Meta AI Standard-tier flagship tuned for agentic workflows with always-on reasoning and a 1M-token context window",
            true,
        ),
        (
            models::meta::MUSE_SPARK_1_3_CONTRIBUTOR,
            "Muse Spark 1.3 Contributor (Meta AI)",
            "Opt-in Meta AI Contributor-tier Muse Spark 1.3 variant; review Meta's data-contribution terms before use",
            false,
        ),
        (
            models::meta::MUSE_SPARK_1_1,
            "Muse Spark 1.1 (Meta AI)",
            "Official Meta AI Standard-tier model with always-on reasoning and a 1M-token context window",
            false,
        ),
    ]
    .into_iter()
    .map(|(model, display_name, description, is_default)| ModelPreset {
        id: model.to_owned(),
        model: model.to_owned(),
        display_name: display_name.to_owned(),
        description: description.to_owned(),
        provider: Provider::Meta,
        default_reasoning_effort: ReasoningEffortLevel::Medium,
        supported_reasoning_efforts: reasoning_efforts(),
        is_default,
        upgrade: None,
        show_in_picker: true,
        supported_in_api: true,
        context_window: Some(1_048_576),
    })
    .collect()
}
