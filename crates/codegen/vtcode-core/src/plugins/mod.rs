//! Plugin system for VT Code
//!
//! This module provides a comprehensive plugin system that supports:
//! - Commands (slash commands)
//! - Agents (specialized profiles)
//! - Skills (model-invoked capabilities)
//! - Hooks (event handlers)
//! - MCP servers (Model Context Protocol)

pub mod caching;
pub mod components;
pub mod directory;
pub mod loader;
pub mod manager;
pub mod manifest;
pub mod runtime;
pub mod validation;

pub use caching::*;
pub use components::*;
pub use directory::*;
pub use loader::*;
pub use manager::*;
pub use manifest::*;
pub use runtime::*;
pub use validation::*;

crate::id_newtype::define_id_newtype! {
    /// Identifier for a plugin.
    pub struct PluginId
}

/// Type alias for plugin names
pub type PluginName = String;

/// Plugin loading result
pub type PluginResult<T> = Result<T, PluginError>;

pub(crate) fn validate_plugin_component(value: &str) -> PluginResult<()> {
    let mut components = std::path::Path::new(value).components();
    if value.trim().is_empty()
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(PluginError::LoadingError(format!("Plugin name must be one normal path component: {value}")));
    }
    Ok(())
}

/// Plugin error types
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Plugin manifest validation failed: {0}")]
    ManifestValidationError(String),

    #[error("Plugin not found: {0}")]
    NotFound(PluginId),

    #[error("Plugin already exists: {0}")]
    AlreadyExists(PluginId),

    #[error("Plugin loading failed: {0}")]
    LoadingError(String),

    #[error("Plugin execution failed: {0}")]
    ExecutionError(String),

    #[error("Plugin permission denied: {0}")]
    PermissionDenied(String),

    #[error("Plugin configuration error: {0}")]
    ConfigurationError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("TOML serialization error: {0}")]
    TomlError(#[from] toml::de::Error),
}
