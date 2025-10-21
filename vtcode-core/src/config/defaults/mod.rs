pub use vtcode_config::defaults::{
    ConfigDefaultsProvider, ContextStoreDefaults, PerformanceDefaults, ScenarioDefaults,
    SyntaxHighlightingDefaults, WorkspacePathsDefaults, current_config_defaults,
    install_config_defaults_provider, reset_to_default_config_defaults, with_config_defaults,
};

pub mod provider {
    pub use vtcode_config::defaults::provider::*;
}

pub mod syntax_highlighting {
    pub use vtcode_config::defaults::syntax_highlighting::*;
}
