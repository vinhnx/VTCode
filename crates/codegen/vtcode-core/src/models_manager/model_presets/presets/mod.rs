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
mod evolink;
pub(crate) use evolink::evolink_presets;
mod moonshot;
pub(crate) use moonshot::moonshot_presets;
mod huggingface;
pub(crate) use huggingface::huggingface_presets;
