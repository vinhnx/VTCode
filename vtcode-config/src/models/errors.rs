use std::fmt;

use super::{ModelId, Provider};

/// Error type for model parsing failures
#[derive(Debug, Clone, PartialEq)]
pub enum ModelParseError {
    InvalidModel(String),
    InvalidProvider(String),
}

impl fmt::Display for ModelParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelParseError::InvalidModel(model) => {
                write!(
                    f,
                    "Invalid model identifier: '{}'. Supported models: {}",
                    model,
                    ModelId::all_models()
                        .iter()
                        .map(|m| m.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            ModelParseError::InvalidProvider(provider) => {
                write!(
                    f,
                    "Invalid provider: '{}'. Supported providers: {}",
                    provider,
                    Provider::all_providers()
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ModelParseError {}
