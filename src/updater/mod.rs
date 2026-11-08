mod archive;
mod cache;
mod desktop;
mod download;
mod github;
mod install_source;
mod interactive;
mod preflight;
mod progress;
mod release_notes;
mod types;

use anyhow::{Context, Result, bail};
use semver::Version;
use tracing::{debug, info};
use vtcode_config::update::UpdateConfig;

pub(crate) use desktop::{
    DesktopInstallOutcome, default_install_dir, desktop_macos_major, desktop_supported, existing_install_dir,
    install_desktop_app, installed_desktop_version,
};
pub(crate) use install_source::InstallSource;
pub(crate) use interactive::{
    InlineUpdateOutcome, append_notice_highlight, display_release_notes, display_update_notice, execute_inline_update,
    run_inline_update_prompt,
};
pub(crate) use preflight::{get_preflight_notice, run_preflight_check};
pub(crate) use progress::UpdateProgress;
pub(crate) use release_notes::parse_highlights;
pub(crate) use types::{
    InstallOutcome, StartupUpdateCheck, StartupUpdateNotice, UpdateExecutionStrategy, UpdateGuidance, UpdateInfo,
    VersionInfo,
};

/// Test seam: redirect the update cache away from the real user cache.
#[cfg(test)]
pub(crate) use cache::set_cache_dir_override_for_tests as set_test_update_cache_dir;

/// Auto-updater for VT Code binary from GitHub Releases
pub(crate) struct Updater {
    current_version: Version,
    config: UpdateConfig,
}

impl Updater {
    pub(crate) fn new(current_version_str: &str) -> Result<Self> {
        let current_version = Version::parse(current_version_str)
            .with_context(|| format!("Invalid version format: {current_version_str}"))?;

        let config = UpdateConfig::load().unwrap_or_else(|e| {
            debug!("Failed to load update config, using defaults: {}", e);
            UpdateConfig::default()
        });

        Ok(Self { current_version, config })
    }

    pub(crate) fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub(crate) fn config(&self) -> &UpdateConfig {
        &self.config
    }

    pub(crate) fn release_url(version: &Version) -> String {
        github::release_url(version)
    }

    pub(crate) async fn check_for_updates(&mut self) -> Result<Option<UpdateInfo>> {
        debug!("Checking for VT Code updates (channel: {})...", self.config.channel);

        if let Some(pinned_version) = self.config.pinned_version() {
            if self.config.should_auto_unpin() {
                // Auto-unpin: fetch the latest release and, if it is newer
                // than the pinned version, clear the pin so the update proceeds.
                debug!("Version pinned to {} with auto-unpin enabled, checking for newer release", pinned_version);
                let latest =
                    github::fetch_latest_release(self, self.config.download_timeout_secs, &self.config.channel).await?;
                if let Some(info) = latest.as_ref()
                    && info.version > *pinned_version
                {
                    info!("Auto-unpinning: newer version {} available (pinned: {})", info.version, pinned_version);
                    self.config.clear_pin();
                    let _ = self.config.save();
                    return Ok(latest);
                }
                debug!("Pinned version is still latest, keeping pin");
                return Ok(None);
            }
            debug!("Version pinned to {}, skipping update check", pinned_version);
            return Ok(None);
        }

        let latest =
            github::fetch_latest_release(self, self.config.download_timeout_secs, &self.config.channel).await?;

        if latest.as_ref().is_some_and(|info| info.version > self.current_version) {
            if let Some(latest) = latest.as_ref() {
                info!("New version available: {} (current: {})", latest.version, self.current_version);
            }
        } else {
            debug!("Already on latest version");
        }

        Ok(latest)
    }

    pub(crate) fn startup_update_check(&self) -> Result<StartupUpdateCheck> {
        if self.config.check_interval_hours == 0 {
            debug!("Startup update checks disabled by configuration");
            return Ok(StartupUpdateCheck::default());
        }

        if let Some(pinned_version) = self.config.pinned_version() {
            debug!("Version pinned to {}, suppressing startup update prompt", pinned_version);
            return Ok(StartupUpdateCheck::default());
        }

        let snapshot = cache::read_snapshot()?;
        let dismissed = snapshot.dismissed_version.as_ref();

        let cached_notice = snapshot.latest_version.as_ref().and_then(|latest_version| {
            if snapshot.latest_was_newer && latest_version > &self.current_version && dismissed != Some(latest_version)
            {
                Some(self.notice_for_version(latest_version.clone()))
            } else {
                None
            }
        });

        Ok(StartupUpdateCheck {
            cached_notice,
            should_refresh: self.config.is_check_due(snapshot.last_checked),
        })
    }

