use super::*;
use crate::config::constants::models;

#[test]
fn test_model_string_conversion() {
    // Gemini models
    assert_eq!(ModelId::Gemini38Flash.as_str(), models::google::GEMINI_3_8_FLASH);
    assert_eq!(ModelId::Gemini38Flash.as_str(), models::google::GEMINI_3_8_FLASH);
    // OpenAI models
    assert_eq!(ModelId::GPT56Sol.as_str(), models::GPT_5_6_SOL);
    assert_eq!(ModelId::GPT56Luna.as_str(), models::openai::GPT_5_6_LUNA);
    // Anthropic models
    assert_eq!(ModelId::ClaudeOpus5.as_str(), models::CLAUDE_OPUS_5);
    assert_eq!(ModelId::ClaudeSonnet5.as_str(), models::CLAUDE_SONNET_5);
    assert_eq!(ModelId::ClaudeSonnet5.as_str(), models::CLAUDE_SONNET_5);
    // Hugging Face models
    // Z.AI models
    assert_eq!(ModelId::ZaiGlm53.as_str(), models::zai::GLM_5_3);
    // OpenCode models
    assert_eq!(ModelId::OpenCodeGoMinimaxM3.as_str(), models::opencode_go::MINIMAX_M3);
}

#[test]
fn test_model_from_string() {
    // Gemini models
    assert_eq!(models::google::GEMINI_3_8_FLASH.parse::<ModelId>().unwrap(), ModelId::Gemini38Flash);
    assert_eq!(models::google::GEMINI_3_8_FLASH.parse::<ModelId>().unwrap(), ModelId::Gemini38Flash);
    // OpenAI models
    assert_eq!(models::GPT_5_6_SOL.parse::<ModelId>().unwrap(), ModelId::GPT56Sol);
    assert_eq!(models::GPT_5_CODEX.parse::<ModelId>().unwrap(), ModelId::GPT56Sol);
    assert_eq!(models::openai::GPT_5_6_LUNA.parse::<ModelId>().unwrap(), ModelId::GPT56Luna);
    assert_eq!(models::openai::GPT_5_6_LUNA.parse::<ModelId>().unwrap(), ModelId::GPT56Luna);
    assert_eq!(models::openai::GPT_OSS_20B.parse::<ModelId>().unwrap(), ModelId::OpenAIGptOss20b);
    assert_eq!(models::openai::GPT_OSS_120B.parse::<ModelId>().unwrap(), ModelId::OpenAIGptOss120b);
    // Anthropic models
    assert_eq!(models::CLAUDE_SONNET_5.parse::<ModelId>().unwrap(), ModelId::ClaudeSonnet5);
    assert_eq!(models::CLAUDE_SONNET_5.parse::<ModelId>().unwrap(), ModelId::ClaudeSonnet5);
    assert_eq!(models::CLAUDE_OPUS_5.parse::<ModelId>().unwrap(), ModelId::ClaudeOpus5);
    assert_eq!(models::CLAUDE_SONNET_5.parse::<ModelId>().unwrap(), ModelId::ClaudeSonnet5);
    // Z.AI models
    assert_eq!(models::zai::GLM_5_3.parse::<ModelId>().unwrap(), ModelId::ZaiGlm53);
    assert_eq!("opencode/gpt-5.4".parse::<ModelId>().unwrap(), ModelId::GPT56Sol);
    assert_eq!("opencode-go/minimax-m3".parse::<ModelId>().unwrap(), ModelId::OpenCodeGoMinimaxM3);
    // Invalid model
    "invalid-model".parse::<ModelId>().unwrap_err();
}

