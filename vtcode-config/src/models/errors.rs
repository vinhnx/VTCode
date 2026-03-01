use super::{ModelId, Provider};

/// Error type for model parsing failures
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelParseError {
    #[error("Invalid model identifier: '{}'. Supported models: {}", .0, ModelId::all_models().iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", "))]
    InvalidModel(String),

    #[error("Invalid provider: '{}'. Supported providers: {}", .0, Provider::all_providers().iter().map(|p| p.to_string()).collect::<Vec<_>>().join(", "))]
    InvalidProvider(String),
}
