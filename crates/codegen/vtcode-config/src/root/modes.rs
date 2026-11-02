use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ToolOutputMode {
    #[default]
    Compact,
    Full,
    /// Catch-all for unknown modes added by future versions.
    #[serde(other)]
    Unknown,
}

/// Controls how adjacent successful tool transition summaries are rendered.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ToolDisplayMode {
    /// Render each tool transition summary independently.
    Expanded,
    /// Group adjacent command summaries and render a bounded live view.
    #[default]
    Compact,
    /// Catch-all for unknown modes added by future versions.
    #[serde(other)]
    Unknown,
}

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ReasoningDisplayMode {
    Always,
    #[default]
    Toggle,
    Hidden,
    /// Catch-all for unknown modes added by future versions.
    #[serde(other)]
    Unknown,
}

/// Layout mode override for responsive UI
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeOverride {
    /// Auto-detect based on terminal size
    #[default]
    Auto,
    /// Force compact mode (no borders)
    Compact,
    /// Force standard mode (borders, no sidebar/footer)
    Standard,
    /// Force wide mode (sidebar + footer)
    Wide,
    /// Catch-all for unknown layouts added by future versions.
    #[serde(other)]
    Unknown,
}

/// UI display mode variants for quick presets
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiDisplayMode {
    /// Full UI with all features (sidebar, footer)
    Full,
    /// Minimal UI - no sidebar, no footer
    #[default]
    Minimal,
    /// Focused mode - transcript only, maximum content space
    Focused,
    /// Catch-all for unknown modes added by future versions.
    #[serde(other)]
    Unknown,
}

/// Notification delivery mode for terminal attention events.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryMode {
    /// Terminal-native alerts only (bell/OSC).
    Terminal,
    /// Terminal alerts with desktop notifications when supported.
    Hybrid,
    /// Desktop notifications only unless the terminal backend is selected explicitly.
    #[default]
    Desktop,
    /// Catch-all for unknown modes added by future versions.
    #[serde(other)]
    Unknown,
}

/// Preferred notification backend for desktop delivery.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NotificationBackend {
    /// Choose the best available backend for the current platform.
    #[default]
    Auto,
    /// Use macOS `osascript` notifications directly.
    Osascript,
    /// Use the `notify-rust` desktop notification backend.
    NotifyRust,
    /// Skip desktop notifications and use terminal attention only.
    Terminal,
    /// Catch-all for unknown backends added by future versions.
    #[serde(other)]
    Unknown,
}