#[test]
fn test_provider_parsing() {
    assert_eq!("gemini".parse::<Provider>().unwrap(), Provider::Gemini);
    assert_eq!("openai".parse::<Provider>().unwrap(), Provider::OpenAI);
    assert_eq!("anthropic".parse::<Provider>().unwrap(), Provider::Anthropic);
    assert_eq!("deepseek".parse::<Provider>().unwrap(), Provider::DeepSeek);
    assert_eq!("openrouter".parse::<Provider>().unwrap(), Provider::OpenRouter);
    assert_eq!("zai".parse::<Provider>().unwrap(), Provider::ZAI);
    assert_eq!("moonshot".parse::<Provider>().unwrap(), Provider::Moonshot);
    assert_eq!("opencode-zen".parse::<Provider>().unwrap(), Provider::OpenCodeZen);
    assert_eq!("opencode-go".parse::<Provider>().unwrap(), Provider::OpenCodeGo);
    assert_eq!("lmstudio".parse::<Provider>().unwrap(), Provider::LmStudio);
    assert_eq!("llamacpp".parse::<Provider>().unwrap(), Provider::LlamaCpp);
    "invalid-provider".parse::<Provider>().unwrap_err();
}

#[test]
fn test_model_providers() {
    assert_eq!(ModelId::Gemini38Flash.provider(), Provider::Gemini);
    assert_eq!(ModelId::GPT56Sol.provider(), Provider::OpenAI);
    assert_eq!(ModelId::ClaudeOpus5.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet5.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet5.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ZaiGlm53.provider(), Provider::ZAI);
    assert_eq!(ModelId::OpenCodeGoMinimaxM3.provider(), Provider::OpenCodeGo);
    assert_eq!(ModelId::OllamaGptOss20b.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaGptOss120bCloud.provider(), Provider::OllamaCloud);
}

#[test]
fn test_provider_defaults() {
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::Gemini), ModelId::Gemini38Flash);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::OpenAI), ModelId::GPT56Sol);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::Anthropic), ModelId::ClaudeOpus55);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::DeepSeek), ModelId::DeepSeekFlash);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::Ollama), ModelId::OllamaGptOss20b);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::ZAI), ModelId::ZaiGlm53);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::OpenCodeZen), ModelId::ClaudeSonnet5);
    assert_eq!(ModelId::default_orchestrator_for_provider(Provider::OpenCodeGo), ModelId::OpenCodeGoMinimaxM3);

    assert_eq!(ModelId::default_single_for_provider(Provider::DeepSeek), ModelId::DeepSeekFlash);
    assert_eq!(ModelId::default_single_for_provider(Provider::Ollama), ModelId::OllamaGptOss20b);
    assert_eq!(ModelId::default_single_for_provider(Provider::ZAI), ModelId::ZaiGlm53);
    assert_eq!(ModelId::default_single_for_provider(Provider::OpenCodeZen), ModelId::ClaudeSonnet5);
    assert_eq!(ModelId::default_single_for_provider(Provider::OpenCodeGo), ModelId::OpenCodeGoMinimaxM3);
}

#[test]
fn test_provider_service_tier_support() {
    assert!(Provider::OpenAI.supports_service_tier(models::openai::GPT_5_6_SOL));
    assert!(Provider::OpenAI.supports_service_tier(models::openai::GPT_5_1));
    assert!(!Provider::OpenAI.supports_service_tier(models::openai::GPT_OSS_20B));
    assert!(!Provider::Anthropic.supports_service_tier(models::openai::GPT_5_6_SOL));
}

#[test]
fn test_model_defaults() {
    assert_eq!(ModelId::default(), ModelId::ClaudeSonnet5);
    assert_eq!(ModelId::default_orchestrator(), ModelId::ClaudeSonnet5);
}

