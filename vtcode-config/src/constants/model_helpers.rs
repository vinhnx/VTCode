use super::models;

/// Get supported models for a provider
pub fn supported_for(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "google" | "gemini" => Some(models::google::SUPPORTED_MODELS),
        "openai" | "codex" => Some(models::openai::SUPPORTED_MODELS),
        "anthropic" => Some(models::anthropic::SUPPORTED_MODELS),
        "copilot" => Some(models::copilot::SUPPORTED_MODELS),
        "mimo" => Some(models::mimo::SUPPORTED_MODELS),
        "minimax" => Some(models::minimax::SUPPORTED_MODELS),
        "deepseek" => Some(models::deepseek::SUPPORTED_MODELS),
        #[cfg(not(docsrs))]
        "openrouter" => Some(models::openrouter::SUPPORTED_MODELS),
        #[cfg(docsrs)]
        "openrouter" => Some(&[]),
        "moonshot" => Some(models::moonshot::SUPPORTED_MODELS),
        "zai" => Some(models::zai::SUPPORTED_MODELS),
        "opencode-zen" => Some(models::opencode_zen::CONFIGURED_MODELS),
        "opencode-go" => Some(models::opencode_go::CONFIGURED_MODELS),
        "ollama" => Some(models::ollama::SUPPORTED_MODELS),
        "qwen" => Some(models::qwen::SUPPORTED_MODELS),
        "poolside" => Some(models::poolside::SUPPORTED_MODELS),
        _ => None,
    }
}

/// Get default model for a provider
pub fn default_for(provider: &str) -> Option<&'static str> {
    match provider {
        "google" | "gemini" => Some(models::google::DEFAULT_MODEL),
        "openai" | "codex" => Some(models::openai::DEFAULT_MODEL),
        "anthropic" => Some(models::anthropic::DEFAULT_MODEL),
        "copilot" => Some(models::copilot::DEFAULT_MODEL),
        "mimo" => Some(models::mimo::DEFAULT_MODEL),
        "minimax" => Some(models::minimax::DEFAULT_MODEL),
        "deepseek" => Some(models::deepseek::DEFAULT_MODEL),
        #[cfg(not(docsrs))]
        "openrouter" => Some(models::openrouter::DEFAULT_MODEL),
        #[cfg(docsrs)]
        "openrouter" => Some("openrouter/auto"), // Fallback for docs.rs build
        "moonshot" => Some(models::moonshot::DEFAULT_MODEL),
        "zai" => Some(models::zai::DEFAULT_MODEL),
        "opencode-zen" => Some(models::opencode_zen::DEFAULT_MODEL),
        "opencode-go" => Some(models::opencode_go::DEFAULT_MODEL),
        "ollama" => Some(models::ollama::DEFAULT_MODEL),
        "qwen" => Some(models::qwen::DEFAULT_MODEL),
        "poolside" => Some(models::poolside::DEFAULT_MODEL),
        _ => None,
    }
}

/// Validate if a model is supported by a provider
pub fn is_valid(provider: &str, model: &str) -> bool {
    supported_for(provider)
        .map(|list| list.contains(&model))
        .unwrap_or(false)
}
