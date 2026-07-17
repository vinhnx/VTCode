use std::sync::Mutex;

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

pub(crate) fn set_preflight_notice(notice: Option<StartupUpdateNotice>) {
    if let Ok(mut guard) = PREFLIGHT_NOTICE.lock() {
        *guard = notice;
    }
}

pub(crate) fn get_preflight_notice() -> Option<StartupUpdateNotice> {
    PREFLIGHT_NOTICE.lock().ok().and_then(|guard| guard.clone())
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

    if latest_is_newer {
        debug!(
            "Preflight update check: new version {latest} > current {current}",
            latest = latest.version,
        );
    } else {
        debug!("Preflight update check: versions match ({current}), no update available",);
    }

    // Keep the on-disk cache in sync so the session init code can also
    // read the result when it loads the cache later.
    if let Err(err) = cache::record_successful_check(Some(&latest.version), latest_is_newer) {
        debug!("Preflight update check: failed to update cache: {err}");
    }

    if latest_is_newer {
        let notice = updater.notice_for_version(latest.version);
        set_preflight_notice(Some(notice));
    } else {
        set_preflight_notice(None);
    }
}
