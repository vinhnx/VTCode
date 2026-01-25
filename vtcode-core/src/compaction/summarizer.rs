use crate::llm::provider::{LLMProvider, LLMRequest, Message};
use anyhow::{Context, Result};

/// Summarize the provided text using the configured provider.
pub async fn summarize_text(
    provider: &dyn LLMProvider,
    model: &str,
    prompt: &str,
) -> Result<String> {
    let request = LLMRequest {
        messages: vec![Message::user(prompt.to_string())],
        model: model.to_string(),
        ..Default::default()
    };

    let response = provider
        .generate(request)
        .await
        .context("Failed to generate summary")?;
    Ok(response.content.unwrap_or_default())
}
