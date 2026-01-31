use super::*;
use crate::config::constants::models;
use vtcode_config::models::openrouter_generated;

#[test]
fn test_model_string_conversion() {
    // Gemini models
    assert_eq!(
        ModelId::Gemini25FlashPreview.as_str(),
        models::GEMINI_2_5_FLASH_PREVIEW
    );
    assert_eq!(ModelId::Gemini25Flash.as_str(), models::GEMINI_2_5_FLASH);
    assert_eq!(
        ModelId::Gemini25FlashLite.as_str(),
        models::GEMINI_2_5_FLASH_LITE
    );
    assert_eq!(ModelId::Gemini25Pro.as_str(), models::GEMINI_2_5_PRO);
    // OpenAI models
    assert_eq!(ModelId::GPT5.as_str(), models::GPT_5);
    assert_eq!(ModelId::GPT5Codex.as_str(), models::GPT_5_CODEX);
    assert_eq!(ModelId::GPT5Mini.as_str(), models::GPT_5_MINI);
    assert_eq!(ModelId::GPT5Nano.as_str(), models::GPT_5_NANO);
    assert_eq!(ModelId::CodexMiniLatest.as_str(), models::CODEX_MINI_LATEST);
    // Anthropic models
    assert_eq!(ModelId::ClaudeSonnet45.as_str(), models::CLAUDE_SONNET_4_5);
    assert_eq!(ModelId::ClaudeHaiku45.as_str(), models::CLAUDE_HAIKU_4_5);
    assert_eq!(ModelId::ClaudeSonnet4.as_str(), models::CLAUDE_SONNET_4_0);
    assert_eq!(ModelId::ClaudeOpus41.as_str(), models::CLAUDE_OPUS_4_1);
    // DeepSeek models
    assert_eq!(ModelId::DeepSeekChat.as_str(), models::DEEPSEEK_CHAT);
    assert_eq!(
        ModelId::DeepSeekReasoner.as_str(),
        models::DEEPSEEK_REASONER
    );
    // Hugging Face models
    assert_eq!(
        ModelId::HuggingFaceDeepseekV32.as_str(),
        models::huggingface::DEEPSEEK_V32
    );
    assert_eq!(
        ModelId::HuggingFaceOpenAIGptOss20b.as_str(),
        models::huggingface::OPENAI_GPT_OSS_20B
    );
    assert_eq!(
        ModelId::HuggingFaceKimiK25Novita.as_str(),
        models::huggingface::MOONSHOT_KIMI_K2_5_NOVITA
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
    assert_eq!(ModelId::ZaiGlm46.as_str(), models::zai::GLM_4_6);
    assert_eq!(ModelId::ZaiGlm45.as_str(), models::zai::GLM_4_5);
    assert_eq!(ModelId::ZaiGlm45Air.as_str(), models::zai::GLM_4_5_AIR);
    assert_eq!(ModelId::ZaiGlm45X.as_str(), models::zai::GLM_4_5_X);
    assert_eq!(ModelId::ZaiGlm45Airx.as_str(), models::zai::GLM_4_5_AIRX);
    assert_eq!(ModelId::ZaiGlm45Flash.as_str(), models::zai::GLM_4_5_FLASH);
    assert_eq!(
        ModelId::ZaiGlm432b0414128k.as_str(),
        models::zai::GLM_4_32B_0414_128K
    );
    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.as_str(), entry.id);
    }
}

