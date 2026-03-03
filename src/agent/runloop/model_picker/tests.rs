use super::*;
use anyhow::Result;
use std::fs;
use tempfile::tempdir;
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
    assert!(has_model(options, ModelId::ClaudeOpus41));
    assert!(has_model(options, ModelId::ClaudeSonnet46));
    assert!(has_model(options, ModelId::ClaudeSonnet45));
    assert!(has_model(options, ModelId::ClaudeHaiku45));
    assert!(has_model(options, ModelId::ClaudeSonnet4));

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
    assert!(has_model(options, ModelId::OllamaMistralLarge3675bCloud));
    assert!(has_model(options, ModelId::OllamaQwen3Coder480bCloud));
    assert!(has_model(options, ModelId::OllamaGlm5Cloud));
    assert!(has_model(options, ModelId::OllamaMinimaxM25Cloud));
    assert!(has_model(options, ModelId::OllamaGemini3FlashPreviewCloud));
    assert!(has_model(options, ModelId::MinimaxM2));
}

#[test]
fn model_picker_lists_new_gemini_models() {
    let options = MODEL_OPTIONS.as_slice();
    assert!(has_model(options, ModelId::Gemini31ProPreview));
}

fn base_picker_state(current_provider: &str, current_model: &str) -> ModelPickerState {
    ModelPickerState {
        options: MODEL_OPTIONS.as_slice(),
        step: PickerStep::AwaitModel,
        inline_enabled: true,
        current_reasoning: ReasoningEffortLevel::Medium,
        current_provider: current_provider.to_string(),
        current_model: current_model.to_string(),
        selection: None,
        selected_reasoning: None,
        pending_api_key: None,
        workspace: None,
        dynamic_models: DynamicModelRegistry::default(),
        plain_mode_active: false,
    }
}

#[test]
fn preferred_model_selection_matches_current_static_model() {
    let model_id = ModelId::ClaudeOpus41.as_str();
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
