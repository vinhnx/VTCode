#[cfg(feature = "bootstrap")]
mod bootstrap;
pub mod layers;
pub mod watch;

mod builder;
mod config;
mod fingerprint;
mod manager;
mod merge;
pub(crate) mod session_override;
mod syntax_highlighting;
mod toml_patch;

#[cfg(test)]
mod tests;

pub use builder::ConfigBuilder;
pub use config::{FeaturesConfig, ProviderConfig, VTCodeConfig, WorkspaceConfig};
pub use fingerprint::{fingerprint_str, fingerprint_toml_value};
pub use manager::{ConfigManager, ConfigPhaseTiming};
pub use merge::{merge_toml_values, merge_toml_values_with_origins};
pub use session_override::{explicit_config_path, set_explicit_config_path};
pub use syntax_highlighting::SyntaxHighlightingConfig;
pub use toml_patch::{merge_toml_value_preserving_comments, read_config_toml_for_edit};
pub use watch::{ConfigWatcher, SimpleConfigWatcher};