#[test]
fn test_model_from_string() {
    // Gemini models
    assert_eq!(
        models::GEMINI_2_5_FLASH_PREVIEW.parse::<ModelId>().unwrap(),
        ModelId::Gemini25FlashPreview
    );
    assert_eq!(
        models::GEMINI_2_5_FLASH.parse::<ModelId>().unwrap(),
        ModelId::Gemini25Flash
    );
    assert_eq!(
        models::GEMINI_2_5_FLASH_LITE.parse::<ModelId>().unwrap(),
        ModelId::Gemini25FlashLite
    );
    assert_eq!(
        models::GEMINI_2_5_PRO.parse::<ModelId>().unwrap(),
        ModelId::Gemini25Pro
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
    assert_eq!(
        models::huggingface::MOONSHOT_KIMI_K2_5_NOVITA
            .parse::<ModelId>()
            .unwrap(),
        ModelId::HuggingFaceKimiK25Novita
    );
    // Anthropic models
    assert_eq!(
        models::CLAUDE_SONNET_4_5.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet45
    );
    assert_eq!(
        models::CLAUDE_SONNET_4_5_20250929
            .parse::<ModelId>()
            .unwrap(),
        ModelId::ClaudeSonnet45
    );
    assert_eq!(
        models::CLAUDE_SONNET_4_0.parse::<ModelId>().unwrap(),
        ModelId::ClaudeSonnet4
    );
    assert_eq!(
        models::CLAUDE_OPUS_4_1.parse::<ModelId>().unwrap(),
        ModelId::ClaudeOpus41
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
    // Hugging Face models (non-overlapping)
    assert_eq!(
        models::huggingface::DEEPSEEK_V32
            .parse::<ModelId>()
            .unwrap(),
        ModelId::HuggingFaceDeepseekV32
    );
    // Removed / invalid HF models should not parse
    assert!("minimaxai/MiniMax-M2".parse::<ModelId>().is_err());
    assert!(
        "qwen/Qwen3-Coder-30B-A3B-Instruct"
            .parse::<ModelId>()
            .is_err()
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
        models::zai::GLM_4_7.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm47
    );
    assert_eq!(
        models::zai::GLM_4_6.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm46
    );
    assert_eq!(
        models::zai::GLM_4_5.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm45
    );
    assert_eq!(
        models::zai::GLM_4_5_AIR.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm45Air
    );
    assert_eq!(
        models::zai::GLM_4_5_X.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm45X
    );
    assert_eq!(
        models::zai::GLM_4_5_AIRX.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm45Airx
    );
    assert_eq!(
        models::zai::GLM_4_5_FLASH.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm45Flash
    );
    assert_eq!(
        models::zai::GLM_4_32B_0414_128K.parse::<ModelId>().unwrap(),
        ModelId::ZaiGlm432b0414128k
    );
    assert_eq!(
        models::moonshot::KIMI_K2_5.parse::<ModelId>().unwrap(),
        ModelId::MoonshotKimiK25
    );
    for entry in openrouter_generated::ENTRIES {
        assert_eq!(
            entry.id.parse::<ModelId>().unwrap().as_str(),
            entry.variant.as_str()
        );
    }
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
    assert_eq!(ModelId::Gemini25FlashPreview.provider(), Provider::Gemini);
    assert_eq!(ModelId::GPT5.provider(), Provider::OpenAI);
    assert_eq!(ModelId::GPT5Codex.provider(), Provider::OpenAI);
    assert_eq!(ModelId::ClaudeSonnet45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeHaiku45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet4.provider(), Provider::Anthropic);
    assert_eq!(ModelId::DeepSeekChat.provider(), Provider::DeepSeek);
    assert_eq!(ModelId::XaiGrok4.provider(), Provider::XAI);
    assert_eq!(ModelId::ZaiGlm47.provider(), Provider::ZAI);
    assert_eq!(ModelId::ZaiGlm46.provider(), Provider::ZAI);
    assert_eq!(ModelId::OllamaGptOss20b.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaGptOss120bCloud.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaQwen317b.provider(), Provider::Ollama);
    assert_eq!(
        ModelId::LmStudioMetaLlama38BInstruct.provider(),
        Provider::LmStudio
    );
    assert_eq!(
        ModelId::LmStudioMetaLlama318BInstruct.provider(),
        Provider::LmStudio
    );
    assert_eq!(
        ModelId::LmStudioQwen257BInstruct.provider(),
        Provider::LmStudio
    );
    assert_eq!(ModelId::LmStudioGemma22BIt.provider(), Provider::LmStudio);
    assert_eq!(ModelId::LmStudioGemma29BIt.provider(), Provider::LmStudio);
    assert_eq!(
        ModelId::LmStudioPhi31Mini4kInstruct.provider(),
        Provider::LmStudio
    );
    assert_eq!(
        ModelId::OpenRouterGrokCodeFast1.provider(),
        Provider::OpenRouter
    );
    assert_eq!(
        ModelId::OpenRouterAnthropicClaudeSonnet45.provider(),
        Provider::OpenRouter
    );

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(
            entry.variant.provider().to_string(),
            Provider::OpenRouter.to_string()
        );
    }
}

#[test]
fn test_provider_defaults() {
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Gemini),
        ModelId::Gemini25Pro
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
        ModelId::default_orchestrator_for_provider(Provider::OpenRouter),
        ModelId::OpenRouterGrokCodeFast1
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
        ModelId::default_orchestrator_for_provider(Provider::LmStudio),
        ModelId::LmStudioMetaLlama318BInstruct
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::ZAI),
        ModelId::ZaiGlm47
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Moonshot),
        ModelId::MoonshotKimiK25
    );

    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::Gemini),
        ModelId::Gemini25FlashPreview
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
        ModelId::default_subagent_for_provider(Provider::OpenRouter),
        ModelId::OpenRouterGrokCodeFast1
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
        ModelId::default_subagent_for_provider(Provider::LmStudio),
        ModelId::LmStudioQwen257BInstruct
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::ZAI),
        ModelId::ZaiGlm45Flash
    );
    assert_eq!(
        ModelId::default_subagent_for_provider(Provider::Moonshot),
        ModelId::MoonshotKimiK25
    );

    assert_eq!(
        ModelId::default_single_for_provider(Provider::DeepSeek),
        ModelId::DeepSeekReasoner
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::Moonshot),
        ModelId::MoonshotKimiK25
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::Ollama),
        ModelId::OllamaGptOss20b
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::LmStudio),
        ModelId::LmStudioMetaLlama318BInstruct
    );
    assert_eq!(
        ModelId::default_single_for_provider(Provider::ZAI),
        ModelId::ZaiGlm47
    );
}