    pub(crate) async fn refresh_startup_update_cache(&self) -> Result<Option<StartupUpdateNotice>> {
        if self.config.check_interval_hours == 0 || self.config.is_pinned() {
            return Ok(None);
        }

        let latest = match github::fetch_latest_release_info(self.config.download_timeout_secs).await {
            Ok(info) => info,
            Err(err) => {
                let _ = cache::record_failed_check();
                return Err(err);
            }
        };

        let latest_is_newer = latest.version > self.current_version;
        cache::record_successful_check_with_notes(
            Some(&latest.version),
            latest_is_newer,
            Some(latest.release_notes.as_str()),
        )?;

        Ok(latest_is_newer.then(|| self.notice_for_version(latest.version)))
    }

    pub(crate) async fn install_update(&self, force: bool) -> Result<InstallOutcome> {
        // The CLI path shows download progress. The inline TUI path passes
        // false so download output does not leak into the alternate screen.
        self.install_update_with_progress(force, true).await
    }

    async fn install_update_with_progress(&self, force: bool, show_progress: bool) -> Result<InstallOutcome> {
        self.install_update_reported(force, show_progress, |_| {}).await
    }

    /// Install the latest release with per-phase progress reporting.
    ///
    /// `on_progress` is called with [`UpdateProgress`] events as the pipeline
    /// advances through download, checksum verification, extraction, and binary
    /// replacement. The download callback is throttled internally so callers can
    /// forward each event directly to the UI without flooding the render channel.
    async fn install_update_reported(
        &self,
        force: bool,
        show_progress: bool,
        mut on_progress: impl FnMut(UpdateProgress) + Send,
    ) -> Result<InstallOutcome> {
        let guidance = self.update_guidance();
        if guidance.source.is_managed() {
            bail!("VT Code was installed via {}. Update with: {}", guidance.source.label(), guidance.command());
        }

        let target = install_source::get_target_triple().context("Unsupported platform for auto-update")?;
        let release =
            github::fetch_latest_release_metadata(self.config.download_timeout_secs, &self.config.channel).await?;
        if !force && release.version <= self.current_version {
            return Ok(InstallOutcome::UpToDate(self.current_version.to_string()));
        }

        let (asset, archive_kind) = github::select_archive_asset(&release.assets, target)?;
        let checksum_asset = download::checksum_asset(&release.assets, &asset.name)
            .with_context(|| format!("no checksum metadata was published for update archive {}", asset.name))?;
        let binary_name = install_source::binary_name_for_target(target);
        let temporary_directory = tempfile::tempdir().context("Failed to create update temporary directory")?;
        // Keep remote asset names out of filesystem paths; GitHub metadata is
        // untrusted even though the selected URL and format were validated.
        let archive_path = temporary_directory.path().join("downloaded-update-archive");
        {
            let mut report_download = |downloaded: u64, total: Option<u64>| {
                on_progress(UpdateProgress::Downloading { downloaded, total });
            };
            download::download_asset(
                asset,
                &archive_path,
                std::time::Duration::from_secs(self.config.download_timeout_secs.max(1)),
                show_progress,
                Some(&mut report_download),
            )
            .await
            .context("Failed to download update archive")?;
        }

        on_progress(UpdateProgress::VerifyingChecksum);
        let metadata = download::download_checksum(
            checksum_asset,
            std::time::Duration::from_secs(self.config.download_timeout_secs.max(1)),
        )
        .await
        .with_context(|| format!("failed to download checksum metadata for update archive {}", asset.name))?;
        let expected = download::parse_checksum_metadata(&metadata, &asset.name)
            .with_context(|| format!("checksum metadata did not contain update archive {}", asset.name))?;
        // `verify_file_checksum` does blocking `std::fs::read` + hashing; run it
        // on the blocking pool so the async runtime is not stalled (tokio `fs`
        // tuning: batch blocking file IO into `spawn_blocking`).
        let archive_path_for_checksum = archive_path.clone();
        tokio::task::spawn_blocking(move || download::verify_file_checksum(&archive_path_for_checksum, &expected))
            .await
            .context("Update checksum task join failed")?
            .context("downloaded update archive failed checksum verification")?;

        on_progress(UpdateProgress::Extracting);
        let extraction_directory = temporary_directory.path().join("extracted");
        let archive_path_for_extraction = archive_path.clone();
        let extracted_binary = tokio::task::spawn_blocking(move || {
            archive::extract_binary(&archive_path_for_extraction, archive_kind, &extraction_directory, binary_name)
        })
        .await
        .context("Update extraction task join failed")??;

        on_progress(UpdateProgress::ReplacingBinary);
        self_replace::self_replace(&extracted_binary).context("Failed to replace the current VT Code binary")?;
        // Seed the update cache with the newly installed version's release
        // notes so the next `vtcode` launch shows highlights on its first
        // start (N) instead of waiting for a background refresh to populate
        // the cache (N+1). `latest_was_newer` is false because the installed
        // version is now current. `last_seen_version` is left untouched so
        // `should_show_release_notes_for_current_version` stays true until
        // the TUI displays the notes and records the version as seen.
        seed_release_notes_for_installed_version(&release.version, &release.release_notes);
        Ok(InstallOutcome::Updated(release.version.to_string()))
    }

