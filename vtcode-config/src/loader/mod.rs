#[cfg(feature = "bootstrap")]
pub mod bootstrap;
pub mod layers;

mod builder;
mod config;
mod manager;
mod merge;
mod syntax_highlighting;

#[cfg(test)]
mod tests;

pub use builder::ConfigBuilder;
pub use config::{ProviderConfig, VTCodeConfig};
pub use manager::ConfigManager;
pub use merge::merge_toml_values;
pub use syntax_highlighting::SyntaxHighlightingConfig;