#[test]
fn test_model_variants() {
    // Flash variants
    assert!(ModelId::Gemini38Flash.is_flash_variant());
    assert!(!ModelId::GPT56Sol.is_flash_variant());

    // Pro variants
    assert!(ModelId::GPT56Sol.is_pro_variant());
    assert!(ModelId::GPT56Sol.is_pro_variant());
    assert!(ModelId::ClaudeOpus5.is_pro_variant());
    assert!(ModelId::ClaudeSonnet5.is_pro_variant());
    assert!(ModelId::ZaiGlm53.is_pro_variant());
    assert!(!ModelId::Gemini38Flash.is_pro_variant());

    // Efficient variants
    assert!(ModelId::Gemini38Flash.is_efficient_variant());
    assert!(ModelId::GPT56Luna.is_efficient_variant());
    assert!(!ModelId::GPT56Sol.is_efficient_variant());

    // Top tier models
    assert!(ModelId::GPT56Sol.is_top_tier());
    assert!(ModelId::ClaudeOpus5.is_top_tier());
    assert!(ModelId::ClaudeSonnet5.is_top_tier());
    assert!(ModelId::ZaiGlm53.is_top_tier());
    assert!(ModelId::Gemini38Flash.is_top_tier());
    assert!(!ModelId::OpenAIGptOss20b.is_top_tier());
}

#[test]
fn test_model_generation() {
    // Gemini generations
    assert_eq!(ModelId::Gemini38Flash.generation(), "3.8");

    // OpenAI generations
    assert_eq!(ModelId::GPT56Sol.generation(), "5.6");
    assert_eq!(ModelId::GPT56Terra.generation(), "5.6");
    assert_eq!(ModelId::GPT56Luna.generation(), "5.6");

    // Anthropic generations
    assert_eq!(ModelId::ClaudeOpus5.generation(), "5");
    assert_eq!(ModelId::ClaudeSonnet5.generation(), "5");

    // Z.AI generations
    assert_eq!(ModelId::ZaiGlm53.generation(), "5.3");
}

#[test]
fn test_models_for_provider() {
    let gemini_models = ModelId::models_for_provider(Provider::Gemini);
    assert!(gemini_models.contains(&ModelId::Gemini38Flash));
    assert!(!gemini_models.contains(&ModelId::GPT56Sol));

    let openai_models = ModelId::models_for_provider(Provider::OpenAI);
    assert!(openai_models.contains(&ModelId::GPT56Sol));
    assert!(openai_models.contains(&ModelId::GPT56Sol));
    assert!(!openai_models.contains(&ModelId::Gemini38Flash));

    let anthropic_models = ModelId::models_for_provider(Provider::Anthropic);
    assert!(anthropic_models.contains(&ModelId::ClaudeOpus5));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet5));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet55));
    assert!(!anthropic_models.contains(&ModelId::GPT56Sol));

    let zai_models = ModelId::models_for_provider(Provider::ZAI);
    assert!(zai_models.contains(&ModelId::ZaiGlm53));

    let ollama_models = ModelId::models_for_provider(Provider::Ollama);
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20b));

    let ollama_cloud_models = ModelId::models_for_provider(Provider::OllamaCloud);
    assert!(ollama_cloud_models.contains(&ModelId::OllamaGptOss20bCloud));
    assert!(ollama_cloud_models.contains(&ModelId::OllamaGptOss120bCloud));
}

#[test]
fn test_fallback_models() {
    let fallbacks = ModelId::fallback_models();
    assert!(!fallbacks.is_empty());
    assert!(fallbacks.contains(&ModelId::Gemini38Flash));
    assert!(fallbacks.contains(&ModelId::GPT56Sol));
    assert!(fallbacks.contains(&ModelId::GPT56Sol));
    assert!(fallbacks.contains(&ModelId::OpenAIGptOss20b));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus5));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet5));
    assert!(fallbacks.contains(&ModelId::ZaiGlm53));
}

#[test]
fn test_reexported_model_id_provider_types() {
    let model: ModelId = ModelId::GPT56Sol;
    let provider: Provider = Provider::Moonshot;
    assert_eq!(model, ModelId::GPT56Sol);
    assert_eq!(provider, Provider::Moonshot);
}

#[test]
fn test_moonshot_and_openrouter_minimax_variants() {
    assert_eq!(models::moonshot::KIMI_K3.parse::<ModelId>().unwrap(), ModelId::MoonshotKimiK3);
    assert_eq!(ModelId::MoonshotKimiK3.provider(), Provider::Moonshot);
}
