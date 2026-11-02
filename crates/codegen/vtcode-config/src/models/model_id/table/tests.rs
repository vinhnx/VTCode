use std::str::FromStr;

use crate::constants::models;
use crate::models::Provider;

use super::ModelId;

/// Providers whose model ids intentionally do not round-trip through
/// `from_str`: their `as_str()` values are bare ids that collide with (and
/// resolve to) native provider variants. They are reachable only through
/// picker selection or provider prefixes (`opencode*/`).
fn round_trip_exempt(model: &ModelId) -> bool {
    matches!(model.provider(), Provider::OpenCodeZen | Provider::OpenCodeGo | Provider::Qwen)
        || *model == ModelId::LlamaCppGptOss20b
        || matches!(
            model,
            ModelId::MergeGatewayAnthropicClaudeOpus5
                | ModelId::MergeGatewayAnthropicClaudeOpus55
                | ModelId::MergeGatewayAnthropicClaudeSonnet5
                | ModelId::MergeGatewayAnthropicClaudeSonnet55
                | ModelId::MergeGatewayGoogleGemini38Flash
                | ModelId::MergeGatewayOpenAIGpt6Astra
                | ModelId::MergeGatewayOpenAIGpt6Sol
                | ModelId::MergeGatewayOpenAIGpt61Sol
                | ModelId::MergeGatewayOpenAIGpt6Luna
                | ModelId::MergeGatewayDeepseekFlash
                | ModelId::MergeGatewayMetaMuseSparkOpen
        )
        || matches!(
            model,
            ModelId::VercelAnthropicClaudeSonnet5
                | ModelId::VercelAnthropicClaudeOpus5
                | ModelId::VercelAnthropicClaudeOpus55
                | ModelId::VercelOpenAiGpt6Astra
                | ModelId::VercelOpenAiGpt56Sol
                | ModelId::VercelOpenAiGpt56Luna
                | ModelId::VercelOpenAiGpt6Sol
                | ModelId::VercelOpenAiGpt6Luna
                | ModelId::VercelGoogleGemini38Flash
                | ModelId::VercelDeepseekFlash
                | ModelId::VercelMoonshotaiKimiK3
        )
}

#[test]
fn all_models_round_trip_through_from_str() {
    for model in ModelId::all_models() {
        if round_trip_exempt(&model) {
            continue;
        }
        let id = model.as_str();
        let parsed =
            ModelId::from_str(&id).unwrap_or_else(|err| panic!("failed to parse {id} back into {model:?}: {err}"));
        assert_eq!(parsed, model, "round-trip mismatch for id {id}");
    }
}

#[test]
fn every_non_openrouter_model_is_in_the_table() {
    for model in ModelId::all_models() {
        if model.provider() == Provider::OpenRouter {
            continue;
        }
        assert!(model.table_id().is_some(), "{model:?} is missing from the model_id_table! invocation");
    }
}

#[test]
fn parse_aliases_resolve_to_canonical_variants() {
    let cases: &[(&str, ModelId)] = &[
        (models::GPT, ModelId::GPT56Sol),
        (models::openai::GPT_5_6_SOL, ModelId::GPT56Sol),
        (models::CLAUDE_SONNET_5, ModelId::ClaudeSonnet5),
        (models::mistral::MISTRAL_LARGE_4, ModelId::MistralLarge4),
        ("mistral-large-2610", ModelId::MistralLarge4),
        ("mistral-large-4-0", ModelId::MistralLarge4),
    ];
    for (alias, expected) in cases {
        let parsed = ModelId::from_str(alias).unwrap_or_else(|err| panic!("alias {alias} failed to parse: {err}"));
        assert_eq!(&parsed, expected, "alias {alias} resolved incorrectly");
    }
}

#[test]
fn shared_gpt_oss_20b_id_resolves_to_openai() {
    assert_eq!(models::openai::GPT_OSS_20B, models::llamacpp::GPT_OSS_20B);
    assert_eq!(ModelId::from_str(models::llamacpp::GPT_OSS_20B).unwrap(), ModelId::OpenAIGptOss20b);
}