#[test]
fn test_model_defaults() {
    assert_eq!(ModelId::default(), ModelId::Gemini25FlashPreview);
    assert_eq!(ModelId::default_orchestrator(), ModelId::Gemini25Pro);
    assert_eq!(ModelId::default_subagent(), ModelId::Gemini25FlashPreview);
}

#[test]
fn test_model_variants() {
    // Flash variants
    assert!(ModelId::Gemini25FlashPreview.is_flash_variant());
    assert!(ModelId::Gemini25Flash.is_flash_variant());
    assert!(ModelId::Gemini25FlashLite.is_flash_variant());
    assert!(!ModelId::GPT5.is_flash_variant());
    assert!(ModelId::ZaiGlm45Flash.is_flash_variant());

    // Pro variants
    assert!(ModelId::Gemini25Pro.is_pro_variant());
    assert!(ModelId::GPT52.is_pro_variant());
    assert!(ModelId::GPT5.is_pro_variant());
    assert!(ModelId::GPT5Codex.is_pro_variant());
    assert!(ModelId::DeepSeekReasoner.is_pro_variant());
    assert!(ModelId::ZaiGlm47.is_pro_variant());
    assert!(ModelId::ZaiGlm46.is_pro_variant());
    assert!(!ModelId::Gemini25FlashPreview.is_pro_variant());

    // Efficient variants
    assert!(ModelId::Gemini25FlashPreview.is_efficient_variant());
    assert!(ModelId::Gemini25Flash.is_efficient_variant());
    assert!(ModelId::Gemini25FlashLite.is_efficient_variant());
    assert!(ModelId::GPT5Mini.is_efficient_variant());
    assert!(ModelId::ClaudeHaiku45.is_efficient_variant());
    assert!(ModelId::XaiGrok4Code.is_efficient_variant());
    assert!(ModelId::DeepSeekChat.is_efficient_variant());
    assert!(ModelId::ZaiGlm45Air.is_efficient_variant());
    assert!(ModelId::ZaiGlm45Airx.is_efficient_variant());
    assert!(ModelId::ZaiGlm45Flash.is_efficient_variant());
    assert!(!ModelId::GPT5.is_efficient_variant());

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.is_efficient_variant(), entry.efficient);
    }

    // Top tier models
    assert!(ModelId::Gemini25Pro.is_top_tier());
    assert!(ModelId::GPT52.is_top_tier());
    assert!(ModelId::GPT5.is_top_tier());
    assert!(ModelId::GPT5Codex.is_top_tier());
    assert!(ModelId::ClaudeSonnet45.is_top_tier());
    assert!(ModelId::ClaudeSonnet4.is_top_tier());
    assert!(ModelId::XaiGrok4.is_top_tier());
    assert!(ModelId::XaiGrok4CodeLatest.is_top_tier());
    assert!(ModelId::DeepSeekReasoner.is_top_tier());
    assert!(ModelId::ZaiGlm47.is_top_tier());
    assert!(ModelId::ZaiGlm46.is_top_tier());
    assert!(!ModelId::Gemini25FlashPreview.is_top_tier());
    assert!(!ModelId::ClaudeHaiku45.is_top_tier());

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.is_top_tier(), entry.top_tier);
    }
}

