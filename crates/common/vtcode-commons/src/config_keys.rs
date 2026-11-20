//! Canonical dotted config keys shared by the settings editor, config loader,
//! desktop settings bridge, and misconfiguration diagnostics.
//!
//! These keys cross crate boundaries: a typo in one surface silently misses a
//! setting on another (the value still loads, it just never matches). Keep the
//! spelling in one struct and reference it from every producer/consumer.

/// Canonical dotted paths for first-party settings.
pub struct ConfigKey;

impl ConfigKey {
    pub const DEFAULT_MODEL: &'static str = "agent.default_model";
    pub const PROVIDER: &'static str = "agent.provider";
    pub const THEME: &'static str = "agent.theme";
    pub const SMALL_MODEL: &'static str = "agent.small_model";
    pub const SMALL_MODEL_NAME: &'static str = "agent.small_model.model";
    pub const EDITOR: &'static str = "tools.editor";
    pub const CODEX_APP_SERVER: &'static str = "agent.codex_app_server";
    pub const CODEX_APP_SERVER_COMMAND: &'static str = "agent.codex_app_server.command";
    pub const CODEX_APP_SERVER_ARGS: &'static str = "agent.codex_app_server.args";
}
