use super::*;
use anyhow::Result;
use std::fs;
use tempfile::tempdir;
use vtcode_config::OpenAIServiceTier;
use vtcode_core::config::models::ModelId;

fn has_model(options: &[ModelOption], model: ModelId) -> bool {
    let id = model.as_str();
    let provider = model.provider();
    options
        .iter()
        .any(|option| option.id == id && option.provider == provider)
}

#[test]
fn model_picker_lists_new_anthropic_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::ClaudeOpus46));
    assert!(has_model(options, ModelId::ClaudeSonnet46));
    assert!(has_model(options, ModelId::ClaudeHaiku45));

    // OpenRouter variants
    assert!(has_model(
        options,
        ModelId::OpenRouterAnthropicClaudeSonnet46
    ));
    assert!(has_model(
        options,
        ModelId::OpenRouterAnthropicClaudeSonnet45
    ));
}

#[test]
fn model_picker_lists_new_nvidia_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(
        options,
        ModelId::OpenRouterNvidiaNemotron3Super120bA12bFree
    ));
}

#[test]
fn model_picker_lists_new_zai_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::ZaiGlm5));
}

#[test]
fn model_picker_lists_new_ollama_cloud_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::OllamaGptOss20b));
    assert!(has_model(options, ModelId::OllamaGptOss120bCloud));
    assert!(has_model(options, ModelId::OllamaQwen3CoderNext));
    assert!(has_model(options, ModelId::OllamaDeepseekV32Cloud));
    assert!(has_model(options, ModelId::OllamaQwen3Next80bCloud));
    assert!(has_model(options, ModelId::OllamaGlm5Cloud));
    assert!(has_model(options, ModelId::OllamaMinimaxM25Cloud));
    assert!(has_model(options, ModelId::OllamaGemini3FlashPreviewCloud));
    assert!(has_model(options, ModelId::MinimaxM25));
}

#[test]
fn model_picker_lists_new_gemini_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::Gemini31ProPreview));
}

#[test]
fn model_search_value_includes_provider_model_aliases() {
    let extra_terms = vec![
        "reasoning".to_string(),
        "tools".to_string(),
        "image".to_string(),
    ];
    let value = super::rendering::model_search_value(
        Provider::OpenAI,
        "GPT-5.2",
        "gpt-5.2",
        Some("Latest frontier model"),
        &extra_terms,
    )
    .to_ascii_lowercase();

    assert!(value.contains("openai gpt-5.2"));
    assert!(value.contains("openai/gpt-5.2"));
    assert!(value.contains("reasoning"));
    assert!(value.contains("tools"));
    assert!(value.contains("image"));
}

#[test]
fn static_model_subtitle_formats_current_capabilities() {
    let option = MODEL_OPTIONS
        .iter()
        .find(|option| option.model == ModelId::GPT54)
        .expect("gpt-5.4 option should exist");

    let subtitle = super::rendering::static_model_subtitle(option, "openai", "gpt-5.4");

    assert_eq!(
        subtitle,
        "gpt-5.4 • Current • Context: 1M • Reasoning • Tools • Input: text, image"
    );
}

#[test]
fn static_model_search_terms_include_modalities_and_tool_state() {
    let terms =
        super::rendering::static_model_search_terms(ModelId::OpenRouterOpenAIGpt5Chat, false);

    assert!(terms.iter().any(|term| term == "no tools"));
    assert!(terms.iter().any(|term| term == "no-tools"));
    assert!(terms.iter().any(|term| term == "tool_call disabled"));
    assert!(terms.iter().any(|term| term == "modalities"));
    assert!(terms.iter().any(|term| term == "file"));
    assert!(terms.iter().any(|term| term == "image"));
    assert!(terms.iter().any(|term| term == "text"));
}

#[test]
fn dynamic_model_subtitle_stays_conservative_for_unknown_local_models() {
    let subtitle = super::rendering::dynamic_model_subtitle(
        Provider::Ollama,
        "custom-local-model",
        false,
        "ollama",
        "custom-local-model",
    );

    assert_eq!(subtitle, "custom-local-model • Current • Local");
}

