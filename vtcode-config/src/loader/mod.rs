#[cfg(feature = "bootstrap")]
pub mod bootstrap;
pub mod layers;

mod builder;
mod config;
mod fingerprint;
mod manager;
mod merge;
mod syntax_highlighting;

#[cfg(test)]
mod tests;

pub use builder::ConfigBuilder;
pub use config::{ProviderConfig, VTCodeConfig};
pub use fingerprint::{fingerprint_str, fingerprint_toml_value};
pub use manager::ConfigManager;
pub use merge::{merge_toml_values, merge_toml_values_with_origins};
pub use syntax_highlighting::SyntaxHighlightingConfig;
