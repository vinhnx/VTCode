use super::*;
use crate::constants::{model_helpers, models};
use std::str::FromStr;

#[test]
fn test_model_string_conversion() {
    // Gemini models
    assert_eq!(ModelId::Gemini35Flash.as_str(), models::GEMINI_3_5_FLASH);
    assert_eq!(
        ModelId::Gemini31ProPreview.as_str(),
        models::GEMINI_3_1_PRO_PREVIEW
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
    assert_eq!(
        ModelId::DeepSeekV4Flash.as_str(),
        models::deepseek::DEEPSEEK_V4_FLASH
    );
    // Hugging Face models
    assert_eq!(
        ModelId::HuggingFaceGlm51ZaiOrg.as_str(),
        models::huggingface::ZAI_GLM_5_1_ZAI_ORG
    );
    // OpenCode models
    assert_eq!(
        ModelId::OpenCodeZenGPT54.as_str(),
        models::opencode_zen::GPT_5_4
    );
    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.as_str(), entry.id);
    }
}

#[test]
fn test_model_from_string() {
    // Gemini models
    assert_eq!(
        models::GEMINI_3_5_FLASH.parse::<ModelId>().unwrap(),
        ModelId::Gemini35Flash
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
    assert_eq!(
        models::ollama::DEEPSEEK_V4_PRO_CLOUD
            .parse::<ModelId>()
            .unwrap(),
        ModelId::OllamaDeepseekV4ProCloud
    );
    // Hugging Face models
    assert_eq!(
        models::huggingface::ZAI_GLM_5_1_ZAI_ORG
            .parse::<ModelId>()
            .unwrap(),
        ModelId::HuggingFaceGlm51ZaiOrg
    );
    assert_eq!(
        "opencode/gpt-5.4".parse::<ModelId>().unwrap(),
        ModelId::OpenCodeZenGPT54
    );
    assert_eq!(
        "opencode-zen/claude-sonnet-4-6".parse::<ModelId>().unwrap(),
        ModelId::OpenCodeZenClaudeSonnet46
    );
    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.id.parse::<ModelId>().unwrap(), entry.variant);
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
    assert!("invalid-provider".parse::<Provider>().is_err());
}

#[test]
fn test_model_providers() {
    assert_eq!(ModelId::Gemini35Flash.provider(), Provider::Gemini);
    assert_eq!(ModelId::GPT55.provider(), Provider::OpenAI);
    assert_eq!(ModelId::ClaudeOpus48.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeSonnet46.provider(), Provider::Anthropic);
    assert_eq!(ModelId::ClaudeHaiku45.provider(), Provider::Anthropic);
    assert_eq!(ModelId::DeepSeekV4Pro.provider(), Provider::DeepSeek);
    assert_eq!(ModelId::ZaiGlm51.provider(), Provider::ZAI);
    assert_eq!(ModelId::OpenCodeZenGPT54.provider(), Provider::OpenCodeZen);
    assert_eq!(
        ModelId::OpenCodeGoMinimaxM27.provider(),
        Provider::OpenCodeGo
    );
    assert_eq!(ModelId::OllamaGptOss20b.provider(), Provider::Ollama);
    assert_eq!(ModelId::OllamaGptOss120bCloud.provider(), Provider::Ollama);
    assert_eq!(
        ModelId::OpenRouterAnthropicClaudeSonnet46.provider(),
        Provider::OpenRouter
    );

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.provider(), Provider::OpenRouter);
    }
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
        ModelId::default_orchestrator_for_provider(Provider::OpenRouter),
        ModelId::OpenRouterXiaomiMimoV25Pro
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::Ollama),
        ModelId::OllamaGptOss20b
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::ZAI),
        ModelId::ZaiGlm51
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenCodeZen),
        ModelId::OpenCodeZenGPT54
    );
    assert_eq!(
        ModelId::default_orchestrator_for_provider(Provider::OpenCodeGo),
        ModelId::OpenCodeGoMinimaxM27
    );
}

#[test]
fn test_model_defaults() {
    assert_eq!(ModelId::default(), ModelId::Gemini35Flash);
    assert_eq!(ModelId::default_model(), ModelId::Gemini35Flash);
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
    assert!(ModelId::OpenCodeZenGPT54.is_pro_variant());
    assert!(ModelId::DeepSeekV4Pro.is_pro_variant());
    assert!(ModelId::ZaiGlm51.is_pro_variant());
    assert!(!ModelId::Gemini35Flash.is_pro_variant());

    // Efficient variants
    assert!(ModelId::Gemini35Flash.is_efficient_variant());
    assert!(ModelId::GPT54Mini.is_efficient_variant());
    assert!(ModelId::ClaudeHaiku45.is_efficient_variant());
    assert!(ModelId::OpenCodeZenGPT54Mini.is_efficient_variant());
    assert!(ModelId::DeepSeekV4Flash.is_efficient_variant());
    assert!(!ModelId::GPT55.is_efficient_variant());

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.is_efficient_variant(), entry.efficient);
    }

    // Top tier models
    assert!(ModelId::GPT55.is_top_tier());
    assert!(ModelId::GPT53Codex.is_top_tier());
    assert!(ModelId::ClaudeOpus48.is_top_tier());
    assert!(ModelId::ClaudeSonnet46.is_top_tier());
    assert!(ModelId::DeepSeekV4Pro.is_top_tier());
    assert!(ModelId::ZaiGlm51.is_top_tier());
    assert!(ModelId::Gemini35Flash.is_top_tier());
    assert!(!ModelId::ClaudeHaiku45.is_top_tier());

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.is_top_tier(), entry.top_tier);
    }
}

