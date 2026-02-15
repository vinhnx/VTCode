use anyhow::{Context, Result};
use semver::Version;
use std::env;
use std::path::PathBuf;
use tracing::{debug, info};
use vtcode_core::utils::file_utils::{ensure_dir_exists_sync, write_file_with_context_sync};

/// Auto-updater for VT Code binary from GitHub Releases
pub struct Updater {
    repo: String,
    current_version: Version,
}

impl Updater {
    /// Create a new updater instance
    pub fn new(current_version_str: &str) -> Result<Self> {
        let current_version = Version::parse(current_version_str)
            .with_context(|| format!("Invalid version format: {}", current_version_str))?;

        Ok(Self {
            repo: "vinhnx/vtcode".to_string(),
            current_version,
        })
    }

    /// Check for updates (non-blocking, respects rate limits)
    pub async fn check_for_updates(&self) -> Result<Option<UpdateInfo>> {
        if !Self::should_check_for_updates()? {
            debug!("Skipping update check (checked recently)");
            return Ok(None);
        }

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

    /// Fetch latest release info from GitHub API
    async fn fetch_latest_release(&self) -> Result<UpdateInfo> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);

        let client = reqwest::Client::builder()
            .user_agent("vtcode-updater")
            .build()
            .context("Failed to create HTTP client")?;

        let response = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .context("Failed to fetch latest release from GitHub")?;

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
            "https://github.com/{}/releases/download/v{}/vtcode-v{}-{}.{}",
            self.repo, version, version, target, file_ext
        ))
    }

    /// Get platform target triple
    fn get_target_triple() -> Option<&'static str> {
        match (env::consts::OS, env::consts::ARCH) {
            ("macos", "x86_64") => Some("x86_64-apple-darwin"),
            ("macos", "aarch64") => Some("aarch64-apple-darwin"),
            ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
            ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
            _ => None,
        }
    }

    /// Check if enough time has passed since last update check
    fn should_check_for_updates() -> Result<bool> {
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

        Ok(elapsed.as_secs() > 24 * 3600) // 24 hours
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
        let updater = Updater::new("0.58.4").unwrap();
        assert_eq!(updater.current_version.major, 0);
        assert_eq!(updater.current_version.minor, 58);
        assert_eq!(updater.current_version.patch, 4);
    }

    #[test]
    fn test_target_triple_macos_intel() {
        // We can't directly test platform detection without mocking,
        // but we can verify the function exists and returns appropriate values
        let triple = Updater::get_target_triple();
        assert!(triple.is_some());
    }

    #[test]
    fn test_update_info_major() {
        let current = Version::parse("1.0.0").unwrap();
        let update = UpdateInfo {
            version: Version::parse("2.0.0").unwrap(),
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
        let current = Version::parse("1.0.0").unwrap();
        let update = UpdateInfo {
            version: Version::parse("1.1.0").unwrap(),
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
        let current = Version::parse("1.0.0").unwrap();
        let update = UpdateInfo {
            version: Version::parse("1.0.1").unwrap(),
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
            version: Version::parse("1.0.0-alpha.1").unwrap(),
            tag: "v1.0.0-alpha.1".to_string(),
            download_url: "http://example.com".to_string(),
            release_notes: "".to_string(),
        };
        assert!(update.is_prerelease());
    }
}
