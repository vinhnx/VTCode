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