    pub(crate) fn update_guidance(&self) -> UpdateGuidance {
        let source = install_source::detect_install_source();
        UpdateGuidance { source, action: source.update_action() }
    }

    pub(crate) async fn list_versions(&self, limit: usize) -> Result<Vec<VersionInfo>> {
        debug!("Fetching available versions (limit: {})...", limit);
        github::list_versions(limit, self.config.download_timeout_secs).await
    }

    pub(crate) fn pin_version(&mut self, version: Version, reason: Option<String>, auto_unpin: bool) -> Result<()> {
        self.config.set_pin(version, reason, auto_unpin);
        self.config
            .save()
            .context("Failed to save update config after pinning version")?;
        Ok(())
    }

    pub(crate) fn unpin_version(&mut self) -> Result<()> {
        self.config.clear_pin();
        self.config
            .save()
            .context("Failed to save update config after unpinning version")?;
        Ok(())
    }

    pub(crate) fn is_pinned(&self) -> bool {
        self.config.is_pinned()
    }

    pub(crate) fn pinned_version(&self) -> Option<&Version> {
        self.config.pinned_version()
    }

    pub(crate) fn notice_for_version(&self, latest_version: Version) -> StartupUpdateNotice {
        StartupUpdateNotice {
            current_version: self.current_version.clone(),
            latest_version,
            guidance: self.update_guidance(),
        }
    }
}

/// Check whether release notes should be shown for the current version.
///
/// Returns `true` if the current version has not been recorded as "seen" in the
/// update cache. A cache read failure stays quiet (`false`): the notes are a
/// courtesy, never worth a spurious banner.
///
/// This is the single source of truth for "does this release still owe the
/// user its notes?" — the TUI transcript banner and the desktop inline banner
/// both derive from it, so one release is announced once across surfaces.
pub(crate) fn should_show_release_notes_for_current_version() -> bool {
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return false;
    };
    !current_version_notes_seen_for(&current)
}

/// Whether the running version's release notes were already announced by any
/// VT Code surface (TUI transcript or desktop banner).
///
/// The marker lives in the shared update cache, so the CLI and the desktop app
/// cannot both announce the same release. A cache read failure reports "seen":
/// announcements are a courtesy, so an unreadable cache must stay quiet rather
/// than risk repeating the notes on every launch.
pub(crate) fn current_version_notes_seen() -> bool {
    let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return true;
    };
    current_version_notes_seen_for(&current)
}

fn current_version_notes_seen_for(version: &Version) -> bool {
    match cache::read_snapshot() {
        Ok(snapshot) => snapshot.last_seen_version.as_ref() == Some(version),
        Err(err) => {
            debug!("Failed to read update cache for release notes check: {err}");
            true
        }
    }
}

pub(crate) fn cached_current_release_highlights() -> Option<(Version, Vec<String>)> {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).ok()?;
    parse_cached_highlights(cache::current_release_info(&current))
}

/// Parsed Highlights for the latest known release, served to the desktop
/// `update/status` handler. `None` when the cache holds no notes (or no
/// Highlights section); callers render a link-only state then.
pub(crate) fn cached_latest_release_highlights() -> Option<(Version, Vec<String>)> {
    parse_cached_highlights(cache::latest_release_info())
}

fn parse_cached_highlights(info: Option<(Version, String)>) -> Option<(Version, Vec<String>)> {
    let (version, release_notes) = info?;
    let parsed = parse_highlights(&version, &release_notes);
    (!parsed.items.is_empty()).then_some((version, parsed.items))
}

/// Seed the update cache with the release notes for a newly installed version.
///
/// Called immediately after a successful install (standalone `self_replace`
/// or a managed package-manager command) so the next `vtcode` launch finds
/// `latest_version == current` with notes available and shows highlights on
/// its first start. Best effort: cache failures are ignored.
pub(crate) fn seed_release_notes_for_installed_version(version: &Version, release_notes: &str) {
    let _ = cache::record_successful_check_with_notes(Some(version), false, Some(release_notes));
    // Keep the install record apart from the check slot: a later check that
    // reports an even newer release overwrites `latest_version`/`release_notes`
    // and would otherwise hide the notes this version still owes the user.
    let _ = cache::record_installed_release(version, release_notes);
    let _ = cache::clear_dismissed_version();
}

