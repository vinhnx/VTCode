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
    /// Parsed `### Highlights` bullets for `latest_version`, taken from the
    /// cached release notes. Empty when the cache has no notes (or no
    /// Highlights section); clients render a link-only state then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_highlights: Vec<String>,
    /// Release channel the service follows (stable/beta/nightly).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// Whether the configured check interval has elapsed since the last check.
    #[serde(default)]
    pub check_due: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_status_snapshot_round_trips_and_older_payloads_decode() {
        let snapshot = UpdateStatusSnapshot {
            current_version: "0.180.5".into(),
            latest_version: Some("0.181.0".into()),
            update_available: true,
            release_url: Some("https://github.com/vinhnx/vtcode/releases/tag/v0.181.0".into()),
            release_highlights: vec!["Faster startup".into(), "New model picker".into()],
            channel: Some("stable".into()),
            check_due: false,
        };
        let decoded: UpdateStatusSnapshot = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
        assert_eq!(decoded, snapshot);

        // Asymmetric fixtures: an up-to-date service omits every optional field,
        // and a legacy payload carries only the running version.
        let quiet = UpdateStatusSnapshot {
            current_version: "0.181.0".into(),
            latest_version: None,
            update_available: false,
            release_url: None,
            release_highlights: Vec::new(),
            channel: None,
            check_due: true,
        };
        let decoded_quiet: UpdateStatusSnapshot =
            serde_json::from_value(serde_json::to_value(&quiet).unwrap()).unwrap();
        assert_eq!(decoded_quiet, quiet);

        let legacy: UpdateStatusSnapshot =
            serde_json::from_value(serde_json::json!({ "currentVersion": "0.179.0" })).unwrap();
        assert_eq!(legacy.current_version, "0.179.0");
        assert!(legacy.latest_version.is_none());
        assert!(!legacy.update_available);
        assert!(legacy.release_url.is_none());
        assert!(legacy.release_highlights.is_empty());
        assert!(legacy.channel.is_none());
        assert!(!legacy.check_due);
    }
}