#[test]
fn test_preferred_lightweight_variant() {
    assert_eq!(
        ModelId::GPT54.preferred_lightweight_variant(),
        Some(ModelId::GPT54Mini)
    );
    assert_eq!(
        ModelId::ClaudeSonnet46.preferred_lightweight_variant(),
        Some(ModelId::ClaudeHaiku45)
    );
    assert_eq!(
        ModelId::Gemini31ProPreview.preferred_lightweight_variant(),
        Some(ModelId::Gemini35Flash)
    );
    assert_eq!(
        ModelId::ZaiGlm51.preferred_lightweight_variant(),
        None
    );
    assert_eq!(
        ModelId::OpenCodeZenGPT54.preferred_lightweight_variant(),
        Some(ModelId::OpenCodeZenGPT54Mini)
    );
    assert_eq!(ModelId::GPT54Mini.preferred_lightweight_variant(), None);
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
    assert_eq!(ModelId::ZaiGlm51.generation(), "5.1");
    assert_eq!(ModelId::OpenCodeZenGPT54.generation(), "5.4");
    assert_eq!(ModelId::OpenCodeGoMinimaxM27.generation(), "m2.7");

    for entry in openrouter_generated::ENTRIES {
        assert_eq!(entry.variant.generation(), entry.generation);
    }
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

    let openrouter_models = ModelId::models_for_provider(Provider::OpenRouter);
    assert!(openrouter_models.contains(&ModelId::OpenRouterOpenAIGpt55));
    for entry in openrouter_generated::ENTRIES {
        assert!(openrouter_models.contains(&entry.variant));
    }

    let zai_models = ModelId::models_for_provider(Provider::ZAI);
    assert!(zai_models.contains(&ModelId::ZaiGlm51));

    let opencode_zen_models = ModelId::models_for_provider(Provider::OpenCodeZen);
    assert!(opencode_zen_models.contains(&ModelId::OpenCodeZenGPT54));
    assert!(opencode_zen_models.contains(&ModelId::OpenCodeZenClaudeSonnet46));

    let opencode_go_models = ModelId::models_for_provider(Provider::OpenCodeGo);
    assert!(opencode_go_models.contains(&ModelId::OpenCodeGoMinimaxM27));

    let ollama_models = ModelId::models_for_provider(Provider::Ollama);
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20b));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss20bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaGptOss120bCloud));
    assert!(ollama_models.contains(&ModelId::OllamaDeepseekV4FlashCloud));
    assert!(ollama_models.contains(&ModelId::OllamaDeepseekV4ProCloud));
    assert!(ollama_models.contains(&ModelId::OllamaMinimaxM27Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaMinimaxM3Cloud));
    assert!(ollama_models.contains(&ModelId::OllamaGlm51Cloud));

    let hf_models = ModelId::models_for_provider(Provider::HuggingFace);
    assert!(hf_models.contains(&ModelId::HuggingFaceGlm51ZaiOrg));
}

#[test]
fn test_ollama_cloud_models() {
    use crate::constants::models;

    // Test parsing of new Ollama cloud models
    let model_pairs = vec![
        (
            ModelId::OllamaGptOss20bCloud,
            models::ollama::GPT_OSS_20B_CLOUD,
        ),
        (
            ModelId::OllamaGptOss120bCloud,
            models::ollama::GPT_OSS_120B_CLOUD,
        ),
        (
            ModelId::OllamaDeepseekV4FlashCloud,
            models::ollama::DEEPSEEK_V4_FLASH_CLOUD,
        ),
        (
            ModelId::OllamaDeepseekV4ProCloud,
            models::ollama::DEEPSEEK_V4_PRO_CLOUD,
        ),
        (
            ModelId::OllamaMinimaxM27Cloud,
            models::ollama::MINIMAX_M27_CLOUD,
        ),
        (
            ModelId::OllamaMinimaxM3Cloud,
            models::ollama::MINIMAX_M3_CLOUD,
        ),
        (ModelId::OllamaGlm51Cloud, models::ollama::GLM_5_1_CLOUD),
    ];

    for (model_id, expected_str) in model_pairs {
        assert_eq!(model_id.as_str(), expected_str);
        assert_eq!(ModelId::from_str(expected_str).unwrap(), model_id);
        assert_eq!(model_id.provider(), Provider::Ollama);

        // Verify display names are not empty
        assert!(!model_id.display_name().is_empty());

        // Verify descriptions are not empty
        assert!(!model_id.description().is_empty());

        // Verify generation is not empty
        assert!(!model_id.generation().is_empty());
    }
}

