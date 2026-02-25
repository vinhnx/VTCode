use super::*;
use crate::config::constants::models;

#[test]
fn test_model_string_conversion() {
    // Gemini models
    assert_eq!(
        ModelId::Gemini3FlashPreview.as_str(),
        models::google::GEMINI_3_FLASH_PREVIEW
    );
    assert_eq!(
        ModelId::Gemini3ProPreview.as_str(),
        models::google::GEMINI_3_PRO_PREVIEW
    );
    // OpenAI models
    assert_eq!(ModelId::GPT5.as_str(), models::GPT_5);
    assert_eq!(ModelId::GPT5Codex.as_str(), models::GPT_5_CODEX);
    assert_eq!(ModelId::GPT5Mini.as_str(), models::GPT_5_MINI);
    assert_eq!(ModelId::GPT5Nano.as_str(), models::GPT_5_NANO);
    assert_eq!(ModelId::CodexMiniLatest.as_str(), models::CODEX_MINI_LATEST);
    // Anthropic models
    assert_eq!(ModelId::ClaudeOpus46.as_str(), models::CLAUDE_OPUS_4_6);
    assert_eq!(ModelId::ClaudeSonnet46.as_str(), models::CLAUDE_SONNET_4_6);
    assert_eq!(ModelId::ClaudeSonnet45.as_str(), models::CLAUDE_SONNET_4_5);
    assert_eq!(ModelId::ClaudeHaiku45.as_str(), models::CLAUDE_HAIKU_4_5);
    assert_eq!(ModelId::ClaudeOpus41.as_str(), models::CLAUDE_OPUS_4_1);
    // DeepSeek models
    assert_eq!(ModelId::DeepSeekChat.as_str(), models::DEEPSEEK_CHAT);
    assert_eq!(
        ModelId::DeepSeekReasoner.as_str(),
        models::DEEPSEEK_REASONER
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
    // xAI models
    assert_eq!(ModelId::XaiGrok4.as_str(), models::xai::GROK_4);
    assert_eq!(ModelId::XaiGrok4Mini.as_str(), models::xai::GROK_4_MINI);
    assert_eq!(ModelId::XaiGrok4Code.as_str(), models::xai::GROK_4_CODE);
    assert_eq!(
        ModelId::XaiGrok4CodeLatest.as_str(),
        models::xai::GROK_4_CODE_LATEST
    );
    assert_eq!(ModelId::XaiGrok4Vision.as_str(), models::xai::GROK_4_VISION);
    // Z.AI models
    assert_eq!(ModelId::ZaiGlm5.as_str(), models::zai::GLM_5);
}

#[test]
fn test_model_from_string() {
    // Gemini models
    assert_eq!(
        models::google::GEMINI_3_FLASH_PREVIEW
            .parse::<ModelId>()
            .unwrap(),
        ModelId::Gemini3FlashPreview
    );
    assert_eq!(
        models::google::GEMINI_3_PRO_PREVIEW
            .parse::<ModelId>()
            .unwrap(),
        ModelId::Gemini3ProPreview
    );
    // OpenAI models
    assert_eq!(models::GPT_5.parse::<ModelId>().unwrap(), ModelId::GPT5);
    assert_eq!(
        models::GPT_5_CODEX.parse::<ModelId>().unwrap(),
        ModelId::GPT5Codex
    );
    assert_eq!(
        models::GPT_5_MINI.parse::<ModelId>().unwrap(),
        ModelId::GPT5Mini
    );
    assert_eq!(
        models::GPT_5_NANO.parse::<ModelId>().unwrap(),
        ModelId::GPT5Nano
    );
    assert_eq!(
        models::CODEX_MINI_LATEST.parse::<ModelId>().unwrap(),
        ModelId::CodexMiniLatest
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
        models::CLAUDE_SONNET_4_5.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet45
    );
    assert_eq!(
        models::CLAUDE_HAIKU_4_5.parse::<ModelId>().unwrap(),
        ModelId::ClaudeHaiku45
    );
    assert_eq!(
        models::CLAUDE_OPUS_4_1.parse::<ModelId>().unwrap(),
        ModelId::ClaudeOpus41
    );
    assert_eq!(
        models::CLAUDE_OPUS_4_6.parse::<ModelId>().unwrap(),
        ModelId::ClaudeOpus46
    );
    assert_eq!(
        models::CLAUDE_SONNET_4_6.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet46
    );
    // DeepSeek models
    assert_eq!(
        models::DEEPSEEK_CHAT.parse::<ModelId>().unwrap(),
        ModelId::DeepSeekChat
    );
    assert_eq!(
        models::DEEPSEEK_REASONER.parse::<ModelId>().unwrap(),
        ModelId::DeepSeekReasoner
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
    // xAI models
    assert_eq!(
        models::xai::GROK_4.parse::<ModelId>().unwrap(),
        ModelId::XaiGrok4
    );
    assert_eq!(
        models::xai::GROK_4_MINI.parse::<ModelId>().unwrap(),
        ModelId::XaiGrok4Mini
    );
    assert_eq!(
        models::xai::GROK_4_CODE.parse::<ModelId>().unwrap(),
        ModelId::XaiGrok4Code
    );
    assert_eq!(
        models::xai::GROK_4_CODE_LATEST.parse::<ModelId>().unwrap(),
        ModelId::XaiGrok4CodeLatest
    );
    assert_eq!(
        models::xai::GROK_4_VISION.parse::<ModelId>().unwrap(),
        ModelId::XaiGrok4Vision
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
    // Invalid model
    assert!("invalid-model".parse::<ModelId>().is_err());
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
    assert_eq!("xai".parse::<Provider>().unwrap(), Provider::XAI);
    assert_eq!("zai".parse::<Provider>().unwrap(), Provider::ZAI);
    assert_eq!("moonshot".parse::<Provider>().unwrap(), Provider::Moonshot);
    assert_eq!("lmstudio".parse::<Provider>().unwrap(), Provider::LmStudio);
    assert!("invalid-provider".parse::<Provider>().is_err());
}

#[test]
fn test_model_providers() {
    assert_eq!(ModelId::Gemini3FlashPreview.provider(), Provider::Gemini);
    assert_eq!(ModelId::GPT5.provider(), Provider::OpenAI);
    assert_eq!(ModelId::GPT5Codex.provider(), Provider::OpenAI);
    assert_eq!(ModelId::ClaudeOpus46.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet46.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeHaiku45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::DeepSeekChat.provider(), Provider::DeepSeek);
    assert_eq!(ModelId::XaiGrok4.provider(), Provider::XAI);
    assert_eq!(ModelId::ZaiGlm5.provider(), Provider::ZAI);
    assert_eq!(ModelId::OllamaGptOss20b.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaGptOss120bCloud.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaQwen317b.provider(), Provider::Ollama);
}

#[test]
fn test_provider_defaults() {
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Gemini),
        ModelId::Gemini3ProPreview
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenAI),
        ModelId::GPT5
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Anthropic),
        ModelId::ClaudeOpus45
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekReasoner
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::XAI),
        ModelId::XaiGrok4
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
        ModelId::default_subagent_for_provider(Provider::Gemini),
        ModelId::Gemini3FlashPreview
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::OpenAI),
        ModelId::GPT5Mini
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::Anthropic),
        ModelId::ClaudeSonnet45
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekChat
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::XAI),
        ModelId::XaiGrok4Code
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::Ollama),
        ModelId::OllamaQwen317b
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::ZAI),
        ModelId::OllamaGlm5Cloud
    );

    assert_eq!(
        ModelId::default_single_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekReasoner
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::Ollama),
        ModelId::OllamaGptOss20b
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::ZAI),
        ModelId::ZaiGlm5
    );
}

