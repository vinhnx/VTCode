//! Cached updater-status DTOs.

use serde::{Deserialize, Serialize};

/// Cached updater status for the desktop client. The service owns update
/// policy (channel, pin, check interval); this snapshot never triggers
/// network I/O, it only reports the last cached check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusSnapshot {
    /// Version of the running service binary.
    pub current_version: String,
    /// Latest known release version, when a check has reported one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Whether `latest_version` is newer than `current_version` and not dismissed.
    #[serde(default)]
    pub update_available: bool,
    /// HTTPS release page for `latest_version`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    /// Release channel the service follows (stable/beta/nightly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Whether the configured check interval has elapsed since the last check.
    #[serde(default)]
    pub check_due: bool,
}