#[test]
fn test_fallback_models() {
    let fallbacks = ModelId::fallback_models();
    assert!(!fallbacks.is_empty());
    assert!(fallbacks.contains(&ModelId::Gemini35Flash));
    assert!(fallbacks.contains(&ModelId::GPT54));
    assert!(fallbacks.contains(&ModelId::GPT55));
    assert!(fallbacks.contains(&ModelId::ClaudeOpus48));
    assert!(fallbacks.contains(&ModelId::ClaudeSonnet46));
    assert!(fallbacks.contains(&ModelId::DeepSeekV4Pro));
    assert!(fallbacks.contains(&ModelId::ZaiGlm51));
}

#[test]
fn test_provider_local_helpers() {
    assert!(Provider::Ollama.is_local());
    assert!(Provider::LmStudio.is_local());
    assert!(Provider::LlamaCpp.is_local());
    assert!(!Provider::OpenAI.is_local());
    assert!(Provider::Ollama.is_dynamic());
    assert!(Provider::LmStudio.is_dynamic());
    assert!(Provider::LlamaCpp.is_dynamic());
    assert!(Provider::Copilot.is_dynamic());
    assert!(!Provider::OpenAI.is_dynamic());
    assert!(Provider::Ollama.local_install_instructions().is_some());
    assert!(Provider::LmStudio.local_install_instructions().is_some());
    assert!(Provider::LlamaCpp.local_install_instructions().is_some());
    assert!(Provider::OpenAI.local_install_instructions().is_none());
}

#[test]
fn test_core_capability_helpers() {
    assert_eq!(
        ModelId::DeepSeekV4Pro.non_reasoning_variant(),
        Some(ModelId::DeepSeekV4Flash)
    );
    assert!(ModelId::GPT54.supports_shell_tool());
    assert!(ModelId::GPT53Codex.supports_shell_tool());
    assert!(!ModelId::GPT53Codex.supports_apply_patch_tool());
}

#[test]
fn test_generated_model_capability_lookup() {
    let gpt54_catalog = model_catalog_entry("openai", "gpt-5.4").expect("gpt-5.4 metadata");
    assert_eq!(gpt54_catalog.context_window, 1_050_000);
    assert!(gpt54_catalog.tool_call);
    assert_eq!(gpt54_catalog.input_modalities, &["text", "image"]);

    let gemini_catalog = model_catalog_entry("google", "gemini-3-flash-preview")
        .expect("gemini-3-flash-preview metadata");
    assert_eq!(gemini_catalog.provider, "gemini");
    assert_eq!(gemini_catalog.context_window, 1_048_576);

    let openai_models = supported_models_for_provider("openai").expect("openai models");
    assert!(openai_models.contains(&models::GPT_5_4));
    assert!(catalog_provider_keys().contains(&"openai"));
    let openrouter_models = supported_models_for_provider("openrouter").expect("openrouter models");
    assert!(openrouter_models.contains(&"openai/gpt-5.5"));
    let opencode_zen_models =
        supported_models_for_provider("opencode-zen").expect("opencode zen models");
    assert!(opencode_zen_models.contains(&models::opencode_zen::GPT_5_4));
    let opencode_go_models =
        supported_models_for_provider("opencode-go").expect("opencode go models");
    assert!(opencode_go_models.contains(&models::opencode_go::MINIMAX_M2_7));

    assert_eq!(ModelId::GPT54.input_modalities(), &["text", "image"]);
    assert_eq!(
        ModelId::OpenCodeZenGPT54.input_modalities(),
        &["text", "image"]
    );
    assert_eq!(ModelId::GPT53Codex.input_modalities(), &["text", "image"]);
    assert_eq!(
        ModelId::Gemini31ProPreview.input_modalities(),
        &["text", "image", "video", "audio", "pdf"]
    );
    assert_eq!(ModelId::ClaudeOpus48.input_modalities(), &["text", "image"]);
    assert_eq!(
        ModelId::OpenRouterOpenAIGpt5Chat.input_modalities(),
        &["file", "image", "text"]
    );

    assert!(ModelId::GPT54.supports_tool_calls());
    assert!(ModelId::OpenCodeZenGPT54.supports_tool_calls());
    assert!(ModelId::GPT53Codex.supports_tool_calls());
    assert!(ModelId::Gemini31ProPreview.supports_tool_calls());
    assert!(!ModelId::OpenRouterOpenAIGpt5Chat.supports_tool_calls());
}

