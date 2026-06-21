use std::sync::Mutex;

use tracing::debug;

use super::Updater;
use super::cache;
use super::github;
use super::types::StartupUpdateNotice;

static PREFLIGHT_NOTICE: Mutex<Option<StartupUpdateNotice>> = Mutex::new(None);

pub(crate) fn set_preflight_notice(notice: Option<StartupUpdateNotice>) {
    *PREFLIGHT_NOTICE.lock().unwrap() = notice;
}

pub(crate) fn get_preflight_notice() -> Option<StartupUpdateNotice> {
    PREFLIGHT_NOTICE.lock().unwrap().clone()
}

/// Run a preflight update check at binary startup.
///
/// Always fetches from the GitHub API (force fetch) to ensure the user sees
/// the most recent version immediately.  Respects the user's
/// `check_interval_hours` and pinned-version config — when updates are
/// disabled or pinned the check is skipped.  Errors are silently ignored so
/// that a network failure never blocks startup.
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

    // Always hit the GitHub API.  Cap at 10 seconds — a version-info
    // request should never block startup for longer than the original
    // hardcoded timeout.  Users can still shorten it via config.
    let timeout = updater.config().download_timeout_secs.min(10);
    let latest = match github::fetch_latest_release_info(timeout).await {
        Ok(info) => info,
        Err(err) => {
            debug!("Preflight update check: GitHub fetch failed: {err}");
            // Mark the check as attempted so the background refresh task
            // does not re-fetch immediately.
            let _ = cache::record_failed_check();
            return;
        }
    };

    let latest_is_newer = latest.version > *updater.current_version();

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
