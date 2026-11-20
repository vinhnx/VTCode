//! Notification preferences for terminal and desktop alerts
//! (`[notifications]` under `ui.notifications`). Split from `mod.rs` to
//! respect the 1000-line ceiling; the surface and defaults are unchanged.

use serde::{Deserialize, Serialize};

use super::env_bool_var;
use super::modes::{NotificationBackend, NotificationDeliveryMode};

/// Notification preferences for terminal and desktop alerts.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UiNotificationsConfig {
    /// Master toggle for all runtime notifications.
    #[serde(default = "default_notifications_enabled")]
    pub enabled: bool,

    /// Notification transport strategy.
    #[serde(default)]
    pub delivery_mode: NotificationDeliveryMode,

    /// Preferred backend for desktop notification delivery.
    #[serde(default)]
    pub backend: NotificationBackend,

    /// Suppress notifications while terminal focus is active.
    #[serde(default = "default_notifications_suppress_when_focused")]
    pub suppress_when_focused: bool,

    /// Notify when a shell/command execution fails.
    /// If omitted, falls back to `tool_failure` for backward compatibility.
    #[serde(default)]
    pub command_failure: Option<bool>,

    /// Notify when a tool call fails.
    #[serde(default = "default_notifications_tool_failure")]
    pub tool_failure: bool,

    /// Notify on runtime/system errors.
    #[serde(default = "default_notifications_error")]
    pub error: bool,

    /// Legacy master toggle for completion notifications.
    /// New installs should prefer `completion_success` and `completion_failure`.
    #[serde(default = "default_notifications_completion")]
    pub completion: bool,

    /// Notify when a turn/session completes successfully.
    /// If omitted, falls back to `completion`.
    #[serde(default)]
    pub completion_success: Option<bool>,

    /// Notify when a turn/session is partial, failed, or cancelled.
    /// If omitted, falls back to `completion`.
    #[serde(default)]
    pub completion_failure: Option<bool>,

    /// Notify when human input/approval is required.
    #[serde(default = "default_notifications_hitl")]
    pub hitl: bool,

    /// Notify when policy approval is required.
    /// If omitted, falls back to `hitl` for backward compatibility.
    #[serde(default)]
    pub policy_approval: Option<bool>,

    /// Notify on generic request events.
    /// If omitted, falls back to `hitl` for backward compatibility.
    #[serde(default)]
    pub request: Option<bool>,

    /// Notify on successful tool calls.
    #[serde(default = "default_notifications_tool_success")]
    pub tool_success: bool,

    /// Suppression window for repeated identical notifications.
    #[serde(default = "default_notifications_repeat_window_seconds")]
    pub repeat_window_seconds: u64,

    /// Maximum identical notifications allowed within the suppression window.
    #[serde(default = "default_notifications_max_identical_in_window")]
    pub max_identical_in_window: u32,
}

impl Default for UiNotificationsConfig {
    fn default() -> Self {
        Self {
            enabled: default_notifications_enabled(),
            delivery_mode: NotificationDeliveryMode::default(),
            backend: NotificationBackend::default(),
            suppress_when_focused: default_notifications_suppress_when_focused(),
            command_failure: Some(default_notifications_command_failure()),
            tool_failure: default_notifications_tool_failure(),
            error: default_notifications_error(),
            completion: default_notifications_completion(),
            completion_success: Some(default_notifications_completion_success()),
            completion_failure: Some(default_notifications_completion_failure()),
            hitl: default_notifications_hitl(),
            policy_approval: Some(default_notifications_policy_approval()),
            request: Some(default_notifications_request()),
            tool_success: default_notifications_tool_success(),
            repeat_window_seconds: default_notifications_repeat_window_seconds(),
            max_identical_in_window: default_notifications_max_identical_in_window(),
        }
    }
}

fn default_notifications_enabled() -> bool {
    true
}

fn default_notifications_suppress_when_focused() -> bool {
    true
}

fn default_notifications_command_failure() -> bool {
    false
}

fn default_notifications_tool_failure() -> bool {
    false
}

fn default_notifications_error() -> bool {
    true
}

fn default_notifications_completion() -> bool {
    true
}

fn default_notifications_completion_success() -> bool {
    false
}

fn default_notifications_completion_failure() -> bool {
    true
}

fn default_notifications_hitl() -> bool {
    true
}

fn default_notifications_policy_approval() -> bool {
    true
}

fn default_notifications_request() -> bool {
    false
}

fn default_notifications_tool_success() -> bool {
    false
}

fn default_notifications_repeat_window_seconds() -> u64 {
    30
}

fn default_notifications_max_identical_in_window() -> u32 {
    1
}