#[test]
fn test_model_defaults() {
    assert_eq!(ModelId::default(), ModelId::Gemini3FlashPreview);
    assert_eq!(ModelId::default_orchestrator(), ModelId::Gemini3ProPreview);
    assert_eq!(ModelId::default_subagent(), ModelId::Gemini3FlashPreview);
}

#[test]
fn test_model_variants() {
    // Flash variants
    assert!(ModelId::Gemini3FlashPreview.is_flash_variant());
    assert!(!ModelId::GPT5.is_flash_variant());

    // Pro variants
    assert!(ModelId::GPT5.is_pro_variant());
    assert!(ModelId::GPT5Codex.is_pro_variant());
    assert!(ModelId::ClaudeOpus46.is_pro_variant());
    assert!(ModelId::ClaudeSonnet46.is_pro_variant());
    assert!(ModelId::DeepSeekReasoner.is_pro_variant());
    assert!(ModelId::ZaiGlm5.is_pro_variant());
    assert!(!ModelId::Gemini3FlashPreview.is_pro_variant());

    // Efficient variants
    assert!(ModelId::Gemini3FlashPreview.is_efficient_variant());
    assert!(ModelId::GPT5Mini.is_efficient_variant());
    assert!(ModelId::ClaudeHaiku45.is_efficient_variant());
    assert!(ModelId::XaiGrok4Code.is_efficient_variant());
    assert!(ModelId::DeepSeekChat.is_efficient_variant());
    assert!(!ModelId::GPT5.is_efficient_variant());

    // Top tier models
    assert!(ModelId::GPT5.is_top_tier());
    assert!(ModelId::GPT5Codex.is_top_tier());
    assert!(ModelId::ClaudeOpus46.is_top_tier());
    assert!(ModelId::ClaudeSonnet45.is_top_tier());
    assert!(ModelId::XaiGrok4.is_top_tier());
    assert!(ModelId::XaiGrok4CodeLatest.is_top_tier());
    assert!(ModelId::DeepSeekReasoner.is_top_tier());
    assert!(ModelId::ZaiGlm5.is_top_tier());
    assert!(ModelId::Gemini3FlashPreview.is_top_tier());
    assert!(!ModelId::ClaudeHaiku45.is_top_tier());
}

