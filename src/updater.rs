use anyhow::{Context, Result, bail};
use semver::Version;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info};
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

const REPO_OWNER: &str = "vinhnx";
const REPO_NAME: &str = "vtcode";
const REPO_SLUG: &str = "vinhnx/vtcode";
const CURL_INSTALL_COMMAND: &str =
    "curl -fsSL https://raw.githubusercontent.com/vinhnx/vtcode/main/scripts/install.sh | bash";

/// Auto-updater for VT Code binary from GitHub Releases
pub struct Updater {
    current_version: Version,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallSource {
    Standalone,
    Homebrew,
    Cargo,
    Npm,
}

impl InstallSource {
    pub fn is_managed(self) -> bool {
        !matches!(self, Self::Standalone)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Homebrew => "homebrew",
            Self::Cargo => "cargo",
            Self::Npm => "npm",
        }
    }

    pub fn update_command(self) -> &'static str {
        match self {
            Self::Standalone => CURL_INSTALL_COMMAND,
            Self::Homebrew => "brew upgrade vinhnx/tap/vtcode",
            Self::Cargo => "cargo install vtcode --force",
            Self::Npm => "npm install -g vtcode@latest",
        }
    }
}

pub struct UpdateGuidance {
    pub source: InstallSource,
    pub command: String,
}

pub enum InstallOutcome {
    Updated(String),
    UpToDate(String),
}

impl Updater {
    /// Create a new updater instance
    pub fn new(current_version_str: &str) -> Result<Self> {
        let current_version = Version::parse(current_version_str)
            .with_context(|| format!("Invalid version format: {}", current_version_str))?;

        Ok(Self { current_version })
    }

    pub fn current_version(&self) -> &Version {
        &self.current_version
    }

    pub fn release_url(version: &Version) -> String {
        format!("https://github.com/{REPO_SLUG}/releases/tag/v{version}")
    }

    /// Check for updates without cache throttling.
    pub async fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
        debug!("Checking for VT Code updates...");

        let latest = self.fetch_latest_release().await?;

