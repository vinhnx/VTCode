use vtcode_core::utils::dot_config::{DotConfig, load_user_config, save_user_config};

use super::notice::DesktopSuggestNotice;

pub(crate) async fn take_desktop_suggest_notice(suggest_install: bool) -> Option<DesktopSuggestNotice> {
    if !suggest_install || !crate::updater::desktop_supported() {
        return None;
    }
    // An explicit VTCODE_DESKTOP_APP override counts as installed: a developer
    // pointing at a custom bundle already knows about the desktop app.
    let installed = crate::updater::existing_install_dir().is_some();
    let (mut config, persist_notice) = load_notice_config().await;
    let now = unix_now_secs();
    let notice = DesktopSuggestNotice::from_snapshot(installed, config.desktop_notice.last_shown_unix_secs, now)?;

    if persist_notice {
        config.desktop_notice.last_shown_unix_secs = Some(now);
        let _ = save_user_config(&config).await;
    }

    Some(notice)
}

async fn load_notice_config() -> (DotConfig, bool) {
    match load_user_config().await {
        Ok(config) => (config, true),
        Err(_) => (DotConfig::default(), false),
    }
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::take_desktop_suggest_notice;

    /// Muted preference short-circuits before any I/O: hermetic on every platform.
    #[tokio::test]
    async fn muted_preference_shows_nothing() {
        assert!(take_desktop_suggest_notice(false).await.is_none());
    }

    /// Off macOS the app can never install, so there is nothing to suggest.
    #[cfg(not(target_os = "macos"))]
    #[tokio::test]
    async fn unsupported_platform_shows_nothing() {
        assert!(take_desktop_suggest_notice(true).await.is_none());
    }
}
