use super::ModelId;

/// Error type for model identifier parsing failures
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ModelParseError {
    #[error("Invalid model identifier: '{}'. Supported models: {}", .0, ModelId::all_models().iter().map(|m| m.as_str()).collect::<Vec<_>>().join(", "))]
    InvalidModel(String),
}
