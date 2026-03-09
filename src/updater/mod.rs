mod cache;
mod github;
mod install_source;
mod types;

use anyhow::{Context, Result, bail};
use semver::Version;
use std::time::Duration;
use tracing::{debug, info};
use vtcode_config::update::UpdateConfig;

pub use install_source::InstallSource;
pub use types::{InstallOutcome, UpdateGuidance, UpdateInfo, VersionInfo};

/// Auto-updater for VT Code binary from GitHub Releases
pub struct Updater {
    current_version: Version,
    config: UpdateConfig,
}

impl Updater {
    pub fn new(current_version_str: &str) -> Result<Self> {
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

    pub fn with_config(current_version_str: &str, config: UpdateConfig) -> Result<Self> {
        let current_version = Version::parse(current_version_str)
            .with_context(|| format!("Invalid version format: {}", current_version_str))?;

        Ok(Self {
            current_version,
            config,
        })
    }

    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn config(&self) -> &UpdateConfig {
        &self.config
    }

    pub fn release_url(version: &Version) -> String {
        github::release_url(version)
    }

    pub async fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
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

    pub async fn check_for_updates_cached(
        &self,
        min_interval: Duration,
    ) -> Result<Option<UpdateInfo>> {
        if !cache::is_check_due(min_interval)? {
            debug!("Skipping update check (checked recently)");
            return Ok(None);
        }

        let result = self.check_for_updates().await;
        let _ = cache::record_update_check();
        result
    }

    pub async fn install_update(&self, force: bool) -> Result<InstallOutcome> {
        let guidance = self.update_guidance();
        if guidance.source.is_managed() {
            bail!(
                "VT Code was installed via {}. Update with: {}",
                guidance.source.label(),
                guidance.command
            );
        }

        let current_version = self.current_version.to_string();
        let target =
            install_source::get_target_triple().context("Unsupported platform for auto-update")?;

        let status = tokio::task::spawn_blocking(move || {
            let mut builder = self_update::backends::github::Update::configure();
            builder
                .repo_owner(github::REPO_OWNER)
                .repo_name(github::REPO_NAME)
                .bin_name("vtcode")
                .target(target)
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

    pub fn update_guidance(&self) -> UpdateGuidance {
        let source = install_source::detect_install_source();
        UpdateGuidance {
            source,
            command: source.update_command().to_string(),
        }
    }

    pub fn record_update_check() -> Result<()> {
        cache::record_update_check()
    }

    pub async fn list_versions(&self, limit: usize) -> Result<Vec<VersionInfo>> {
        debug!("Fetching available versions (limit: {})...", limit);
        github::list_versions(limit).await
    }

    pub fn pin_version(
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

    pub fn unpin_version(&mut self) -> Result<()> {
        self.config.clear_pin();
        self.config
            .save()
            .context("Failed to save update config after unpinning version")?;
        Ok(())
    }

    pub fn is_pinned(&self) -> bool {
        self.config.is_pinned()
    }

    pub fn pinned_version(&self) -> Option<&Version> {
        self.config.pinned_version()
    }
}

impl UpdateInfo {
    pub fn is_major_update(&self, current: &Version) -> bool {
        self.version.major > current.major
    }

    pub fn is_minor_update(&self, current: &Version) -> bool {
        self.version.major == current.major && self.version.minor > current.minor
    }

    pub fn is_patch_update(&self, current: &Version) -> bool {
        self.version.major == current.major
            && self.version.minor == current.minor
            && self.version.patch > current.patch
    }

    pub fn is_prerelease(&self) -> bool {
        !self.version.pre.is_empty()
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
    fn test_update_info_major() {
        let current = Version::parse("1.0.0").expect("current");
        let update = UpdateInfo {
            version: Version::parse("2.0.0").expect("version"),
            tag: "v2.0.0".to_string(),
            download_url: "http://example.com".to_string(),
            release_notes: "".to_string(),
        };
        assert!(update.is_major_update(&current));
        assert!(!update.is_minor_update(&current));
        assert!(!update.is_patch_update(&current));
    }

    #[test]
    fn test_update_info_minor() {
        let current = Version::parse("1.0.0").expect("current");
        let update = UpdateInfo {
            version: Version::parse("1.1.0").expect("version"),
            tag: "v1.1.0".to_string(),
            download_url: "http://example.com".to_string(),
            release_notes: "".to_string(),
        };
        assert!(!update.is_major_update(&current));
        assert!(update.is_minor_update(&current));
        assert!(!update.is_patch_update(&current));
    }

    #[test]
    fn test_update_info_patch() {
        let current = Version::parse("1.0.0").expect("current");
        let update = UpdateInfo {
            version: Version::parse("1.0.1").expect("version"),
            tag: "v1.0.1".to_string(),
            download_url: "http://example.com".to_string(),
            release_notes: "".to_string(),
        };
        assert!(!update.is_major_update(&current));
        assert!(!update.is_minor_update(&current));
        assert!(update.is_patch_update(&current));
    }

    #[test]
    fn test_prerelease_detection() {
        let update = UpdateInfo {
            version: Version::parse("1.0.0-alpha.1").expect("version"),
            tag: "v1.0.0-alpha.1".to_string(),
            download_url: "http://example.com".to_string(),
            release_notes: "".to_string(),
        };
        assert!(update.is_prerelease());
    }
}