/// Record that a version's release notes have been shown.
///
/// Called by every surface that announced the notes: `vtcode update` (for the
/// version it just installed, so the next launch stays quiet — the running
/// process still reports the old `CARGO_PKG_VERSION`), the TUI transcript
/// banner, and the desktop banner via `update/acknowledge`.
pub(crate) fn record_version_notes_seen(version: &Version) {
    let _ = cache::record_seen_version(version);
}

/// Record that the current version's release notes have been shown.
pub(crate) fn record_current_version_seen() {
    if let Ok(current) = Version::parse(env!("CARGO_PKG_VERSION")) {
        record_version_notes_seen(&current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_version_parsing() {
        let updater = Updater::new("0.58.4").expect("updater");
        assert_eq!(updater.current_version().major, 0);
        assert_eq!(updater.current_version().minor, 58);
        assert_eq!(updater.current_version().patch, 4);
    }

    #[test]
    fn test_install_source_detection() {
        assert_eq!(
            install_source::detect_install_source_from_path(Path::new("/opt/homebrew/Cellar/vtcode/0.1/bin/vtcode")),
            InstallSource::Homebrew
        );
        assert_eq!(
            install_source::detect_install_source_from_path(Path::new("/Users/dev/.cargo/bin/vtcode")),
            InstallSource::Cargo
        );
        assert_eq!(
            install_source::detect_install_source_from_path(Path::new("/usr/local/bin/vtcode")),
            InstallSource::Standalone
        );
    }

    #[test]
    fn startup_update_check_respects_disabled_interval() {
        let updater = Updater {
            current_version: Version::parse("0.111.0").expect("version"),
            config: UpdateConfig { check_interval_hours: 0, ..UpdateConfig::default() },
        };

        let check = updater.startup_update_check().expect("startup check");
        assert!(check.cached_notice.is_none());
        assert!(!check.should_refresh);
    }

    #[test]
    fn startup_update_check_respects_pinned_version() {
        let mut config = UpdateConfig::default();
        config.set_pin(Version::parse("0.111.0").expect("version"), None, false);
        let updater = Updater {
            current_version: Version::parse("0.111.0").expect("version"),
            config,
        };

        let check = updater.startup_update_check().expect("startup check");
        assert!(check.cached_notice.is_none());
        assert!(!check.should_refresh);
    }

    #[test]
    fn startup_update_check_suppresses_dismissed_version() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let dismissed = Version::parse("0.113.0").expect("dismissed");
        cache::record_successful_check(Some(&dismissed), true).expect("write cache");
        cache::record_dismissed_version(&dismissed).expect("record dismissal");

        let updater = Updater {
            current_version: Version::parse("0.111.0").expect("current"),
            config: UpdateConfig::default(),
        };

        let check = updater.startup_update_check().expect("startup check");
        assert!(check.cached_notice.is_none(), "Dismissed version should not produce a cached notice");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn startup_update_check_shows_undismissed_newer_version() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let latest = Version::parse("0.114.0").expect("latest");
        cache::record_successful_check(Some(&latest), true).expect("write cache");
        // Only dismissed 0.113.0, not 0.114.0 — notice should appear
        cache::record_dismissed_version(&Version::parse("0.113.0").expect("older")).expect("record dismissal");

        let updater = Updater {
            current_version: Version::parse("0.111.0").expect("current"),
            config: UpdateConfig::default(),
        };

        let check = updater.startup_update_check().expect("startup check");
        assert!(check.cached_notice.is_some(), "Newer, non-dismissed version should produce a cached notice");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn seeded_release_notes_show_on_first_launch_after_install() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        // Simulate `vtcode update` installing the running version: seed the
        // cache with highlights, then the first startup must find them without
        // waiting for a background refresh (the N vs N+1 regression).
        let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
        let body = "### Highlights\n- Fresh feature (abc1234)\n";
        seed_release_notes_for_installed_version(&current, body);

        assert!(should_show_release_notes_for_current_version(), "freshly installed version must not be marked seen");
        let highlights = cached_current_release_highlights().expect("seeded highlights must be readable");
        assert_eq!(highlights.0, current);
        assert_eq!(highlights.1, vec!["Fresh feature".to_string()]);

        record_current_version_seen();
        assert!(!should_show_release_notes_for_current_version(), "notes must show once, then be marked seen");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn seeded_release_notes_survive_a_later_newer_check() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        // The updater installs the running version and seeds its notes, then a
        // preflight check reports an even newer release before first launch
        // (the common N+1 race). The installed version's notes must still be
        // readable instead of being handed the newer release's changelog.
        let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
        seed_release_notes_for_installed_version(&current, "### Highlights\n- Fresh feature (abc1234)\n");

        let newer = Version::parse("99.0.0").expect("newer version");
        cache::record_successful_check_with_notes(
            Some(&newer),
            true,
            Some("### Highlights\n- Later feature (def5678)\n"),
        )
        .expect("write newer check");

        let served = cached_current_release_highlights().expect("install notes survive the newer check");
        assert_eq!(served.0, current);
        assert_eq!(served.1, vec!["Fresh feature".to_string()]);
        assert_eq!(cached_latest_release_highlights().map(|(version, _)| version), Some(newer));

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn seen_marker_is_per_version_so_update_can_acknowledge_the_installed_release() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        // `vtcode update` prints the new release's notes before the replaced
        // binary exits, so it acknowledges that version explicitly while
        // `CARGO_PKG_VERSION` is still the old one.
        let installed = Version::parse("99.1.0").expect("installed version");
        record_version_notes_seen(&installed);

        assert!(cache::current_release_info(&Version::parse(env!("CARGO_PKG_VERSION")).expect("current")).is_none());
        assert!(!current_version_notes_seen(), "acknowledging the new release does not mark the running one");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn current_version_notes_seen_tracks_the_shared_marker() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        assert!(!current_version_notes_seen(), "a fresh cache reports unseen notes");
        record_current_version_seen();
        assert!(current_version_notes_seen(), "the shared marker records the running version");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn seeing_an_installed_release_drops_its_cached_body() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        // The install record bridges install time to the first announcement;
        // once seen it must not linger as a second copy of the changelog.
        let current = Version::parse(env!("CARGO_PKG_VERSION")).expect("current version");
        seed_release_notes_for_installed_version(&current, "### Highlights\n- Fresh feature (abc1234)\n");
        let snapshot = cache::read_snapshot().expect("snapshot");
        assert!(snapshot.installed_release.is_some(), "install record exists before it is seen");

        record_current_version_seen();
        let snapshot = cache::read_snapshot().expect("snapshot");
        assert!(snapshot.installed_release.is_none(), "the seen install record is dropped");

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn an_unreadable_cache_stays_quiet_about_release_notes() {
        // The gate and its "seen" mirror must agree on a broken cache: an
        // announcement is a courtesy, so neither may surface one.
        let previous =
            cache::set_cache_dir_override_for_tests(Some(std::path::PathBuf::from("/nonexistent/vtcode-cache")));
        assert!(current_version_notes_seen(), "an unreadable cache reports seen");
        assert!(!should_show_release_notes_for_current_version(), "and never announces");
        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn latest_release_highlights_serve_a_newer_cached_release() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        // A background check found a newer release: its notes belong to the
        // desktop status snapshot, not to the current-version startup banner.
        let latest = Version::parse("99.0.0").expect("latest version");
        assert_ne!(latest.to_string(), env!("CARGO_PKG_VERSION"));
        let body = "### Highlights\n- Shiny feature (abc1234)\n";
        cache::record_successful_check_with_notes(Some(&latest), true, Some(body)).expect("write cache");

        let served = cached_latest_release_highlights().expect("latest highlights must be readable");
        assert_eq!(served.0, latest);
        assert_eq!(served.1, vec!["Shiny feature".to_string()]);
        assert!(
            cached_current_release_highlights().is_none(),
            "newer notes must not leak into the current-version banner"
        );

        cache::set_cache_dir_override_for_tests(previous);
    }

    #[test]
    fn latest_release_highlights_are_none_without_notes_or_section() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("temp dir");
        let previous = cache::set_cache_dir_override_for_tests(Some(temp_dir.path().to_path_buf()));

        let latest = Version::parse("99.0.0").expect("latest version");
        cache::record_successful_check_with_notes(Some(&latest), true, None).expect("write cache");
        assert!(cached_latest_release_highlights().is_none(), "no cached notes serves no highlights");

        cache::record_successful_check_with_notes(Some(&latest), true, Some("## 99.0.0\nNo highlights here\n"))
            .expect("write cache");
        assert!(
            cached_latest_release_highlights().is_none(),
            "notes without a Highlights section serve no highlights"
        );

        cache::set_cache_dir_override_for_tests(previous);
    }
}
