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
    /// State of an in-app update install. The service owns install policy and
    /// the download itself; clients render this and never derive progress from
    /// anything else, so a re-render cannot disagree with the install.
    #[serde(default)]
    pub install: UpdateInstallState,
}

/// Phase of an in-app update install, carried by [`UpdateStatusSnapshot`] and
/// the `update/installProgress` notification.
///
/// Internally tagged so the wire form names its own phase
/// (`{"phase":"downloading","downloaded":10,"total":100}`), which keeps a
/// client from mislabeling a phase it did not expect.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "phase", rename_all_fields = "camelCase")]
pub enum UpdateInstallState {
    /// No install has run, or the last one was superseded by a newer check.
    #[default]
    Idle,
    /// Downloading the release archive.
    Downloading {
        /// Bytes written so far.
        downloaded: u64,
        /// Total archive size when the server sent `Content-Length`; `None`
        /// otherwise, which renders as a byte count rather than a percentage.
        total: Option<u64>,
    },
    /// Checksum metadata resolved; hashing the downloaded archive.
    Verifying,
    /// Unpacking the archive and staging the new bundle.
    Extracting,
    /// Replacing the installed bundle on disk.
    Installing,
    /// The install finished.
    Completed {
        /// Version now on disk.
        version: String,
        /// The replaced bundle needs a one-time Gatekeeper approval before
        /// macOS launches it (ad hoc-signed builds): the client must say so
        /// instead of only prompting a restart. `#[serde(default)]` keeps
        /// older services decodable.
        #[serde(default)]
        needs_gatekeeper_approval: bool,
    },
    /// The install failed.
    Failed {
        /// Sanitized reason; never a raw path or URL.
        message: String,
    },
}

impl UpdateInstallState {
    /// Whether an install is currently in flight, so a client can disable the
    /// install action instead of starting a second concurrent one.
    pub fn is_in_flight(&self) -> bool {
        matches!(self, Self::Downloading { .. } | Self::Verifying | Self::Extracting | Self::Installing)
    }
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
            install: UpdateInstallState::Downloading { downloaded: 1024, total: Some(2048) },
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
            install: UpdateInstallState::Idle,
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
        assert_eq!(legacy.install, UpdateInstallState::Idle);
    }

    /// The install state is internally tagged, so the wire form names its own
    /// phase and a client cannot mislabel one it did not expect.
    #[test]
    fn install_state_round_trips_with_a_named_phase() {
        let states = [
            UpdateInstallState::Idle,
            UpdateInstallState::Downloading { downloaded: 1846, total: Some(18462109) },
            // Asymmetric input: a missing Content-Length must survive as null
            // rather than collapsing to a zero total that renders 0%.
            UpdateInstallState::Downloading { downloaded: 4096, total: None },
            UpdateInstallState::Verifying,
            UpdateInstallState::Extracting,
            UpdateInstallState::Installing,
            UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: false,
            },
            UpdateInstallState::Failed { message: "network unreachable".into() },
        ];

        for state in states {
            let encoded = serde_json::to_value(&state).unwrap();
            let decoded: UpdateInstallState = serde_json::from_value(encoded.clone()).unwrap();
            assert_eq!(decoded, state);
            assert!(encoded.get("phase").is_some_and(|phase| phase.is_string()), "{encoded}");
        }

        // An ad hoc-signed bundle needs its approval flag to survive the wire.
        let gated = UpdateInstallState::Completed {
            version: "0.181.8".into(),
            needs_gatekeeper_approval: true,
        };
        let decoded: UpdateInstallState = serde_json::from_value(serde_json::to_value(&gated).unwrap()).unwrap();
        assert_eq!(decoded, gated);

        // Older services send no approval flag at all; that must read as a
        // build that needs no extra step, not as a decode failure.
        let legacy: UpdateInstallState =
            serde_json::from_value(serde_json::json!({ "phase": "completed", "version": "0.181.8" })).unwrap();
        assert_eq!(
            legacy,
            UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: false
            }
        );

        assert_eq!(
            serde_json::to_value(UpdateInstallState::Downloading { downloaded: 7, total: None }).unwrap(),
            serde_json::json!({ "phase": "downloading", "downloaded": 7, "total": null })
        );
    }

    /// An older service sends no install field at all; a newer client must
    /// read that as "nothing running" rather than failing to decode.
    #[test]
    fn a_payload_without_install_decodes_as_idle() {
        let decoded: UpdateStatusSnapshot =
            serde_json::from_value(serde_json::json!({ "currentVersion": "0.181.7" })).unwrap();

        assert_eq!(decoded.install, UpdateInstallState::Idle);
    }

    #[test]
    fn only_active_phases_report_an_install_in_flight() {
        assert!(!UpdateInstallState::Idle.is_in_flight());
        assert!(
            !UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: true
            }
            .is_in_flight()
        );
        assert!(!UpdateInstallState::Failed { message: "boom".into() }.is_in_flight());
        assert!(UpdateInstallState::Downloading { downloaded: 1, total: Some(2) }.is_in_flight());
        assert!(UpdateInstallState::Verifying.is_in_flight());
        assert!(UpdateInstallState::Extracting.is_in_flight());
        assert!(UpdateInstallState::Installing.is_in_flight());
    }
}
