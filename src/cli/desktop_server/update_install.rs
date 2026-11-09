//! In-app update install for the desktop client.
//!
//! `update/status` is deliberately read-only, so a client cannot observe a
//! release newer than the service's cached check by clicking "Check for
//! updates". This module adds the two missing halves: a live network check
//! (`update/check`) and an install (`update/install`) that streams
//! `update/installProgress` notifications while it runs.
//!
//! The install replaces the whole app bundle — the same thing
//! `vtcode desktop --install` does — because the running app spawns its
//! service from inside that bundle, so replacing only the helper would leave
//! the GUI binary stale and report a version the user never received.

use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::ServiceContext;
use super::catalog::sanitize_public_error;
use super::send_notification;
use crate::updater::{
    DesktopInstallOutcome, UpdateProgress, Updater, default_install_dir, desktop_supported, existing_install_dir,
    install_desktop_app_with_progress,
};
use vtcode_desktop_protocol::{UpdateInstallState, method};

/// Install state shared with `update/status`, so every status response — and
/// every progress notification — reports the same phase. A `Mutex` (not an
/// async lock) because every access is a non-awaiting copy.
static INSTALL_STATE: Mutex<UpdateInstallState> = Mutex::new(UpdateInstallState::Idle);

/// The install state to report to clients.
pub(super) fn install_state() -> UpdateInstallState {
    INSTALL_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

fn set_install_state(state: UpdateInstallState) {
    *INSTALL_STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = state;
}

/// Test seam: drive the shared install state without running an install, so
/// dispatch tests can cover the concurrent-install refusal without touching
/// the network or the app bundle.
#[cfg(test)]
pub(super) fn set_test_install_state(state: UpdateInstallState) {
    set_install_state(state);
}

/// Whether an install is currently in flight. Callers use this to refuse a
/// second concurrent install rather than racing two bundle replacements.
pub(super) fn install_in_flight() -> bool {
    install_state().is_in_flight()
}

/// Re-run the update check against GitHub and return the refreshed status.
///
/// Unlike [`super::update_status_snapshot`], this performs network I/O, so a
/// release published after the service's cached check becomes visible without
/// waiting for the configured interval. Errors propagate: a failed check is
/// reported to the user instead of silently re-serving a stale cache.
pub(super) async fn check_for_updates() -> Result<()> {
    // Upper bound for the whole check: the service request loop only drains
    // notifications between requests, so an unbounded check would stall every
    // other desktop surface for the duration of a GitHub API outage (the
    // shared retry budget spans minutes by design). The About row already
    // renders a Checking state, so a timeout surfaces as a failed check there.
    const UPDATE_CHECK_TIMEOUT_SECS: u64 = 30;
    check_for_updates_with_timeout(std::time::Duration::from_secs(UPDATE_CHECK_TIMEOUT_SECS)).await
}

/// The check itself, with the deadline as a parameter so tests can pin the
/// timeout path without waiting out the production bound.
async fn check_for_updates_with_timeout(timeout: std::time::Duration) -> Result<()> {
    let check = async {
        let updater = Updater::new(env!("CARGO_PKG_VERSION")).context("failed to initialize updater")?;
        updater.refresh_startup_update_cache().await?;
        Ok(())
    };
    match tokio::time::timeout(timeout, check).await {
        Ok(result) => result,
        Err(_) => anyhow::bail!("update check timed out"),
    }
}

/// Start installing the newest release, streaming progress to `context`.
///
/// Returns the starting state immediately; the install itself runs on a
/// background task. This matters because the service request loop only drains
/// the notification queue between requests: awaiting a multi-minute download
/// inside the handler would buffer every progress tick unsent (and wedge the
/// final publish on a full queue). Idempotent while one runs: a second
/// request is refused rather than starting a competing install.
pub(super) async fn start_install(context: &ServiceContext) -> Result<UpdateInstallState> {
    if !desktop_supported() {
        anyhow::bail!("the VT Code desktop app requires macOS");
    }
    if install_in_flight() {
        anyhow::bail!("an update install is already in progress");
    }

    let starting = UpdateInstallState::Downloading { downloaded: 0, total: None };
    set_install_state(starting.clone());

    let notifications = context.notifications.clone();
    publish_progress(&notifications, starting.clone()).await;
    tokio::spawn(async move {
        let outcome = run_install(&notifications).await;
        match outcome {
            Ok(DesktopInstallOutcome::Installed { version, needs_gatekeeper_approval, .. }) => {
                if needs_gatekeeper_approval {
                    info!(target: "vtcode.desktop", "installed bundle needs one-time Gatekeeper approval");
                }
                set_install_state(UpdateInstallState::Completed { version, needs_gatekeeper_approval });
                info!(target: "vtcode.desktop", "in-app update install completed");
            }
            Ok(DesktopInstallOutcome::UpToDate { version, .. }) => {
                set_install_state(UpdateInstallState::Completed { version, needs_gatekeeper_approval: false });
                info!(target: "vtcode.desktop", "in-app update install completed");
            }
            Err(error) => {
                // Sanitized like every other service error (secrets stripped,
                // length-capped), never the raw chain: the message renders in
                // the About row, and paths add nothing there.
                let message = sanitize_public_error(&format!("{error:#}"));
                warn!(target: "vtcode.desktop", %error, "in-app update install failed");
                set_install_state(UpdateInstallState::Failed { message });
            }
        }
        publish_progress(&notifications, install_state()).await;
    });

    Ok(starting)
}

/// Drive the bundle install, mapping updater phases onto the wire state.
async fn run_install(
    notifications: &tokio::sync::mpsc::Sender<vtcode_desktop_protocol::Notification>,
) -> Result<DesktopInstallOutcome> {
    let updater = Updater::new(env!("CARGO_PKG_VERSION")).context("failed to initialize updater")?;
    // Updates install in place so a system-wide install is never duplicated
    // per-user; a fresh install lands in ~/Applications.
    let install_dir = existing_install_dir()
        .or_else(|| default_install_dir().ok())
        .context("could not resolve a desktop app install directory")?;

    let mut last_download = UpdateInstallState::Downloading { downloaded: 0, total: None };
    let outcome = install_desktop_app_with_progress(&updater, &install_dir, |progress| {
        let state = match progress {
            UpdateProgress::Downloading { downloaded, total } => UpdateInstallState::Downloading { downloaded, total },
            UpdateProgress::VerifyingChecksum => UpdateInstallState::Verifying,
            UpdateProgress::Extracting => UpdateInstallState::Extracting,
            UpdateProgress::ReplacingBinary => UpdateInstallState::Installing,
        };
        // `download_asset` already throttles its callback (percent change or
        // 100 ms), so every tick here is worth publishing. Dedupe exact
        // repeats only; a slow client drops ticks at the bounded queue
        // instead of slowing the install (see `publish_progress`).
        if state != last_download {
            last_download = state.clone();
            set_install_state(state.clone());
            let notifications = notifications.clone();
            tokio::spawn(async move {
                publish_progress(&notifications, state).await;
            });
        }
    })
    .await?;

    Ok(outcome)
}

async fn publish_progress(
    notifications: &tokio::sync::mpsc::Sender<vtcode_desktop_protocol::Notification>,
    state: UpdateInstallState,
) {
    // A full queue means the client is not consuming: drop the tick rather
    // than blocking the install, mirroring the runtime notification paths.
    if let Err(error) = send_notification(
        notifications,
        method::UPDATE_INSTALL_PROGRESS,
        serde_json::to_value(state).unwrap_or_default(),
    )
    .await
    {
        warn!(target: "vtcode.desktop", %error, "failed to publish update install progress");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn a_fresh_process_reports_no_install_in_flight() {
        // The static starts `Idle`; a poison must not wedge the state either.
        set_install_state(UpdateInstallState::Idle);
        assert!(!install_in_flight());
        assert_eq!(install_state(), UpdateInstallState::Idle);
    }

    #[test]
    #[serial]
    fn completing_an_install_clears_the_in_flight_flag() {
        set_install_state(UpdateInstallState::Installing);
        assert!(install_in_flight(), "installing counts as in flight");

        set_install_state(UpdateInstallState::Completed {
            version: "0.181.8".into(),
            needs_gatekeeper_approval: true,
        });
        assert!(!install_in_flight(), "a finished install is not in flight");
        assert_eq!(
            install_state(),
            UpdateInstallState::Completed {
                version: "0.181.8".into(),
                needs_gatekeeper_approval: true
            }
        );

        set_install_state(UpdateInstallState::Idle);
    }

    #[test]
    #[serial]
    fn a_failed_install_also_clears_the_in_flight_flag() {
        set_install_state(UpdateInstallState::Failed { message: "network unreachable".into() });
        assert!(!install_in_flight(), "a failed install must not block a retry");

        set_install_state(UpdateInstallState::Idle);
    }

    /// The timeout bound is what keeps a live check from stalling the whole
    /// service request loop during a GitHub API outage. A millisecond deadline
    /// can never survive the network round trip the check needs, so this pins
    /// the timeout path without waiting out the production bound.
    #[tokio::test]
    async fn an_unanswered_check_fails_as_a_timeout() {
        let cache_dir = tempfile::tempdir().expect("cache");
        let previous = crate::updater::set_test_update_cache_dir(Some(cache_dir.path().to_path_buf()));

        let error = check_for_updates_with_timeout(std::time::Duration::from_millis(1))
            .await
            .expect_err("a 1 ms check must time out");
        assert!(error.to_string().contains("timed out"), "{error:#}");

        crate::updater::set_test_update_cache_dir(previous);
    }
}