        if latest.version > self.current_version {
            info!(
                "New version available: {} (current: {})",
                latest.version, self.current_version
            );
            Ok(Some(latest))
        } else {
            debug!("Already on latest version");
            Ok(None)
        }
    }

    /// Check for updates with a cache cooldown.
    pub async fn check_for_updates_cached(
        &self,
        min_interval: Duration,
    ) -> Result<Option<UpdateInfo>> {
        if !Self::is_check_due(min_interval)? {
            debug!("Skipping update check (checked recently)");
            return Ok(None);
        }

        let result = self.check_for_updates().await;
        let _ = Self::record_update_check();
        result
    }

    /// Install latest update from GitHub Releases.
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
        let target = Self::get_target_triple().context("Unsupported platform for auto-update")?;

        let status = tokio::task::spawn_blocking(move || {
            let mut builder = self_update::backends::github::Update::configure();
            builder
                .repo_owner(REPO_OWNER)
                .repo_name(REPO_NAME)
                .bin_name("vtcode")
                .target(target)
                .show_download_progress(true)
                .no_confirm(true);

            if force {
                // Force path: treat current version as very old to trigger reinstall.
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
        let source = Self::detect_install_source();
        UpdateGuidance {
            source,
            command: source.update_command().to_string(),
        }
    }

    /// Fetch latest release info from GitHub API
    async fn fetch_latest_release(&self) -> Result<UpdateInfo> {
        let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases/latest");

        let client = reqwest::Client::builder()
            .user_agent("vtcode-updater")
            .build()
            .context("Failed to create HTTP client")?;

        let response = client
            .get(&url)
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .context("Failed to fetch latest release from GitHub")?
            .error_for_status()
            .context("GitHub API returned non-success status")?;

        let json = response
            .json::<serde_json::Value>()
            .await
            .context("Failed to parse GitHub API response")?;

        let tag_name = json
            .get("tag_name")
            .and_then(|v| v.as_str())
            .context("Missing tag_name in GitHub response")?;

        let version_str = tag_name.trim_start_matches('v');
        let version = Version::parse(version_str)
            .with_context(|| format!("Invalid version in GitHub release: {}", tag_name))?;

        let download_url = self.get_download_url(&version)?;

        Ok(UpdateInfo {
            version,
            tag: tag_name.to_string(),
            download_url,
            release_notes: json
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("See release notes on GitHub")
                .to_string(),
        })
    }

    /// Generate platform-specific download URL
    fn get_download_url(&self, version: &Version) -> Result<String> {
        let target = Self::get_target_triple().context("Unsupported platform for auto-update")?;

        let file_ext = if target.contains("windows") {
            "zip"
        } else {
            "tar.gz"
        };

        Ok(format!(
            "https://github.com/{REPO_SLUG}/releases/download/v{version}/vtcode-v{version}-{target}.{file_ext}"
        ))
    }

    /// Get platform target triple
    fn get_target_triple() -> Option<&'static str> {
        match (env::consts::OS, env::consts::ARCH) {
            ("macos", "x86_64") => Some("x86_64-apple-darwin"),
            ("macos", "aarch64") => Some("aarch64-apple-darwin"),
            ("linux", "x86_64") => Some("x86_64-unknown-linux-musl"),
            ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
            ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
            _ => None,
        }
    }

    fn detect_install_source() -> InstallSource {
        let exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(_) => return InstallSource::Standalone,
        };

        let canonical = std::fs::canonicalize(&exe).unwrap_or(exe);
        Self::detect_install_source_from_path(&canonical)
    }

    fn detect_install_source_from_path(path: &Path) -> InstallSource {
        let path_text = path.to_string_lossy().to_ascii_lowercase();

        if path_text.contains("/cellar/")
            || path_text.contains("/homebrew/")
            || path_text.contains("/linuxbrew/")
            || path_text.contains("/opt/homebrew/")
        {
            return InstallSource::Homebrew;
        }

        if path_text.contains("/.cargo/bin/") {
            return InstallSource::Cargo;
        }

        if path_text.contains("/node_modules/") || path_text.contains("/npm/") {
            return InstallSource::Npm;
        }

        InstallSource::Standalone
    }

    /// Check if enough time has passed since last update check
    fn is_check_due(min_interval: Duration) -> Result<bool> {
        let cache_dir = Self::get_cache_dir()?;
        let last_check_file = cache_dir.join("last_update_check");

        if !last_check_file.exists() {
            return Ok(true);
        }

        let metadata = std::fs::metadata(&last_check_file)
            .context("Failed to read last update check timestamp")?;

        let modified = metadata
            .modified()
            .context("Failed to get modification time")?;

        let elapsed = std::time::SystemTime::now()
            .duration_since(modified)
            .context("Failed to calculate elapsed time")?;

        Ok(elapsed >= min_interval)
    }

    /// Record that we checked for updates
    pub fn record_update_check() -> Result<()> {
        let cache_dir = Self::get_cache_dir()?;
        write_file_with_context_sync(
            &cache_dir.join("last_update_check"),
            "",
            "update check timestamp",
        )
        .context("Failed to record update check timestamp")?;
        Ok(())
    }

    /// Get VT Code cache directory
    fn get_cache_dir() -> Result<PathBuf> {
        let dir = if let Ok(xdg_cache) = env::var("XDG_CACHE_HOME") {
            PathBuf::from(xdg_cache).join("vtcode")
        } else {
            let home = dirs::home_dir().context("Cannot determine home directory")?;
            home.join(".cache/vtcode")
        };

        ensure_dir_exists_sync(&dir).context("Failed to create cache directory")?;
        Ok(dir)
    }
}

/// Information about an available update
pub struct UpdateInfo {
    pub version: Version,
    pub tag: String,
    pub download_url: String,
    pub release_notes: String,
}

impl UpdateInfo {
    /// Check if this is a major version update
    pub fn is_major_update(&self, current: &Version) -> bool {
        self.version.major > current.major
    }

    /// Check if this is a minor update
    pub fn is_minor_update(&self, current: &Version) -> bool {
        self.version.major == current.major && self.version.minor > current.minor
    }

    /// Check if this is a patch update
    pub fn is_patch_update(&self, current: &Version) -> bool {
        self.version.major == current.major
            && self.version.minor == current.minor
            && self.version.patch > current.patch
    }

    /// Check if this is a pre-release
    pub fn is_prerelease(&self) -> bool {
        !self.version.pre.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            Updater::detect_install_source_from_path(Path::new(
                "/opt/homebrew/Cellar/vtcode/0.1/bin/vtcode"
            )),
            InstallSource::Homebrew
        );
        assert_eq!(
            Updater::detect_install_source_from_path(Path::new("/Users/dev/.cargo/bin/vtcode")),
            InstallSource::Cargo
        );
        assert_eq!(
            Updater::detect_install_source_from_path(Path::new(
                "/usr/local/lib/node_modules/vtcode/bin/vtcode"
            )),
            InstallSource::Npm
        );
        assert_eq!(
            Updater::detect_install_source_from_path(Path::new("/usr/local/bin/vtcode")),
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