#[test]
fn test_gpt_5_5_dated_alias_round_trips_to_gpt55_capabilities() {
    assert_eq!(
        ModelId::from_str(models::openai::GPT_5_5_DATED).unwrap(),
        ModelId::GPT55
    );
    assert!(
        models::openai::RESPONSES_API_MODELS.contains(&models::openai::GPT_5_5_DATED),
        "dated GPT-5.5 alias should stay on the Responses API path"
    );
    assert!(
        Provider::OpenAI.supports_reasoning_effort(models::openai::GPT_5_5_DATED),
        "dated GPT-5.5 alias should inherit reasoning-effort support"
    );
    assert!(
        Provider::OpenAI.supports_service_tier(models::openai::GPT_5_5_DATED),
        "dated GPT-5.5 alias should inherit service-tier support"
    );
}

#[test]
fn test_model_helpers_include_curated_opencode_models() {
    let zen_models = model_helpers::supported_for("opencode-zen").expect("opencode zen helpers");
    assert!(zen_models.contains(&models::opencode_zen::GPT_5_4));
    assert!(zen_models.contains(&models::opencode_zen::CLAUDE_SONNET_4_6));
    assert!(!zen_models.contains(&models::opencode_zen::GPT_5_2));
    assert_eq!(
        model_helpers::default_for("opencode-zen"),
        Some(models::opencode_zen::DEFAULT_MODEL)
    );

    let go_models = model_helpers::supported_for("opencode-go").expect("opencode go helpers");
    assert!(go_models.contains(&models::opencode_go::MINIMAX_M2_7));
    assert!(go_models.contains(&models::opencode_go::GLM_5_1));
    assert_eq!(
        model_helpers::default_for("opencode-go"),
        Some(models::opencode_go::DEFAULT_MODEL)
    );
}

#[test]
fn test_enum_variants_match_all_models_collection() {
    let src = include_str!("model_id.rs");
    let mut in_enum = false;
    let mut enum_variants = std::collections::BTreeSet::new();

    for raw in src.lines() {
        let line = raw.trim();
        if line.starts_with("pub enum ModelId") {
            in_enum = true;
            continue;
        }
        if in_enum && line.starts_with('}') {
            break;
        }
        if !in_enum
            || line.is_empty()
            || line.starts_with("//")
            || line.starts_with("///")
            || line.starts_with("#[")
        {
            continue;
        }
        if let Some((name, _)) = line.split_once(',') {
            enum_variants.insert(name.trim().to_string());
        }
    }

    let all_models_vec = ModelId::all_models();
    let all_models: std::collections::BTreeSet<String> = all_models_vec
        .iter()
        .map(|model| format!("{model:?}"))
        .collect();

    assert_eq!(
        all_models_vec.len(),
        all_models.len(),
        "all_models should not contain duplicate variants"
    );
    assert_eq!(all_models, enum_variants);
}

#[test]
fn test_all_models_have_non_empty_metadata_and_parse() {
    for model in ModelId::all_models() {
        assert!(!model.as_str().is_empty());
        assert!(!model.display_name().is_empty());
        assert!(!model.description().is_empty());
        assert!(!model.generation().is_empty());
        let parsed = match model {
            ModelId::OpenCodeZenGPT54 => ModelId::from_str("opencode/gpt-5.4"),
            ModelId::OpenCodeZenGPT54Mini => ModelId::from_str("opencode/gpt-5.4-mini"),
            ModelId::OpenCodeZenClaudeSonnet46 => ModelId::from_str("opencode/claude-sonnet-4-6"),
            ModelId::OpenCodeZenGlm51 => ModelId::from_str("opencode/glm-5.1"),
            ModelId::OpenCodeGoGlm51 => ModelId::from_str("opencode-go/glm-5.1"),
            ModelId::OpenCodeGoMinimaxM27 => ModelId::from_str("opencode-go/minimax-m2.7"),
            // Qwen third-party variants share model strings with their native providers;
            // `deepseek-v4-flash`, `deepseek-v4-pro`, `glm-5.1` resolve to native variants.
            ModelId::QwenDeepSeekV4Flash | ModelId::QwenDeepSeekV4Pro | ModelId::QwenGlm51 => {
                continue;
            }
            // LlamaCpp/Ollama GPT-OSS-20B share the same model string as OpenAI's variant;
            // `gpt-oss-20b` resolves to OpenAIGptOss20b first.
            ModelId::LlamaCppGptOss20b | ModelId::OllamaGptOss20b => continue,
            _ => ModelId::from_str(model.as_str()),
        };
        assert_eq!(parsed.unwrap(), model);
    }
}
