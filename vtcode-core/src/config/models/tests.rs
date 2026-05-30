use super::*;
use crate::config::constants::models;

#[test]
fn test_model_string_conversion() {
    // Gemini models
    assert_eq!(
        ModelId::Gemini35Flash.as_str(),
        models::google::GEMINI_3_5_FLASH
    );
    assert_eq!(
        ModelId::Gemini31ProPreview.as_str(),
        models::google::GEMINI_3_1_PRO_PREVIEW
    );
    // OpenAI models
    assert_eq!(ModelId::GPT55.as_str(), models::GPT_5_5);
    assert_eq!(ModelId::GPT53Codex.as_str(), models::GPT_5_3_CODEX);
    assert_eq!(ModelId::GPT54Mini.as_str(), models::openai::GPT_5_4_MINI);
    assert_eq!(ModelId::GPT54Nano.as_str(), models::openai::GPT_5_4_NANO);
    // Anthropic models
    assert_eq!(ModelId::ClaudeOpus48.as_str(), models::CLAUDE_OPUS_4_8);
    assert_eq!(ModelId::ClaudeSonnet46.as_str(), models::CLAUDE_SONNET_4_6);
    assert_eq!(ModelId::ClaudeHaiku45.as_str(), models::CLAUDE_HAIKU_4_5);
    // DeepSeek models
    assert_eq!(
        ModelId::DeepSeekV4Pro.as_str(),
        models::deepseek::DEEPSEEK_V4_PRO
    );
    assert_eq!(
        ModelId::DeepSeekV4Flash.as_str(),
        models::deepseek::DEEPSEEK_V4_FLASH
    );
    // Hugging Face models
    assert_eq!(
        ModelId::HuggingFaceGlm5Novita.as_str(),
        models::huggingface::ZAI_GLM_5_NOVITA
    );
    assert_eq!(
        ModelId::HuggingFaceQwen3CoderNextNovita.as_str(),
        models::huggingface::QWEN3_CODER_NEXT_NOVITA
    );
    // Z.AI models
    assert_eq!(ModelId::ZaiGlm5.as_str(), models::zai::GLM_5);
    // OpenCode models
    assert_eq!(
        ModelId::OpenCodeZenGPT54.as_str(),
        models::opencode_zen::GPT_5_4
    );
    assert_eq!(
        ModelId::OpenCodeGoMinimaxM25.as_str(),
        models::opencode_go::MINIMAX_M2_5
    );
}

#[test]
fn test_model_from_string() {
    // Gemini models
    assert_eq!(
        models::google::GEMINI_3_5_FLASH.parse::<ModelId>().unwrap(),
        ModelId::Gemini35Flash
    );
    assert_eq!(
        models::google::GEMINI_3_1_PRO_PREVIEW
            .parse::<ModelId>()
            .unwrap(),
        ModelId::Gemini31ProPreview
    );
    // OpenAI models
    assert_eq!(models::GPT_5_5.parse::<ModelId>().unwrap(), ModelId::GPT55);
    assert_eq!(
        models::GPT_5_3_CODEX.parse::<ModelId>().unwrap(),
        ModelId::GPT53Codex
    );
    assert_eq!(
        models::openai::GPT_5_4_MINI.parse::<ModelId>().unwrap(),
        ModelId::GPT54Mini
    );
    assert_eq!(
        models::openai::GPT_5_4_NANO.parse::<ModelId>().unwrap(),
        ModelId::GPT54Nano
    );
    assert_eq!(
        models::openai::GPT_OSS_20B.parse::<ModelId>().unwrap(),
        ModelId::OpenAIGptOss20b
    );
    assert_eq!(
        models::openai::GPT_OSS_120B.parse::<ModelId>().unwrap(),
        ModelId::OpenAIGptOss120b
    );
    // Anthropic models
    assert_eq!(
        models::CLAUDE_SONNET_4_6.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet46
    );
    assert_eq!(
        models::CLAUDE_HAIKU_4_5.parse::<ModelId>().unwrap(),
        ModelId::ClaudeHaiku45
    );
    assert_eq!(
        models::CLAUDE_OPUS_4_8.parse::<ModelId>().unwrap(),
        ModelId::ClaudeOpus48
    );
    assert_eq!(
        models::CLAUDE_SONNET_4_6.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet46
    );
    // DeepSeek models
    assert_eq!(
        models::deepseek::DEEPSEEK_V4_PRO
            .parse::<ModelId>()
            .unwrap(),
        ModelId::DeepSeekV4Pro
    );
    assert_eq!(
        models::deepseek::DEEPSEEK_V4_FLASH
            .parse::<ModelId>()
            .unwrap(),
        ModelId::DeepSeekV4Flash
    );
    // Hugging Face models
    assert_eq!(
        models::huggingface::ZAI_GLM_5_NOVITA
            .parse::<ModelId>()
            .unwrap(),
        ModelId::HuggingFaceGlm5Novita
    );
    assert_eq!(
        models::huggingface::QWEN3_CODER_NEXT_NOVITA
            .parse::<ModelId>()
            .unwrap(),
        ModelId::HuggingFaceQwen3CoderNextNovita
    );
    // Z.AI models
    assert_eq!(
        models::zai::GLM_5.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm5
    );
    assert_eq!(
        models::zai::GLM_5_LEGACY.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm5
    );
    assert_eq!(
        "opencode/gpt-5.4".parse::<ModelId>().unwrap(),
        ModelId::OpenCodeZenGPT54
    );
    assert_eq!(
        "opencode-go/minimax-m2.5".parse::<ModelId>().unwrap(),
        ModelId::OpenCodeGoMinimaxM25
    );
    // Invalid model
    "invalid-model".parse::<ModelId>().unwrap_err();
}

