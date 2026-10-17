//! Codex app-server sidecar configuration.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentCodexAppServerConfig {
    /// Executable used to launch the official Codex app-server sidecar.
    #[serde(default = "default_codex_app_server_command")]
    pub command: String,
    /// Arguments passed before VT Code appends `--listen stdio://`.
    #[serde(default = "default_codex_app_server_args")]
    pub args: Vec<String>,
    /// Maximum startup handshake time when launching the sidecar.
    #[serde(default = "default_codex_app_server_startup_timeout_secs")]
    pub startup_timeout_secs: u64,
    /// Enable experimental Codex app-server sidecar features.
    #[serde(default = "default_codex_app_server_experimental_features")]
    pub experimental_features: bool,
}

impl Default for AgentCodexAppServerConfig {
    fn default() -> Self {
        Self {
            command: default_codex_app_server_command(),
            args: default_codex_app_server_args(),
            startup_timeout_secs: default_codex_app_server_startup_timeout_secs(),
            experimental_features: default_codex_app_server_experimental_features(),
        }
    }
}

#[inline]
fn default_codex_app_server_command() -> String {
    "codex".to_string()
}

#[inline]
fn default_codex_app_server_args() -> Vec<String> {
    vec!["app-server".to_string()]
}

#[inline]
const fn default_codex_app_server_startup_timeout_secs() -> u64 {
    10
}

#[inline]
const fn default_codex_app_server_experimental_features() -> bool {
    false
}
