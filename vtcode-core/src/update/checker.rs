//! Version checking and update detection

use super::{
    CURRENT_VERSION, GITHUB_REPO_NAME, GITHUB_REPO_OWNER, UpdateStatus, config::UpdateConfig,
};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;
use update_informer::{Check, registry};

/// GitHub release information
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubRelease {
    tag_name: String,
    name: String,
    body: Option<String>,
    prerelease: bool,
    draft: bool,
    assets: Vec<GitHubAsset>,
    published_at: String,
    #[serde(default)]
    tarball_url: Option<String>,
    #[serde(default)]
    zipball_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Handles checking for available updates
pub struct UpdateChecker {
    config: UpdateConfig,
    // Keep reqwest client for other operations like downloading release notes
    client: reqwest::Client,
    last_check_file: PathBuf,
}

impl UpdateChecker {
    pub fn new(config: UpdateConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(format!("vtcode/{}", CURRENT_VERSION))
            .build()?;

        let last_check_file = config.update_dir.join("last_check.json");

        Ok(Self {
            config,
            client,
            last_check_file,
        })
    }

    /// Check if an update is available
    pub async fn check_for_updates(&self) -> Result<UpdateStatus> {
        // Check if we should skip based on frequency
        if !self.should_check()? {
            return self.load_cached_status();
        }

        // Use update-informer to check for the latest version from GitHub
        let update_informer = update_informer::new(
            registry::GitHub,
            format!("{}/{}", GITHUB_REPO_OWNER, GITHUB_REPO_NAME),
            CURRENT_VERSION,
        )
        .interval(std::time::Duration::from_secs(60 * 60 * 24));

        let mut fallback_release: Option<GitHubRelease> = None;

        let informer_result = update_informer
            .check_version()
            .map_err(|err| err.to_string());

        let latest_version = match informer_result {
            Ok(Some(latest_version)) => latest_version.to_string(),
            Ok(None) => {
                // No newer version available
                let status = UpdateStatus {
                    current_version: CURRENT_VERSION.to_string(),
                    latest_version: Some(CURRENT_VERSION.to_string()),
                    update_available: false,
                    download_url: None,
                    release_notes: None,
                    last_checked: Some(chrono::Utc::now()),
                };

                self.save_status(&status)?;
                return Ok(status);
            }
            Err(err) => {
                debug!(
                    "update-informer check failed; falling back to GitHub API: {}",
                    err
                );

                let release = self.fetch_latest_release().await?;
                let tag_name = release.tag_name.clone();
                fallback_release = Some(release);
                tag_name
            }
        };

        let current_version = semver::Version::parse(CURRENT_VERSION.trim_start_matches('v'))
            .map_err(|e| anyhow::anyhow!("Failed to parse current version: {}", e))?;

        let latest_version_parsed = semver::Version::parse(latest_version.trim_start_matches('v'))
            .map_err(|e| anyhow::anyhow!("Failed to parse latest version: {}", e))?;

        let update_available = latest_version_parsed > current_version;

        let (download_url, release_notes) = if update_available {
            let release = match fallback_release {
                Some(release) => release,
                None => self.fetch_latest_release_by_tag(&latest_version).await?,
            };

            let download_url = if release.assets.is_empty() {
                tracing::warn!(
                    "No binary assets found for release {}, only source distribution available",
                    release.tag_name
                );
                None
            } else {
                self.find_platform_asset(&release.assets)?
            };

            let release_notes = release.body;
            (download_url, release_notes)
        } else {
            (None, None)
        };

        let status = UpdateStatus {
            current_version: CURRENT_VERSION.to_string(),
            latest_version: Some(latest_version),
            update_available,
            download_url,
            release_notes,
            last_checked: Some(chrono::Utc::now()),
        };

        // Cache the status
        self.save_status(&status)?;

        Ok(status)
    }

    /// Fetch a specific release by tag from GitHub
    async fn fetch_latest_release_by_tag(&self, tag: &str) -> Result<GitHubRelease> {
        let url = format!(
            "{}/repos/{}/{}/releases/tags/{}",
            self.config.github_api_base(),
            GITHUB_REPO_OWNER,
            GITHUB_REPO_NAME,
            tag
        );

        let mut request = self.client.get(&url);

        // Add authentication if token is available
        if let Some(token) = &self.config.github_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .context("Failed to fetch release information")?;

        if !response.status().is_success() {
            anyhow::bail!("GitHub API returned error: {}", response.status());
        }

        let release: GitHubRelease = response
            .json()
            .await
            .context("Failed to parse release information")?;

        Ok(release)
    }

    /// Fetch the latest release from GitHub (fallback method)
    async fn fetch_latest_release(&self) -> Result<GitHubRelease> {
        let url = format!(
            "{}/repos/{}/{}/releases/latest",
            self.config.github_api_base(),
            GITHUB_REPO_OWNER,
            GITHUB_REPO_NAME
        );

        let mut request = self.client.get(&url);

        // Add authentication if token is available
        if let Some(token) = &self.config.github_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        let response = request
            .send()
            .await
            .context("Failed to fetch release information")?;

        if !response.status().is_success() {
            anyhow::bail!("GitHub API returned error: {}", response.status());
        }

        let release: GitHubRelease = response
            .json()
            .await
            .context("Failed to parse release information")?;

        // Filter based on channel
        if release.draft {
            anyhow::bail!("Latest release is a draft");
        }

        if release.prerelease && self.config.channel == super::config::UpdateChannel::Stable {
            anyhow::bail!("Latest release is a pre-release but stable channel is configured");
        }

        Ok(release)
    }

    /// Find the appropriate asset for the current platform
    fn find_platform_asset(&self, assets: &[GitHubAsset]) -> Result<Option<String>> {
        let target = self.get_target_triple();

        // Try exact target match first
        for asset in assets {
            if asset.name.contains(&target) {
                tracing::debug!("Found matching asset for target {}: {}", target, asset.name);
                return Ok(Some(asset.browser_download_url.clone()));
            }
        }

        // Fallback: try to find by OS and architecture
        let (os, arch) = self.get_os_arch();
        for asset in assets {
            let name_lower = asset.name.to_lowercase();
            if name_lower.contains(os) && name_lower.contains(arch) {
                tracing::debug!("Found matching asset for {}-{}: {}", os, arch, asset.name);
                return Ok(Some(asset.browser_download_url.clone()));
            }
        }

        tracing::warn!(
            "No matching asset found for platform {} ({}-{}). Available assets: {:?}",
            target,
            os,
            arch,
            assets.iter().map(|a| &a.name).collect::<Vec<_>>()
        );

        Ok(None)
    }

    /// Get the target triple for the current platform
    fn get_target_triple(&self) -> String {
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "x86_64-unknown-linux-gnu".to_string();

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "aarch64-unknown-linux-gnu".to_string();

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "x86_64-apple-darwin".to_string();

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "aarch64-apple-darwin".to_string();

        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "x86_64-pc-windows-msvc".to_string();

        #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
        return "aarch64-pc-windows-msvc".to_string();

        #[allow(unreachable_code)]
        "unknown".to_string()
    }

    /// Get OS and architecture as separate strings
    fn get_os_arch(&self) -> (&'static str, &'static str) {
        let os = if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "macos") {
            "macos"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        };

        let arch = if cfg!(target_arch = "x86_64") {
            "x86_64"
        } else if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "unknown"
        };

        (os, arch)
    }

    /// Parse a version string into a comparable tuple
    #[cfg_attr(not(test), allow(dead_code))]
    fn parse_version(&self, version: &str) -> Result<(u32, u32, u32)> {
        let version = version.trim_start_matches('v');
        let parts: Vec<&str> = version.split('.').collect();

        if parts.len() < 3 {
            anyhow::bail!("Invalid version format: {}", version);
        }

        let major = parts[0].parse::<u32>().context("Invalid major version")?;
        let minor = parts[1].parse::<u32>().context("Invalid minor version")?;
        let patch = parts[2].parse::<u32>().context("Invalid patch version")?;

        Ok((major, minor, patch))
    }

    /// Check if we should perform an update check based on frequency
    fn should_check(&self) -> Result<bool> {
        use super::config::UpdateFrequency;

        match self.config.frequency {
            UpdateFrequency::Never => return Ok(false),
            UpdateFrequency::Always => return Ok(true),
            _ => {}
        }

        // Load last check time
        let last_check = match self.load_last_check_time() {
            Ok(time) => time,
            Err(_) => return Ok(true), // No previous check, so check now
        };

        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(last_check);

        let should_check = match self.config.frequency {
            UpdateFrequency::Daily => duration.num_hours() >= 24,
            UpdateFrequency::Weekly => duration.num_days() >= 7,
            _ => true,
        };

        Ok(should_check)
    }

    /// Load the last check time from cache
    fn load_last_check_time(&self) -> Result<chrono::DateTime<chrono::Utc>> {
        let status = self.load_cached_status()?;
        status
            .last_checked
            .context("No last check time in cached status")
    }

    /// Load cached update status
    fn load_cached_status(&self) -> Result<UpdateStatus> {
        let content = std::fs::read_to_string(&self.last_check_file)
            .context("Failed to read cached status")?;
        let status: UpdateStatus =
            serde_json::from_str(&content).context("Failed to parse cached status")?;
        Ok(status)
    }

    /// Save update status to cache
    fn save_status(&self, status: &UpdateStatus) -> Result<()> {
        self.config.ensure_directories()?;
        let content = serde_json::to_string_pretty(status)?;
        std::fs::write(&self.last_check_file, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version() {
        let config = UpdateConfig::default();
        let checker = UpdateChecker::new(config).unwrap();

        assert_eq!(checker.parse_version("1.2.3").unwrap(), (1, 2, 3));
        assert_eq!(checker.parse_version("v1.2.3").unwrap(), (1, 2, 3));
        assert_eq!(checker.parse_version("0.33.1").unwrap(), (0, 33, 1));
    }

    #[test]
    fn test_version_comparison() {
        let config = UpdateConfig::default();
        let checker = UpdateChecker::new(config).unwrap();

        let v1 = checker.parse_version("0.33.1").unwrap();
        let v2 = checker.parse_version("0.34.0").unwrap();
        assert!(v2 > v1);

        let v3 = checker.parse_version("1.0.0").unwrap();
        assert!(v3 > v2);
    }

    #[test]
    fn test_get_target_triple() {
        let config = UpdateConfig::default();
        let checker = UpdateChecker::new(config).unwrap();
        let target = checker.get_target_triple();
        assert!(!target.is_empty());
        assert_ne!(target, "unknown");
    }
}