#[test]
fn test_provider_parsing() {
    assert_eq!("gemini".parse::<Provider>().unwrap(), Provider::Gemini);
    assert_eq!("openai".parse::<Provider>().unwrap(), Provider::OpenAI);
    assert_eq!(
        "anthropic".parse::<Provider>().unwrap(),
        Provider::Anthropic
    );
    assert_eq!("deepseek".parse::<Provider>().unwrap(), Provider::DeepSeek);
    assert_eq!(
        "openrouter".parse::<Provider>().unwrap(),
        Provider::OpenRouter
    );
    assert_eq!("zai".parse::<Provider>().unwrap(), Provider::ZAI);
    assert_eq!("moonshot".parse::<Provider>().unwrap(), Provider::Moonshot);
    assert_eq!(
        "opencode-zen".parse::<Provider>().unwrap(),
        Provider::OpenCodeZen
    );
    assert_eq!(
        "opencode-go".parse::<Provider>().unwrap(),
        Provider::OpenCodeGo
    );
    assert_eq!("lmstudio".parse::<Provider>().unwrap(), Provider::LmStudio);
    assert_eq!("llamacpp".parse::<Provider>().unwrap(), Provider::LlamaCpp);
    "invalid-provider".parse::<Provider>().unwrap_err();
}

#[test]
fn test_model_providers() {
    assert_eq!(ModelId::Gemini35Flash.provider(), Provider::Gemini);
    assert_eq!(ModelId::GPT55.provider(), Provider::OpenAI);
    assert_eq!(ModelId::ClaudeOpus48.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet46.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeHaiku45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::DeepSeekV4Pro.provider(), Provider::DeepSeek);
    assert_eq!(ModelId::ZaiGlm5.provider(), Provider::ZAI);
    assert_eq!(ModelId::OpenCodeZenGPT54.provider(), Provider::OpenCodeZen);
    assert_eq!(
        ModelId::OpenCodeGoMinimaxM25.provider(),
        Provider::OpenCodeGo
    );
    assert_eq!(ModelId::OllamaGptOss20b.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaGptOss120bCloud.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaQwen317b.provider(), Provider::Ollama);
}

#[test]
fn test_provider_defaults() {
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Gemini),
        ModelId::Gemini31ProPreview
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenAI),
        ModelId::GPT54
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Anthropic),
        ModelId::ClaudeOpus48
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekV4Pro
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Ollama),
        ModelId::OllamaGptOss20b
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::ZAI),
        ModelId::ZaiGlm5
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenCodeZen),
        ModelId::OpenCodeZenGPT54
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenCodeGo),
        ModelId::OpenCodeGoMinimaxM27
    );

    assert_eq!(
        ModelId::default_single_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekV4Pro
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::Ollama),
        ModelId::OllamaGptOss20b
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::ZAI),
        ModelId::ZaiGlm5
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::OpenCodeZen),
        ModelId::OpenCodeZenGPT54
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::OpenCodeGo),
        ModelId::OpenCodeGoMinimaxM27
    );
}

#[test]
fn test_provider_service_tier_support() {
    assert!(Provider::OpenAI.supports_service_tier(models::GPT_5));
    assert!(Provider::OpenAI.supports_service_tier(models::openai::O3));
    assert!(!Provider::OpenAI.supports_service_tier(models::openai::GPT_OSS_20B));
    assert!(!Provider::Anthropic.supports_service_tier(models::GPT_5));
}

#[test]
fn test_model_defaults() {
    assert_eq!(ModelId::default(), ModelId::Gemini35Flash);
    assert_eq!(ModelId::default_orchestrator(), ModelId::Gemini31ProPreview);
}

