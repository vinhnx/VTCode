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
    /// Newest release the service's update cache knows about that is *newer*
    /// than `current_version`; clients read its notes. Never the running or an
    /// older release, so a reader row cannot label a back-version "latest".
    /// Only [`Self::update_available`] means an update is actually offered (a
    /// dismissed update keeps its notes readable while this stays `true`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_version: Option<String>,
    /// Whether `latest_version` is newer than `current_version` and not dismissed.
    #[serde(default)]
    pub update_available: bool,
    /// HTTPS release page for `latest_version`, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_url: Option<String>,
    /// Parsed `### Highlights` bullets for `latest_version`, taken from the
    /// cached release notes. Empty when the cache holds notes without a
    /// Highlights section; clients render a link-only state then.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release_highlights: Vec<String>,
    /// Parsed `### Highlights` bullets for the running `current_version`,
    /// for the post-update "what's new" moment. Empty unless the cache holds
    /// notes describing exactly the running version.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_release_highlights: Vec<String>,
    /// HTTPS release page for the running version, sent alongside
    /// `current_release_highlights` so the "what's new" viewer can link out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_release_url: Option<String>,
    /// Whether the running version's notes were already announced by any VT
    /// Code surface. The marker lives in the shared update cache, so notes
    /// shown by the CLI are not repeated by the desktop app and vice versa.
    #[serde(default)]
    pub current_release_seen: bool,
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
            current_release_highlights: vec!["Older highlight".into()],
            current_release_url: Some("https://github.com/vinhnx/vtcode/releases/tag/v0.180.5".into()),
            current_release_seen: true,
            channel: Some("stable".into()),
            check_due: false,
        };
        let decoded: UpdateStatusSnapshot = serde_json::from_value(serde_json::to_value(&snapshot).unwrap()).unwrap();
        assert_eq!(decoded, snapshot);

        // Asymmetric fixtures: the post-update shape carries current-version
        // notes with no update available, and a legacy payload carries only
        // the running version.
        let quiet = UpdateStatusSnapshot {
            current_version: "0.181.0".into(),
            latest_version: None,
            update_available: false,
            release_url: None,
            release_highlights: Vec::new(),
            current_release_highlights: vec!["Freshly installed".into()],
            current_release_url: Some("https://github.com/vinhnx/vtcode/releases/tag/v0.181.0".into()),
            current_release_seen: false,
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
        assert!(legacy.current_release_highlights.is_empty());
        assert!(legacy.current_release_url.is_none());
        assert!(!legacy.current_release_seen);
        assert!(legacy.channel.is_none());
        assert!(!legacy.check_due);
    }
}
