//! LM Studio OSS provider integration for VT Code.
//!
//! This crate provides integration with LM Studio for running local OSS models.
//! It handles model management, downloading, and loading operations.

mod client;

pub use client::LMStudioClient;

use vtcode_config::VTCodeConfig;

/// Default OSS model to use when LM Studio is selected without an explicit model.
pub const DEFAULT_OSS_MODEL: &str = "openai/gpt-oss-20b";

/// Prepare the local OSS environment when LM Studio is selected.
///
/// - Ensures a local LM Studio server is reachable.
/// - Checks if the model exists locally and downloads it if missing.
/// - Loads the model in the background.
pub async fn ensure_oss_ready(config: &VTCodeConfig) -> std::io::Result<()> {
    // Determine which model to use
    let model = if config.agent.provider == "lmstudio" {
        config.agent.default_model.clone()
    } else {
        DEFAULT_OSS_MODEL.to_string()
    };

    // Verify local LM Studio is reachable.
    let lmstudio_client = LMStudioClient::try_from_provider(config).await?;

    match lmstudio_client.fetch_models().await {
        Ok(models) => {
            if !models.iter().any(|m| m == &model) {
                lmstudio_client.download_model(&model).await?;
            }
        }
        Err(err) => {
            // Not fatal; higher layers may still proceed and surface errors later.
            tracing::warn!("Failed to query local models from LM Studio: {}.", err);
        }
    }

    // Load the model in the background
    tokio::spawn({
        let client = lmstudio_client.clone();
        async move {
            if let Err(e) = client.load_model(&model).await {
                tracing::warn!("Failed to load model {}: {}", model, e);
            }
        }
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_oss_model() {
        assert_eq!(DEFAULT_OSS_MODEL, "openai/gpt-oss-20b");
    }
}
