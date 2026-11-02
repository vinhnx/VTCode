//! Per-provider model preset definitions.
//!
//! Each provider's presets live in a dedicated submodule. The parent
//! `model_presets` module calls these via `presets::<provider>::<provider>_presets()`.

use super::ReasoningEffortPreset;
use crate::config::types::ReasoningEffortLevel;

/// Build a single [`ReasoningEffortPreset`] from an effort level and static description.
/// Shared across providers that enumerate reasoning efforts.
pub(crate) fn reasoning_preset(effort: ReasoningEffortLevel, description: &'static str) -> ReasoningEffortPreset {
    ReasoningEffortPreset { effort, description: description.to_string() }
}

/// Two-tier ladder for providers exposing only a balanced default and a deeper
/// tier: `Balanced` (medium) then `Deep` (high).
pub(crate) fn reasoning_ladder_medium_high() -> Vec<ReasoningEffortPreset> {
    vec![
        reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
        reasoning_preset(ReasoningEffortLevel::High, "Deep"),
    ]
}

/// Three-tier ladder with terse labels: `Fast` (low), `Balanced` (medium),
/// `Deep` (high).
pub(crate) fn reasoning_ladder_low_medium_high() -> Vec<ReasoningEffortPreset> {
    vec![
        reasoning_preset(ReasoningEffortLevel::Low, "Fast"),
        reasoning_preset(ReasoningEffortLevel::Medium, "Balanced"),
        reasoning_preset(ReasoningEffortLevel::High, "Deep"),
    ]
}

/// Three-tier ladder whose labels each name the word "reasoning": `Fast
/// responses` (low), `Balanced reasoning` (medium), `Deep reasoning` (high).
pub(crate) fn reasoning_ladder_low_medium_high_verbose() -> Vec<ReasoningEffortPreset> {
    vec![
        reasoning_preset(ReasoningEffortLevel::Low, "Fast responses"),
        reasoning_preset(ReasoningEffortLevel::Medium, "Balanced reasoning"),
        reasoning_preset(ReasoningEffortLevel::High, "Deep reasoning"),
    ]
}

/// Two-tier ladder from a balanced high tier to maximum thinking: `Balanced`
/// (high) then `Maximum thinking` (max).
pub(crate) fn reasoning_ladder_high_max() -> Vec<ReasoningEffortPreset> {
    vec![
        reasoning_preset(ReasoningEffortLevel::High, "Balanced"),
        reasoning_preset(ReasoningEffortLevel::Max, "Maximum thinking"),
    ]
}

mod copilot;
pub(crate) use copilot::copilot_presets;
mod gemini;
pub(crate) use gemini::gemini_presets;
mod openai;
pub(crate) use openai::openai_presets;
mod anthropic;
pub(crate) use anthropic::anthropic_presets;
mod deepseek;
pub(crate) use deepseek::deepseek_presets;
mod zai;
pub(crate) use zai::zai_presets;
mod mistral;
pub(crate) use mistral::mistral_presets;
mod minimax;
pub(crate) use minimax::minimax_presets;
mod meta;
pub(crate) use meta::meta_presets;
mod openrouter;
pub(crate) use openrouter::openrouter_presets;
mod ollama;
pub(crate) use ollama::ollama_presets;
mod lmstudio;
pub(crate) use lmstudio::lmstudio_presets;
mod llamacpp;
pub(crate) use llamacpp::llamacpp_presets;
mod opencode_zen;
pub(crate) use opencode_zen::opencode_zen_presets;
mod opencode_go;
pub(crate) use opencode_go::opencode_go_presets;
mod poolside;
pub(crate) use poolside::poolside_presets;
mod mimo;
pub(crate) use mimo::mimo_presets;
mod qwen;
pub(crate) use qwen::qwen_presets;
mod stepfun;
pub(crate) use stepfun::stepfun_presets;
mod xai;
pub(crate) use xai::xai_presets;
mod nvidia;
pub(crate) use nvidia::nvidia_presets;
mod merge_gateway;
pub(crate) use merge_gateway::merge_gateway_presets;
mod vercel;
pub(crate) use vercel::vercel_presets;
mod evolink;
pub(crate) use evolink::evolink_presets;
mod moonshot;
pub(crate) use moonshot::moonshot_presets;
mod huggingface;
pub(crate) use huggingface::huggingface_presets;
