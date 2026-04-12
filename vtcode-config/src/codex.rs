use serde::{Deserialize, Serialize};

/// Editor URI scheme for clickable file citations.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum FileOpener {
    Cursor,
    #[default]
    None,
    Vscode,
    VscodeInsiders,
    Windsurf,
}

impl FileOpener {
    pub fn scheme(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Vscode => Some("vscode"),
            Self::Cursor => Some("cursor"),
            Self::Windsurf => Some("windsurf"),
            Self::VscodeInsiders => Some("vscode-insiders"),
        }
    }
}

/// Local history persistence mode.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HistoryPersistence {
    #[default]
    File,
    None,
}

/// Codex-compatible history persistence settings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HistoryConfig {
    #[serde(default)]
    pub persistence: HistoryPersistence,
    #[serde(default)]
    pub max_bytes: Option<usize>,
}

/// Built-in TUI notification delivery method.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TerminalNotificationMethod {
    #[default]
    Auto,
    Bel,
    Osc9,
}

/// Alternate-screen preference for the TUI.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TuiAlternateScreen {
    Always,
    Never,
}

/// TUI notification filters compatible with Codex config.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TuiNotificationEvent {
    AgentTurnComplete,
    ApprovalRequested,
}

/// Accept either `true`/`false` or an allowlist of event names.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum TuiNotificationsConfig {
    Enabled(bool),
    Events(Vec<TuiNotificationEvent>),
}

/// When to deliver desktop notifications relative to terminal focus.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationCondition {
    /// Only deliver when the terminal is unfocused (legacy behaviour).
    #[default]
    Unfocused,
    /// Always deliver regardless of focus state.
    Always,
}

/// Codex-compatible TUI settings.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TuiConfig {
    #[serde(default)]
    pub notifications: Option<TuiNotificationsConfig>,
    #[serde(default)]
    pub notification_method: Option<TerminalNotificationMethod>,
    /// When to deliver desktop notifications relative to terminal focus.
    /// Defaults to `unfocused` (only deliver when terminal is not focused).
    /// Set to `always` to deliver notifications even when the terminal is focused.
    #[serde(default)]
    pub notification_condition: Option<NotificationCondition>,
    #[serde(default)]
    pub animations: Option<bool>,
    #[serde(default)]
    pub alternate_screen: Option<TuiAlternateScreen>,
    #[serde(default)]
    pub show_tooltips: Option<bool>,
}
