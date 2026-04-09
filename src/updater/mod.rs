mod cache;
mod github;
mod install_source;
mod interactive;
mod types;

use anyhow::{Context, Result, bail};
use semver::Version;
use tracing::{debug, info};
use vtcode_config::update::UpdateConfig;

pub(crate) use install_source::InstallSource;
pub(crate) use interactive::{
    InlineUpdateOutcome, append_notice_highlight, display_update_notice, execute_inline_update,
    run_inline_update_prompt,
};
pub(crate) use types::{
    InstallOutcome, StartupUpdateCheck, StartupUpdateNotice, UpdateExecutionStrategy,
    UpdateGuidance, UpdateInfo, VersionInfo,
};

/// Auto-updater for VT Code binary from GitHub Releases
pub(crate) struct Updater {
    current_version: Version,
    config: UpdateConfig,
}

impl Updater {
    pub(crate) fn new(current_version_str: &str) -> Result<Self> {
        let current_version = Version::parse(current_version_str)
            .with_context(|| format!("Invalid version format: {}", current_version_str))?;

        let config = UpdateConfig::load().unwrap_or_else(|e| {
            debug!("Failed to load update config, using defaults: {}", e);
            UpdateConfig::default()
        });

        Ok(Self {
            current_version,
            config,
        })
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

    pub(crate) async fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
        debug!(
            "Checking for VT Code updates (channel: {})...",
            self.config.channel
        );

        if let Some(pinned_version) = self.config.pinned_version() {
            debug!(
                "Version pinned to {}, skipping update check",
                pinned_version
            );
            return Ok(None);
        }

        let latest = github::fetch_latest_release(self).await?;

        if latest
            .as_ref()
            .is_some_and(|info| info.version > self.current_version)
        {
            if let Some(latest) = latest.as_ref() {
                info!(
                    "New version available: {} (current: {})",
                    latest.version, self.current_version
                );
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
            debug!(
                "Version pinned to {}, suppressing startup update prompt",
                pinned_version
            );
            return Ok(StartupUpdateCheck::default());
        }

        let snapshot = cache::read_snapshot()?;
        let cached_notice = snapshot.latest_version.as_ref().and_then(|latest_version| {
            if snapshot.latest_was_newer && latest_version > &self.current_version {
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

        let latest = match github::fetch_latest_release_info().await {
            Ok(info) => info,
            Err(err) => {
                let _ = cache::record_failed_check();
                return Err(err);
            }
        };

        let latest_is_newer = latest.version > self.current_version;
        cache::record_successful_check(Some(&latest.version), latest_is_newer)?;

        Ok(latest_is_newer.then(|| self.notice_for_version(latest.version)))
    }

    pub(crate) async fn install_update(&self, force: bool) -> Result<InstallOutcome> {
        let guidance = self.update_guidance();
        if guidance.source.is_managed() {
            bail!(
                "VT Code was installed via {}. Update with: {}",
                guidance.source.label(),
                guidance.command()
            );
        }

        let current_version = self.current_version.to_string();
        let target =
            install_source::get_target_triple().context("Unsupported platform for auto-update")?;

        let status = tokio::task::spawn_blocking(move || {
            // Use .tar.gz as identifier to specifically match tarball archives
            // and avoid .sha256 checksum files that have the same target triple
            let identifier = format!("{target}.tar.gz");
            let mut builder = self_update::backends::github::Update::configure();
            builder
                .repo_owner(github::REPO_OWNER)
                .repo_name(github::REPO_NAME)
                .bin_name("vtcode")
                .target(target)
                .identifier(&identifier)
                .show_download_progress(true)
                .no_confirm(true);

            if force {
                builder.current_version("0.0.0");
            } else {
                builder.current_version(&current_version);
            }

            let status = builder
                .build()
                .context("Failed to build self-update request")?
                .update()
                .context("Failed to apply self-update")?;

            Ok::<self_update::Status, anyhow::Error>(status)
        })
        .await
        .context("Update task join failed")??;

        match status {
            self_update::Status::Updated(version) => Ok(InstallOutcome::Updated(version)),
            self_update::Status::UpToDate(version) => Ok(InstallOutcome::UpToDate(version)),
        }
    }

    pub(crate) fn update_guidance(&self) -> UpdateGuidance {
        let source = install_source::detect_install_source();
        UpdateGuidance {
            source,
            action: source.update_action(),
        }
    }

    pub(crate) async fn list_versions(&self, limit: usize) -> Result<Vec<VersionInfo>> {
        debug!("Fetching available versions (limit: {})...", limit);
        github::list_versions(limit).await
    }

    pub(crate) fn pin_version(
        &mut self,
        version: Version,
        reason: Option<String>,
        auto_unpin: bool,
    ) -> Result<()> {
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
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_identifier_selects_tarball_not_checksum() {
        // Verify that our identifier correctly picks .tar.gz over .sha256 files
        let target = "aarch64-apple-darwin";
        let identifier = format!("{target}.tar.gz");
        
        // Simulate the asset selection logic from self_update
        let assets = vec![
            "checksums.txt",
            "vtcode-0.98.1-aarch64-apple-darwin.sha256",
            "vtcode-0.98.1-aarch64-apple-darwin.tar.gz",
            "vtcode-0.98.1-x86_64-apple-darwin.sha256",
            "vtcode-0.98.1-x86_64-apple-darwin.tar.gz",
        ];
        
        let selected = assets.iter().find(|asset| {
            asset.contains(target) && asset.contains(&identifier)
        });
        
        assert_eq!(
            selected,
            Some(&"vtcode-0.98.1-aarch64-apple-darwin.tar.gz"),
            "Should select the .tar.gz archive, not the .sha256 checksum file"
        );
    }

    #[test]
    fn test_identifier_prevents_wrong_target_selection() {
        // Verify identifier doesn't accidentally pick assets for wrong target
        let target = "aarch64-apple-darwin";
        let identifier = format!("{target}.tar.gz");
        
        let assets = vec![
            "vtcode-0.98.1-x86_64-apple-darwin.sha256",
            "vtcode-0.98.1-x86_64-apple-darwin.tar.gz",
        ];
        
        let selected = assets.iter().find(|asset| {
            asset.contains(target) && asset.contains(&identifier)
        });
        
        assert!(
            selected.is_none(),
            "Should not select x86_64 assets when target is aarch64"
        );
    }

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
            install_source::detect_install_source_from_path(Path::new(
                "/opt/homebrew/Cellar/vtcode/0.1/bin/vtcode"
            )),
            InstallSource::Homebrew
        );
        assert_eq!(
            install_source::detect_install_source_from_path(Path::new(
                "/Users/dev/.cargo/bin/vtcode"
            )),
            InstallSource::Cargo
        );
        assert_eq!(
            install_source::detect_install_source_from_path(Path::new(
                "/usr/local/lib/node_modules/vtcode/bin/vtcode"
            )),
            InstallSource::Npm
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
            config: UpdateConfig {
                check_interval_hours: 0,
                ..UpdateConfig::default()
            },
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
}