#[test]
fn test_model_generation() {
    // Gemini generations
    assert_eq!(ModelId::Gemini25FlashPreview.generation(), "2.5");
    assert_eq!(ModelId::Gemini25Flash.generation(), "2.5");
    assert_eq!(ModelId::Gemini25FlashLite.generation(), "2.5");
    assert_eq!(ModelId::Gemini25Pro.generation(), "2.5");

    // OpenAI generations
    assert_eq!(ModelId::GPT52.generation(), "5.2");
    assert_eq!(ModelId::GPT5.generation(), "5");
    assert_eq!(ModelId::GPT5Codex.generation(), "5");
    assert_eq!(ModelId::GPT5Mini.generation(), "5");
    assert_eq!(ModelId::GPT5Nano.generation(), "5");
    assert_eq!(ModelId::CodexMiniLatest.generation(), "5");

    // Anthropic generations
    assert_eq!(ModelId::ClaudeSonnet45.generation(), "4.5");
    assert_eq!(ModelId::ClaudeHaiku45.generation(), "4.5");
    assert_eq!(ModelId::ClaudeSonnet4.generation(), "4");
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
    assert_eq!(ModelId::ZaiGlm47.generation(), "4.7");
    assert_eq!(ModelId::ZaiGlm46.generation(), "4.6");
    assert_eq!(ModelId::ZaiGlm45.generation(), "4.5");
    assert_eq!(ModelId::ZaiGlm45Air.generation(), "4.5");
    assert_eq!(ModelId::ZaiGlm45X.generation(), "4.5");
    assert_eq!(ModelId::ZaiGlm45Airx.generation(), "4.5");
    assert_eq!(ModelId::ZaiGlm45Flash.generation(), "4.5");
    assert_eq!(ModelId::ZaiGlm432b0414128k.generation(), "4-32B");
    assert_eq!(ModelId::OllamaGptOss20b.generation(), "oss");
    assert_eq!(ModelId::OllamaGptOss120bCloud.generation(), "oss-cloud");
    assert_eq!(ModelId::OllamaQwen317b.generation(), "oss");
    assert_eq!(
        ModelId::OllamaDeepseekV32Cloud.generation(),
        "deepseek-v3.2"
    );
    assert_eq!(ModelId::OllamaQwen3Coder480bCloud.generation(), "qwen3");
    assert_eq!(ModelId::OllamaGlm46Cloud.generation(), "glm-4.6");
    assert_eq!(
        ModelId::OllamaNemotron3Nano30bCloud.generation(),
        "nemotron-3"
    );
    assert_eq!(
        ModelId::LmStudioMetaLlama38BInstruct.generation(),
        "meta-llama-3"
    );
    assert_eq!(
        ModelId::LmStudioMetaLlama318BInstruct.generation(),
        "meta-llama-3.1"
    );
    assert_eq!(ModelId::LmStudioQwen257BInstruct.generation(), "qwen2.5");
    assert_eq!(ModelId::LmStudioGemma22BIt.generation(), "gemma-2");
    assert_eq!(ModelId::LmStudioGemma29BIt.generation(), "gemma-2");
    assert_eq!(ModelId::LmStudioPhi31Mini4kInstruct.generation(), "phi-3.1");

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.generation(), entry.generation);
    }
}

