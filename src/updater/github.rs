use anyhow::{Context, Result};
use semver::Version;
use std::time::Duration;
use vtcode_config::update::ReleaseChannel;

use super::Updater;
use super::types::{UpdateInfo, VersionInfo};

pub(super) const REPO_OWNER: &str = "vinhnx";
pub(super) const REPO_NAME: &str = "vtcode";
const REPO_SLUG: &str = "vinhnx/vtcode";

fn github_client() -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().user_agent("vtcode-updater");

    // Authenticate with GitHub API when a token is available.
    // Unauthenticated requests are limited to 60/hour per IP; authenticated
    // requests get 5,000/hour. Check the two conventional env var names.
    if let Some(token) = github_token() {
        let mut headers = reqwest::header::HeaderMap::new();
        let mut value = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .context("Invalid GITHUB_TOKEN value")?;
        value.set_sensitive(true);
        headers.insert(reqwest::header::AUTHORIZATION, value);
        builder = builder.default_headers(headers);
    }

    builder.build().context("Failed to create HTTP client")
}

fn github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .ok()
        .or_else(|| std::env::var("GH_TOKEN").ok())
        .filter(|token| !token.trim().is_empty())
}

/// Strip the `Authorization` header and rebuild the client for unauthenticated
/// requests.  Used as a fallback when a configured token is rejected.
fn unauthenticated_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("vtcode-updater")
        .build()
        .context("Failed to create HTTP client")
}

/// Fetch JSON from `url` using the configured (possibly authenticated) client.
/// If the request fails with a 401, retry without authentication -- public
/// repos work fine unauthenticated.
async fn try_fetch_json_with_auth_fallback(
    url: &str,
    timeout: Duration,
) -> Result<serde_json::Value> {
    let response = github_client()?
        .get(url)
        .timeout(timeout)
        .send()
        .await
        .context("Failed to fetch from GitHub API");

    let response = match response {
        Ok(response)
            if response.status() == reqwest::StatusCode::UNAUTHORIZED
                && github_token().is_some() =>
        {
            unauthenticated_client()?
                .get(url)
                .timeout(timeout)
                .send()
                .await
                .context("Failed to fetch from GitHub API")?
        }
        Ok(response) => response,
        Err(err) => return Err(err),
    };

    response
        .error_for_status()
        .context("GitHub API returned non-success status")?
        .json::<serde_json::Value>()
        .await
        .context("Failed to parse GitHub API response")
}

/// Clamp the timeout to at least 1 second to prevent a zero-duration timeout
/// from immediately failing every request.
fn effective_timeout(timeout_secs: u64) -> Duration {
    Duration::from_secs(timeout_secs.max(1))
}

pub(super) fn release_url(version: &Version) -> String {
    format!("https://github.com/{REPO_SLUG}/releases/tag/v{version}")
}

pub(super) async fn fetch_latest_release(
    updater: &Updater,
    timeout_secs: u64,
    channel: &ReleaseChannel,
) -> Result<Option<UpdateInfo>> {
    let latest = match channel {
        ReleaseChannel::Stable => fetch_latest_release_info(timeout_secs).await?,
        ReleaseChannel::Beta | ReleaseChannel::Nightly => {
            fetch_latest_prerelease_info(timeout_secs, channel).await?
        }
    };
    if latest.version > updater.current_version {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

pub(super) async fn fetch_latest_release_info(timeout_secs: u64) -> Result<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases/latest");
    let timeout = effective_timeout(timeout_secs);
    let json = try_fetch_json_with_auth_fallback(&url, timeout).await?;

    let tag_name = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .context("Missing tag_name in GitHub response")?;

    let version_str = tag_name.trim_start_matches('v');
    let version = Version::parse(version_str)
        .with_context(|| format!("Invalid version in GitHub release: {}", tag_name))?;

    Ok(UpdateInfo {
        version,
        release_notes: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("See release notes on GitHub")
            .to_string(),
    })
}

/// Fetch the latest pre-release from GitHub for beta/nightly channels.
///
/// Uses `/releases?per_page=20` and filters by channel:
/// - **Beta**: any release where `prerelease == true`
/// - **Nightly**: releases whose tag contains "nightly" or whose semver
///   pre-release identifier starts with "nightly"
///
/// Returns the highest-versioned match.
async fn fetch_latest_prerelease_info(
    timeout_secs: u64,
    channel: &ReleaseChannel,
) -> Result<UpdateInfo> {
    let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases?per_page=20");
    let timeout = effective_timeout(timeout_secs);

    let json = try_fetch_json_with_auth_fallback(&url, timeout).await?;
    let releases = json.as_array().context("Expected array of releases")?;

    let mut best: Option<(Version, String)> = None;

    for release in releases {
        let tag_name = match release.get("tag_name").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let version_str = tag_name.trim_start_matches('v');
        let version = match Version::parse(version_str) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let is_prerelease = release
            .get("prerelease")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let matches_channel = match channel {
            ReleaseChannel::Stable => !is_prerelease,
            ReleaseChannel::Beta => is_prerelease,
            ReleaseChannel::Nightly => {
                let tag_lower = tag_name.to_ascii_lowercase();
                tag_lower.contains("nightly") || version.pre.as_str().starts_with("nightly")
            }
        };

        if !matches_channel {
            continue;
        }

        if best.as_ref().is_none_or(|(v, _)| version > *v) {
            let notes = release
                .get("body")
                .and_then(|v| v.as_str())
                .unwrap_or("See release notes on GitHub")
                .to_string();
            best = Some((version, notes));
        }
    }

    let (version, release_notes) = best.context(format!(
        "No {channel} releases found on GitHub for {REPO_SLUG}"
    ))?;

    Ok(UpdateInfo {
        version,
        release_notes,
    })
}

pub(super) async fn list_versions(limit: usize, timeout_secs: u64) -> Result<Vec<VersionInfo>> {
    let url = format!(
        "https://api.github.com/repos/{REPO_SLUG}/releases?per_page={}",
        limit
    );
    let timeout = effective_timeout(timeout_secs);

    let json = try_fetch_json_with_auth_fallback(&url, timeout).await?;
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