#[test]
fn test_model_variants() {
    // Flash variants
    assert!(ModelId::Gemini35Flash.is_flash_variant());
    assert!(!ModelId::GPT55.is_flash_variant());

    // Pro variants
    assert!(ModelId::GPT55.is_pro_variant());
    assert!(ModelId::GPT53Codex.is_pro_variant());
    assert!(ModelId::ClaudeOpus48.is_pro_variant());
    assert!(ModelId::ClaudeSonnet46.is_pro_variant());
    assert!(ModelId::DeepSeekV4Pro.is_pro_variant());
    assert!(ModelId::ZaiGlm5.is_pro_variant());
    assert!(!ModelId::Gemini35Flash.is_pro_variant());

    // Efficient variants
    assert!(ModelId::Gemini35Flash.is_efficient_variant());
    assert!(ModelId::GPT54Mini.is_efficient_variant());
    assert!(ModelId::ClaudeHaiku45.is_efficient_variant());
    assert!(ModelId::DeepSeekV4Flash.is_efficient_variant());
    assert!(!ModelId::GPT55.is_efficient_variant());

    // Top tier models
    assert!(ModelId::GPT55.is_top_tier());
    assert!(ModelId::GPT53Codex.is_top_tier());
    assert!(ModelId::ClaudeOpus48.is_top_tier());
    assert!(ModelId::ClaudeSonnet46.is_top_tier());
    assert!(ModelId::DeepSeekV4Pro.is_top_tier());
    assert!(ModelId::ZaiGlm5.is_top_tier());
    assert!(ModelId::Gemini35Flash.is_top_tier());
    assert!(!ModelId::ClaudeHaiku45.is_top_tier());
}

#[test]
fn test_model_generation() {
    // Gemini generations
    assert_eq!(ModelId::Gemini35Flash.generation(), "3.5");

    // OpenAI generations
    assert_eq!(ModelId::GPT55.generation(), "5.5");
    assert_eq!(ModelId::GPT53Codex.generation(), "5.3");
    assert_eq!(ModelId::GPT54Mini.generation(), "5.4");
    assert_eq!(ModelId::GPT54Nano.generation(), "5.4");

    // Anthropic generations
    assert_eq!(ModelId::ClaudeOpus48.generation(), "4.8");
    assert_eq!(ModelId::ClaudeSonnet46.generation(), "4.6");
    assert_eq!(ModelId::ClaudeHaiku45.generation(), "4.5");

    // DeepSeek generations
    assert_eq!(ModelId::DeepSeekV4Pro.generation(), "4");
    assert_eq!(ModelId::DeepSeekV4Flash.generation(), "4");

    // Z.AI generations
    assert_eq!(ModelId::ZaiGlm5.generation(), "5");
}

#[test]
fn test_models_for_provider() {
    let gemini_models = ModelId::models_for_provider(Provider::Gemini);
    assert!(gemini_models.contains(&ModelId::Gemini35Flash));
    assert!(!gemini_models.contains(&ModelId::GPT55));

    let openai_models = ModelId::models_for_provider(Provider::OpenAI);
    assert!(openai_models.contains(&ModelId::GPT55));
    assert!(openai_models.contains(&ModelId::GPT53Codex));
    assert!(!openai_models.contains(&ModelId::Gemini35Flash));

    let anthropic_models = ModelId::models_for_provider(Provider::Anthropic);
    assert!(anthropic_models.contains(&ModelId::ClaudeOpus48));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet46));
    assert!(anthropic_models.contains(&ModelId::ClaudeHaiku45));
    assert!(!anthropic_models.contains(&ModelId::GPT55));

    let deepseek_models = ModelId::models_for_provider(Provider::DeepSeek);
    assert!(deepseek_models.contains(&ModelId::DeepSeekV4Pro));
    assert!(deepseek_models.contains(&ModelId::DeepSeekV4Flash));

    let zai_models = ModelId::models_for_provider(Provider::ZAI);
    assert!(zai_models.contains(&ModelId::ZaiGlm5));

    let ollama_models = ModelId::models_for_provider(Provider::Ollama);
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20b));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss120bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaQwen317b));
}

#[test]
fn test_fallback_models() {
    let fallbacks = ModelId::fallback_models();
    assert!(!fallbacks.is_empty());
    assert!(fallbacks.contains(&ModelId::Gemini35Flash));
    assert!(fallbacks.contains(&ModelId::GPT54));
    assert!(fallbacks.contains(&ModelId::GPT55));
    assert!(fallbacks.contains(&ModelId::OpenAIGptOss20b));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus48));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet46));
    assert!(fallbacks.contains(&ModelId::DeepSeekV4Pro));
    assert!(fallbacks.contains(&ModelId::ZaiGlm5));
}

#[test]
fn test_reexported_model_id_provider_types() {
    let model: ModelId = ModelId::GPT53Codex;
    let provider: Provider = Provider::Moonshot;
    assert_eq!(model, ModelId::GPT53Codex);
    assert_eq!(provider, Provider::Moonshot);
}

#[test]
fn test_moonshot_and_openrouter_minimax_variants() {
    assert_eq!(
        models::moonshot::KIMI_K2_6.parse::<ModelId>().unwrap(),
        ModelId::MoonshotKimiK26
    );
    assert_eq!(
        "minimax/minimax-m2.5".parse::<ModelId>().unwrap(),
        ModelId::OpenRouterMinimaxM25
    );
    assert_eq!(ModelId::MoonshotKimiK26.provider(), Provider::Moonshot);
    assert_eq!(
        ModelId::OpenRouterMinimaxM25.provider(),
        Provider::OpenRouter
    );
}