#[test]
fn current_model_line_shows_effective_anthropic_context_window() {
    let line = super::rendering::current_model_line("anthropic", "claude-sonnet-4-6");
    assert_eq!(line, "Current: anthropic / claude-sonnet-4-6 • Context: 1M");
}

fn base_picker_state(current_provider: &str, current_model: &str) -> ModelPickerState {
    ModelPickerState {
        options: MODEL_OPTIONS.as_slice(),
        step: PickerStep::AwaitModel,
        inline_enabled: true,
        current_reasoning: ReasoningEffortLevel::Medium,
        current_service_tier: None,
        current_provider: current_provider.to_string(),
        current_model: current_model.to_string(),
        selection: None,
        selected_reasoning: None,
        selected_service_tier: None,
        pending_api_key: None,
        workspace: None,
        dynamic_models: DynamicModelRegistry::default(),
        plain_mode_active: false,
    }
}

#[test]
fn preferred_model_selection_matches_current_static_model() {
    let model_id = ModelId::ClaudeOpus46.as_str();
    let picker = base_picker_state("anthropic", model_id);

    let selection = picker.preferred_model_selection();
    let Some(InlineListSelection::Model(index)) = selection else {
        panic!("expected static model selection, got {selection:?}");
    };

    let option = picker
        .options
        .get(index)
        .expect("selected index should be valid");
    assert_eq!(option.provider, Provider::Anthropic);
    assert_eq!(option.id, model_id);
}

#[test]
fn preferred_model_selection_returns_none_for_unknown_model() {
    let picker = base_picker_state("anthropic", "does-not-exist");
    assert_eq!(picker.preferred_model_selection(), None);
}

#[test]
fn read_workspace_env_returns_value_when_present() -> Result<()> {
    let dir = tempdir()?;
    let env_path = dir.path().join(".env");
    fs::write(&env_path, "OPENAI_API_KEY=sk-test\n")?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, Some("sk-test".to_string()));
    Ok(())
}

#[test]
fn read_workspace_env_returns_none_when_missing_file() -> Result<()> {
    let dir = tempdir()?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn read_workspace_env_returns_none_when_key_absent() -> Result<()> {
    let dir = tempdir()?;
    let env_path = dir.path().join(".env");
    fs::write(&env_path, "OTHER_KEY=value\n")?;
    let value = super::read_workspace_env(dir.path(), "OPENAI_API_KEY")?;
    assert_eq!(value, None);
    Ok(())
}

#[test]
fn selection_marks_openai_service_tier_support_for_supported_models() {
    let detail = selection::selection_from_option(
        MODEL_OPTIONS
            .iter()
            .find(|option| option.id == "gpt-5.2")
            .expect("gpt-5.2 option should exist"),
    );

    assert!(detail.service_tier_supported);
}

#[test]
fn selection_omits_openai_service_tier_support_for_gpt_oss() {
    let detail = selection::selection_from_option(
        MODEL_OPTIONS
            .iter()
            .find(|option| option.id == "gpt-oss-20b")
            .expect("gpt-oss option should exist"),
    );

    assert!(!detail.service_tier_supported);
}

#[test]
fn build_result_uses_selected_service_tier() {
    let mut picker = base_picker_state("openai", "gpt-5.2");
    picker.selection = Some(selection::SelectionDetail {
        provider_key: "openai".to_string(),
        provider_label: "OpenAI".to_string(),
        provider_enum: Some(Provider::OpenAI),
        model_id: "gpt-5.2".to_string(),
        model_display: "GPT-5.2".to_string(),
        known_model: true,
        reasoning_supported: true,
        reasoning_optional: false,
        reasoning_off_model: None,
        service_tier_supported: true,
        requires_api_key: false,
        env_key: "OPENAI_API_KEY".to_string(),
    });
    picker.selected_reasoning = Some(ReasoningEffortLevel::Low);
    picker.selected_service_tier = Some(true);

    let result = picker.build_result().expect("result should build");

    assert_eq!(result.service_tier, Some(OpenAIServiceTier::Priority));
    assert!(result.service_tier_changed);
}
