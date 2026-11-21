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

pub(crate) use release_notes::backfill_current_release_notes;
use release_notes::record_fetched_release;

pub(crate) use desktop::{
    DesktopInstallOutcome, default_install_dir, desktop_macos_major, desktop_supported, existing_install_dir,
    install_desktop_app, install_desktop_app_with_progress, installed_desktop_version,
};
pub(crate) use install_source::InstallSource;
pub(crate) use interactive::{
    InlineUpdateOutcome, append_notice_highlight, display_release_notes, display_update_notice, execute_inline_update,
    run_inline_update_prompt,
};
pub(crate) use preflight::{get_preflight_notice, run_preflight_check};
pub(crate) use progress::UpdateProgress;
pub(crate) use release_notes::{
    cached_current_release_highlights, cached_latest_release, current_version_notes_seen, parse_highlights,
    record_current_version_seen, resolve_release_notes_for_display, seed_release_notes_for_installed_version,
    should_show_release_notes_for_current_version,
};
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
            match github::fetch_latest_release(self, self.config.download_timeout_secs, &self.config.channel).await {
                Ok(info) => info,
                Err(err) => {
                    let _ = cache::record_failed_check();
                    return Err(err);
                }
            };

        if latest.as_ref().is_some_and(|info| info.version > self.current_version) {
            if let Some(latest) = latest.as_ref() {
                info!("New version available: {} (current: {})", latest.version, self.current_version);
            }
        } else {
            debug!("Already on latest version");
        }

        // Record the check like the background refresh does, so the fetched
        // release's notes are in the cache before any install. The inline
        // managed update (`/update install` on a Homebrew/cargo install) runs
        // the package manager and relaunches without going through
        // `install_update_reported`; without this record the relaunched first
        // start reads a stale `latest_version` and defers the notes to a
        // second start (the N vs N+1 regression).
        if let Some(info) = latest.as_ref() {
            record_fetched_release(&self.current_version, info);
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

        // Channel-aware: a beta/nightly follower must check its own channel,
        // not stable. For stable this resolves to the same latest endpoint
        // the old stable-only helper used.
        let latest =
            match github::fetch_latest_for_channel(self.config.download_timeout_secs, &self.config.channel).await {
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
        let binary_name = install_source::binary_name_for_target(target);
        let download_timeout = std::time::Duration::from_secs(self.config.download_timeout_secs.max(1));
        // Resolve the checksum before the archive: it is a few hundred bytes,
        // so a checksum outage — or a release that published none — fails
        // before the multi-megabyte archive is downloaded and discarded.
        let expected = download::download_expected_checksum(&release.assets, &asset.name, download_timeout).await?;
        let temporary_directory = tempfile::tempdir().context("Failed to create update temporary directory")?;
        // Keep remote asset names out of filesystem paths; GitHub metadata is
        // untrusted even though the selected URL and format were validated.
        let archive_path = temporary_directory.path().join("downloaded-update-archive");
        {
            let mut report_download = |downloaded: u64, total: Option<u64>| {
                on_progress(UpdateProgress::Downloading { downloaded, total });
            };
            download::download_asset(asset, &archive_path, download_timeout, show_progress, Some(&mut report_download))
                .await
                .context("Failed to download update archive")?;
        }

        on_progress(UpdateProgress::VerifyingChecksum);
        // Stream and hash the staged file on the blocking pool.
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

#[cfg(test)]
mod tests;
