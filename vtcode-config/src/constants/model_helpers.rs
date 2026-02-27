use super::models;

/// Get supported models for a provider
pub fn supported_for(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "google" | "gemini" => Some(models::google::SUPPORTED_MODELS),
        "openai" => Some(models::openai::SUPPORTED_MODELS),
        "anthropic" => Some(models::anthropic::SUPPORTED_MODELS),
        "minimax" => Some(models::minimax::SUPPORTED_MODELS),
        "deepseek" => Some(models::deepseek::SUPPORTED_MODELS),
        #[cfg(not(docsrs))]
        "openrouter" => Some(models::openrouter::SUPPORTED_MODELS),
        #[cfg(docsrs)]
        "openrouter" => Some(&[]),
        "moonshot" => Some(models::moonshot::SUPPORTED_MODELS),
        "zai" => Some(models::zai::SUPPORTED_MODELS),
        "ollama" => Some(models::ollama::SUPPORTED_MODELS),
        _ => None,
    }
}

/// Get default model for a provider
pub fn default_for(provider: &str) -> Option<&'static str> {
    match provider {
        "google" | "gemini" => Some(models::google::DEFAULT_MODEL),
        "openai" => Some(models::openai::DEFAULT_MODEL),
        "anthropic" => Some(models::anthropic::DEFAULT_MODEL),
        "minimax" => Some(models::minimax::DEFAULT_MODEL),
        "deepseek" => Some(models::deepseek::DEFAULT_MODEL),
        #[cfg(not(docsrs))]
        "openrouter" => Some(models::openrouter::DEFAULT_MODEL),
        #[cfg(docsrs)]
        "openrouter" => Some("openrouter/auto"), // Fallback for docs.rs build
        "moonshot" => None,
        "zai" => Some(models::zai::DEFAULT_MODEL),
        "ollama" => Some(models::ollama::DEFAULT_MODEL),
        _ => None,
    }
}

/// Validate if a model is supported by a provider
pub fn is_valid(provider: &str, model: &str) -> bool {
    supported_for(provider)
        .map(|list| list.contains(&model))
        .unwrap_or(false)
}
