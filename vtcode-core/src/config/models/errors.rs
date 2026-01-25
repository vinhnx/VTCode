//! Error types for model parsing.

use std::fmt;

use super::{ModelId, Provider};
use itertools::Itertools;

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
                    ModelId::all_models().iter().map(|m| m.as_str()).join(", ")
                )
            }
            ModelParseError::InvalidProvider(provider) => {
                write!(
                    f,
                    "Invalid provider: '{}'. Supported providers: {}",
                    provider,
                    Provider::all_providers()
                        .iter()
                        .map(|p| p.to_owned())
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for ModelParseError {}
