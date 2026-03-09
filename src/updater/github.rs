use anyhow::{Context, Result};
use semver::Version;
use std::time::Duration;

use super::Updater;
use super::install_source::get_target_triple;
use super::types::{UpdateInfo, VersionInfo};

pub(super) const REPO_OWNER: &str = "vinhnx";
pub(super) const REPO_NAME: &str = "vtcode";
const REPO_SLUG: &str = "vinhnx/vtcode";

pub(super) fn release_url(version: &Version) -> String {
    format!("https://github.com/{REPO_SLUG}/releases/tag/v{version}")
}

pub(super) async fn fetch_latest_release(updater: &Updater) -> Result<Option<UpdateInfo>> {
    let latest = fetch_latest_release_info().await?;
    if latest.version > updater.current_version {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

async fn fetch_latest_release_info() -> Result<UpdateInfo> {
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

    Ok(UpdateInfo {
        download_url: download_url(&version)?,
        version,
        tag: tag_name.to_string(),
        release_notes: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("See release notes on GitHub")
            .to_string(),
    })
}

fn download_url(version: &Version) -> Result<String> {
    let target = get_target_triple().context("Unsupported platform for auto-update")?;
    let file_ext = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };

    Ok(format!(
        "https://github.com/{REPO_SLUG}/releases/download/v{version}/vtcode-v{version}-{target}.{file_ext}"
    ))
}

pub(super) async fn list_versions(limit: usize) -> Result<Vec<VersionInfo>> {
    let url = format!(
        "https://api.github.com/repos/{REPO_SLUG}/releases?per_page={}",
        limit
    );

    let client = reqwest::Client::builder()
        .user_agent("vtcode-updater")
        .build()
        .context("Failed to create HTTP client")?;

    let response = client
        .get(&url)
        .timeout(Duration::from_secs(8))
        .send()
        .await
        .context("Failed to fetch releases from GitHub")?
        .error_for_status()
        .context("GitHub API returned non-success status")?;

    let json = response
        .json::<serde_json::Value>()
        .await
        .context("Failed to parse GitHub API response")?;

    let versions = json
        .as_array()
        .context("Expected array of releases")?
        .iter()
        .filter_map(|release| {
            let tag_name = release.get("tag_name")?.as_str()?;
            let version_str = tag_name.trim_start_matches('v');
            let version = Version::parse(version_str).ok()?;
            let is_prerelease = release
                .get("prerelease")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let published_at = release
                .get("published_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());

            Some(VersionInfo {
                version,
                tag: tag_name.to_string(),
                is_prerelease,
                published_at,
            })
        })
        .collect();

    Ok(versions)
}