#[test]
fn test_model_generation() {
    // Gemini generations
    assert_eq!(ModelId::Gemini3FlashPreview.generation(), "3");

    // OpenAI generations
    assert_eq!(ModelId::GPT5.generation(), "5");
    assert_eq!(ModelId::GPT5Codex.generation(), "5");
    assert_eq!(ModelId::GPT5Mini.generation(), "5");
    assert_eq!(ModelId::GPT5Nano.generation(), "5");
    assert_eq!(ModelId::CodexMiniLatest.generation(), "5");

    // Anthropic generations
    assert_eq!(ModelId::ClaudeOpus46.generation(), "4.6");
    assert_eq!(ModelId::ClaudeSonnet46.generation(), "4.6");
    assert_eq!(ModelId::ClaudeSonnet45.generation(), "4.5");
    assert_eq!(ModelId::ClaudeHaiku45.generation(), "4.5");
    assert_eq!(ModelId::ClaudeOpus41.generation(), "4.1");

    // DeepSeek generations
    assert_eq!(ModelId::DeepSeekChat.generation(), "V3.2-Exp");
    assert_eq!(ModelId::DeepSeekReasoner.generation(), "V3.2-Exp");

    // xAI generations
    assert_eq!(ModelId::XaiGrok4.generation(), "4");
    assert_eq!(ModelId::XaiGrok4Mini.generation(), "4");
    assert_eq!(ModelId::XaiGrok4Code.generation(), "4");
    assert_eq!(ModelId::XaiGrok4CodeLatest.generation(), "4");
    assert_eq!(ModelId::XaiGrok4Vision.generation(), "4");
    // Z.AI generations
    assert_eq!(ModelId::ZaiGlm5.generation(), "5");
}

#[test]
fn test_models_for_provider() {
    let gemini_models = ModelId::models_for_provider(Provider::Gemini);
    assert!(gemini_models.contains(&ModelId::Gemini3FlashPreview));
    assert!(!gemini_models.contains(&ModelId::GPT5));

    let openai_models = ModelId::models_for_provider(Provider::OpenAI);
    assert!(openai_models.contains(&ModelId::GPT5));
    assert!(openai_models.contains(&ModelId::GPT5Codex));
    assert!(!openai_models.contains(&ModelId::Gemini3FlashPreview));

    let anthropic_models = ModelId::models_for_provider(Provider::Anthropic);
    assert!(anthropic_models.contains(&ModelId::ClaudeOpus46));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet46));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet45));
    assert!(anthropic_models.contains(&ModelId::ClaudeHaiku45));
    assert!(!anthropic_models.contains(&ModelId::GPT5));

    let deepseek_models = ModelId::models_for_provider(Provider::DeepSeek);
    assert!(deepseek_models.contains(&ModelId::DeepSeekChat));
    assert!(deepseek_models.contains(&ModelId::DeepSeekReasoner));

    let xai_models = ModelId::models_for_provider(Provider::XAI);
    assert!(xai_models.contains(&ModelId::XaiGrok4));
    assert!(xai_models.contains(&ModelId::XaiGrok4Mini));
    assert!(xai_models.contains(&ModelId::XaiGrok4Code));
    assert!(xai_models.contains(&ModelId::XaiGrok4CodeLatest));
    assert!(xai_models.contains(&ModelId::XaiGrok4Vision));

    let zai_models = ModelId::models_for_provider(Provider::ZAI);
    assert!(zai_models.contains(&ModelId::ZaiGlm5));

    let ollama_models = ModelId::models_for_provider(Provider::Ollama);
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20b));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss120bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaQwen317b));
    assert!(ollama_models.contains(&ModelId::OllamaDeepseekV32Cloud));
}

#[test]
fn test_fallback_models() {
    let fallbacks = ModelId::fallback_models();
    assert!(!fallbacks.is_empty());
    assert!(fallbacks.contains(&ModelId::Gemini3FlashPreview));
    assert!(fallbacks.contains(&ModelId::GPT5));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus41));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet46));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet45));
    assert!(fallbacks.contains(&ModelId::DeepSeekReasoner));
    assert!(fallbacks.contains(&ModelId::XaiGrok4));
    assert!(fallbacks.contains(&ModelId::ZaiGlm5));
    assert!(fallbacks.contains(&ModelId::OpenRouterGrokCodeFast1));
}