#[test]
fn test_models_for_provider() {
    let gemini_models = ModelId::models_for_provider(Provider::Gemini);
    assert!(gemini_models.contains(&ModelId::Gemini25Pro));
    assert!(!gemini_models.contains(&ModelId::GPT5));

    let openai_models = ModelId::models_for_provider(Provider::OpenAI);
    assert!(openai_models.contains(&ModelId::GPT52));
    assert!(openai_models.contains(&ModelId::GPT5));
    assert!(openai_models.contains(&ModelId::GPT5Codex));
    assert!(!openai_models.contains(&ModelId::Gemini25Pro));

    // Verify Anthropic models
    let anthropic_models = ModelId::models_for_provider(Provider::Anthropic);
    assert_eq!(anthropic_models.len(), 8); // Opus (4.5, 4.1, 4), Sonnet (4.5, 4, 3.7), Haiku (4.5, 3.5)
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet45));
    assert!(anthropic_models.contains(&ModelId::ClaudeHaiku45));
    assert!(anthropic_models.contains(&ModelId::ClaudeSonnet4));
    assert!(!anthropic_models.contains(&ModelId::GPT5));

    let deepseek_models = ModelId::models_for_provider(Provider::DeepSeek);
    assert!(deepseek_models.contains(&ModelId::DeepSeekChat));
    assert!(deepseek_models.contains(&ModelId::DeepSeekReasoner));

    let openrouter_models = ModelId::models_for_provider(Provider::OpenRouter);
    for entry in openrouter_generated::ENTRIES {
        let local_variant = entry.variant.as_str().parse::<ModelId>().unwrap();
        assert!(openrouter_models.contains(&local_variant));
    }

    let xai_models = ModelId::models_for_provider(Provider::XAI);
    assert!(xai_models.contains(&ModelId::XaiGrok4));
    assert!(xai_models.contains(&ModelId::XaiGrok4Mini));
    assert!(xai_models.contains(&ModelId::XaiGrok4Code));
    assert!(xai_models.contains(&ModelId::XaiGrok4CodeLatest));
    assert!(xai_models.contains(&ModelId::XaiGrok4Vision));

    let zai_models = ModelId::models_for_provider(Provider::ZAI);
    assert!(zai_models.contains(&ModelId::ZaiGlm46));
    assert!(zai_models.contains(&ModelId::ZaiGlm45));
    assert!(zai_models.contains(&ModelId::ZaiGlm45Air));
    assert!(zai_models.contains(&ModelId::ZaiGlm45X));
    assert!(zai_models.contains(&ModelId::ZaiGlm45Airx));
    assert!(zai_models.contains(&ModelId::ZaiGlm45Flash));
    assert!(zai_models.contains(&ModelId::ZaiGlm432b0414128k));

    let moonshot_models = ModelId::models_for_provider(Provider::Moonshot);
    assert!(moonshot_models.contains(&ModelId::MoonshotKimiK25));

    let ollama_models = ModelId::models_for_provider(Provider::Ollama);
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20b));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss120bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaQwen317b));
    assert!(ollama_models.contains(&ModelId::OllamaDeepseekV32Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaQwen3Next80bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaMistralLarge3675bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaKimiK2ThinkingCloud));
    assert!(ollama_models.contains(&ModelId::OllamaKimiK25Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaQwen3Coder480bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaGlm46Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaGemini3ProPreviewLatestCloud));
    assert!(ollama_models.contains(&ModelId::OllamaDevstral2123bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaMinimaxM2Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaNemotron3Nano30bCloud));
    assert_eq!(ollama_models.len(), 18);

    let lmstudio_models = ModelId::models_for_provider(Provider::LmStudio);
    assert!(lmstudio_models.contains(&ModelId::LmStudioMetaLlama38BInstruct));
    assert!(lmstudio_models.contains(&ModelId::LmStudioMetaLlama318BInstruct));
    assert!(lmstudio_models.contains(&ModelId::LmStudioQwen257BInstruct));
    assert!(lmstudio_models.contains(&ModelId::LmStudioGemma22BIt));
    assert!(lmstudio_models.contains(&ModelId::LmStudioGemma29BIt));
    assert!(lmstudio_models.contains(&ModelId::LmStudioPhi31Mini4kInstruct));
    assert_eq!(lmstudio_models.len(), 6);
}

#[test]
fn test_fallback_models() {
    let fallbacks = ModelId::fallback_models();
    assert!(!fallbacks.is_empty());
    assert!(fallbacks.contains(&ModelId::Gemini25FlashPreview));
    assert!(fallbacks.contains(&ModelId::Gemini25Pro));
    assert!(fallbacks.contains(&ModelId::GPT52));
    assert!(fallbacks.contains(&ModelId::GPT5));
    assert!(fallbacks.contains(&ModelId::GPT51));
    assert!(fallbacks.contains(&ModelId::OpenAIGptOss20b));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus45));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus41));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet45));
    assert!(fallbacks.contains(&ModelId::DeepSeekReasoner));
    assert!(fallbacks.contains(&ModelId::XaiGrok4));
    assert!(fallbacks.contains(&ModelId::ZaiGlm46));
    assert!(fallbacks.contains(&ModelId::OpenRouterGrokCodeFast1));
}
