use std::sync::Mutex;

use semver::Version;
use tracing::debug;

use super::Updater;
use super::cache;
use super::github;
use super::types::StartupUpdateNotice;

/// Maximum time (seconds) to wait for the GitHub API during preflight.
/// The preflight runs at startup and must never block the binary for longer
/// than this.  Users can still shorten the timeout via config.
const PREFLIGHT_TIMEOUT_CAP_SECS: u64 = 10;

static PREFLIGHT_NOTICE: Mutex<Option<StartupUpdateNotice>> = Mutex::new(None);

/// The release the preflight fetched, as `(version, release_notes)`.
///
/// Session setup decides the release-notes banner from the on-disk cache,
/// which the preflight only finishes writing a moment later (the check is
/// spawned so its network I/O never blocks startup). Publishing the fetch
/// here lets `cached_current_release_highlights` serve the running version's
/// notes on the first launch after an update instead of deferring them to a
/// second launch.
static PREFLIGHT_LATEST_RELEASE: Mutex<Option<(Version, String)>> = Mutex::new(None);

fn set_preflight_notice(notice: Option<StartupUpdateNotice>) {
    if let Ok(mut guard) = PREFLIGHT_NOTICE.lock() {
        *guard = notice;
    }
}

pub(crate) fn get_preflight_notice() -> Option<StartupUpdateNotice> {
    PREFLIGHT_NOTICE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The release the preflight fetched, as `(version, release_notes)`.
///
/// Like [`get_preflight_notice`], this is a same-process snapshot: `None`
/// means the spawned check has not completed (or did not run), never an error.
pub(crate) fn get_preflight_latest_release() -> Option<(Version, String)> {
    PREFLIGHT_LATEST_RELEASE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_preflight_latest_release(version: Version, release_notes: String) {
    if let Ok(mut guard) = PREFLIGHT_LATEST_RELEASE.lock() {
        *guard = Some((version, release_notes));
    }
}

/// Test seam: pretend the preflight fetched this release. Returns the
/// previous value for restoration, mirroring the cache-dir override.
#[cfg(test)]
pub(crate) fn set_preflight_latest_release_for_tests(release: Option<(Version, String)>) -> Option<(Version, String)> {
    let mut guard = PREFLIGHT_LATEST_RELEASE.lock().unwrap_or_else(|e| e.into_inner());
    std::mem::replace(&mut *guard, release)
}

/// Run a preflight update check at binary startup.
///
/// Always fetches from the GitHub API (force fetch) to ensure the user sees
/// the most recent version immediately.  Respects the user's
/// `check_interval_hours`, `release_channel`, and pinned-version config —
/// when updates are disabled or pinned the check is skipped.  Errors are
/// silently ignored so that a network failure never blocks startup.
pub(crate) async fn run_preflight_check() {
    let current_version_str = env!("CARGO_PKG_VERSION");

    let updater = match Updater::new(current_version_str) {
        Ok(u) => u,
        Err(err) => {
            debug!("Preflight update check: failed to create Updater: {err}");
            return;
        }
    };

    // Respect user config — skip if update checks are disabled or pinned.
    if updater.config().check_interval_hours == 0 || updater.is_pinned() {
        debug!("Preflight update check: skipped (disabled or pinned)");
        return;
    }

    // Always hit the GitHub API.  Cap the timeout so a version-info request
    // never blocks startup for longer than the cap.  Users can shorten it
    // further via config.
    let timeout = updater.config().download_timeout_secs.min(PREFLIGHT_TIMEOUT_CAP_SECS);
    let channel = &updater.config().channel;
    let latest = match github::fetch_latest_for_channel(timeout, channel).await {
        Ok(info) => info,
        Err(err) => {
            debug!("Preflight update check: GitHub fetch failed: {err}");
            // Mark the check as attempted so the background refresh task
            // does not re-fetch immediately.
            let _ = cache::record_failed_check();
            return;
        }
    };

    let current = updater.current_version();
    let latest_is_newer = latest.version > *current;

    // Keep the on-disk cache in sync so the session init code can also
    // read the result when it loads the cache later.
    if let Err(err) = cache::record_successful_check_with_notes(
        Some(&latest.version),
        latest_is_newer,
        Some(latest.release_notes.as_str()),
    ) {
        debug!("Preflight update check: failed to update cache: {err}");
    }
    // Publish for same-process readers that decided from the cache before
    // this write landed (first launch after an update).
    set_preflight_latest_release(latest.version.clone(), latest.release_notes.clone());

    if latest_is_newer {
        let cache_snapshot = cache::read_snapshot().unwrap_or_default();
        let was_dismissed = cache_snapshot.dismissed_version.as_ref().is_some_and(|d| d == &latest.version);

        if was_dismissed {
            debug!(
                "Preflight update check: latest version {latest} was already dismissed, suppressing notice",
                latest = latest.version
            );
            set_preflight_notice(None);
        } else {
            debug!("Preflight update check: new version {latest} > current {current}", latest = latest.version,);
            let notice = updater.notice_for_version(latest.version);
            set_preflight_notice(Some(notice));
        }
    } else {
        debug!("Preflight update check: versions match ({current}), no update available",);
        set_preflight_notice(None);
    }
}
