use anyhow::{Context, Result, bail};
use semver::Version;
use serde::Deserialize;
use std::time::Duration;
use url::Url;
use vtcode_commons::http::send_with_retry;
use vtcode_commons::retry::RetryPolicy;
use vtcode_config::update::ReleaseChannel;

use super::Updater;
use super::archive::ArchiveKind;
use super::types::{UpdateInfo, VersionInfo};

const REPO_SLUG: &str = "vinhnx/vtcode";
const GITHUB_HOST: &str = "github.com";
const RELEASE_ASSET_PATH_PREFIX: &str = "/vinhnx/vtcode/releases/download/";

/// Retry budget for updater fetches.
///
/// GitHub serves every release asset as its own CDN object and answers
/// transient 502/504s that routinely outlive a few seconds. Measured
/// 2026-11-09: every asset of release 0.181.7 — plus assets of older
/// releases — 504'd continuously for ~2.5 minutes. The previous budget
/// (3 attempts over ~3s) therefore failed an update that a backoff schedule
/// spanning a few minutes survives, and the failure is expensive: the
/// archive has already been downloaded. Requests carry their own
/// `.timeout(...)`, so the extra attempts add only the backoff sleeps
/// (~3 minutes worst case), never an unbounded wait.
///
/// Shared with `download.rs`.
pub(super) fn updater_retry_policy() -> RetryPolicy {
    RetryPolicy::from_retries(8, Duration::from_secs(1), Duration::from_secs(45), 2.0)
        .with_jitter(RetryPolicy::DEFAULT_JITTER)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct ReleaseAsset {
    pub(super) name: String,
    #[serde(rename = "browser_download_url")]
    pub(super) download_url: String,
}

pub(super) fn select_archive_asset<'a>(
    assets: &'a [ReleaseAsset],
    target: &str,
) -> Result<(&'a ReleaseAsset, ArchiveKind)> {
    let target = target.to_ascii_lowercase();
    let is_windows = target.contains("-pc-windows-");
    let (suffix, kind) = if is_windows {
        (".zip", ArchiveKind::Zip)
    } else {
        (".tar.gz", ArchiveKind::TarGz)
    };
    let target_suffix = format!("{target}{suffix}");
    let candidates: Vec<_> = assets
        .iter()
        .filter(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.starts_with("vtcode-") && name.ends_with(&target_suffix)
        })
        .collect();

    match candidates.as_slice() {
        [asset] => Ok((asset, kind)),
        [] => bail!("no compatible archive found for target {target}"),
        _ => bail!("multiple compatible archives found for target {target}"),
    }
}

/// Validate the URL supplied by GitHub for a release asset before making a
/// request. Release metadata is remote input, so downloads must remain on the
/// expected GitHub release endpoint.
pub(super) fn validate_asset_download_url(raw_url: &str) -> Result<()> {
    let url = Url::parse(raw_url).context("invalid release asset URL")?;

    if url.scheme() != "https" {
        bail!("release asset URL must use HTTPS");
    }
    if url.host_str().is_none_or(|host| !host.eq_ignore_ascii_case(GITHUB_HOST)) {
        bail!("release asset URL must use github.com");
    }
    if !url.username().is_empty() || url.password().is_some() {
        bail!("release asset URL must not include credentials");
    }
    if url.port().is_some_and(|port| port != 443) {
        bail!("release asset URL must use the default HTTPS port");
    }
    if url.query().is_some() || url.fragment().is_some() {
        bail!("release asset URL must not include a query or fragment");
    }

    let path = url.path().to_ascii_lowercase();
    let Some(download_path) = path.strip_prefix(RELEASE_ASSET_PATH_PREFIX) else {
        bail!("release asset URL is outside the VT Code release path");
    };

    let mut path_segments = download_path.split('/');
    let has_release_tag = path_segments.next().is_some_and(|segment| !segment.is_empty());
    let has_asset_name = path_segments.next().is_some_and(|segment| !segment.is_empty());
    if !has_release_tag || !has_asset_name || path_segments.next().is_some() {
        bail!("release asset URL does not identify one release asset");
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct GithubRelease {
    pub(super) version: Version,
    pub(super) release_notes: String,
    pub(super) assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubReleaseJson {
    tag_name: String,
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

fn parse_release(value: &serde_json::Value) -> Result<GithubRelease> {
    let release: GithubReleaseJson =
        serde_json::from_value(value.clone()).context("Failed to parse GitHub release metadata")?;
    let version_str = release.tag_name.trim_start_matches('v');
    let version = Version::parse(version_str)
        .with_context(|| format!("Invalid version in GitHub release: {}", release.tag_name))?;
    Ok(GithubRelease {
        version,
        release_notes: release.body.unwrap_or_else(|| "See release notes on GitHub".to_string()),
        assets: release.assets,
    })
}

/// Connect-phase bound shared by the updater clients.
///
/// Requests carry their own `.timeout(...)` from `update.toml`, but reqwest
/// leaves the connect phase unbounded unless it is set, so a stalled handshake
/// could still hang the update check.
fn connect_timeout() -> Option<Duration> {
    vtcode_commons::http::http_timeouts().connect
}

fn github_client() -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().user_agent("vtcode-updater");
    if let Some(connect) = connect_timeout() {
        builder = builder.connect_timeout(connect);
    }

    // Authenticate with GitHub API when a token is available.
    // Unauthenticated requests are limited to 60/hour per IP; authenticated
    // requests get 5,000/hour. Check the two conventional env var names.
    if let Some(token) = github_token() {
        let mut headers = reqwest::header::HeaderMap::new();
        let mut value =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).context("Invalid GITHUB_TOKEN value")?;
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

/// Build a client without credentials for public asset downloads and API
/// fallback requests. Asset URLs come from remote release metadata and must
/// never receive the GitHub API token.
pub(super) fn unauthenticated_client() -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().user_agent("vtcode-updater");
    if let Some(connect) = connect_timeout() {
        builder = builder.connect_timeout(connect);
    }
    builder.build().context("Failed to create HTTP client")
}

/// Fetch JSON from `url` using the configured (possibly authenticated) client.
/// If the request fails with a 401, retry without authentication -- public
/// repos work fine unauthenticated.
async fn try_fetch_json_with_auth_fallback(url: &str, timeout: Duration) -> Result<serde_json::Value> {
    let client = github_client()?;
    let response = send_with_retry(&updater_retry_policy(), || client.get(url).timeout(timeout).send())
        .await
        .context("Failed to fetch from GitHub API");

    let response = match response {
        Ok(response) if response.status() == reqwest::StatusCode::UNAUTHORIZED && github_token().is_some() => {
            let fallback = unauthenticated_client()?;
            send_with_retry(&updater_retry_policy(), || fallback.get(url).timeout(timeout).send())
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
    format!("https://github.com/{REPO_SLUG}/releases/tag/{version}")
}

/// Fetch the latest release info for a given release channel.
///
/// Dispatches to the stable latest-release endpoint or the pre-release
/// listing endpoint depending on the channel.
pub(super) async fn fetch_latest_for_channel(timeout_secs: u64, channel: &ReleaseChannel) -> Result<UpdateInfo> {
    let release = fetch_latest_release_metadata(timeout_secs, channel).await?;
    Ok(UpdateInfo {
        version: release.version,
        release_notes: release.release_notes,
    })
}

pub(super) async fn fetch_latest_release_metadata(
    timeout_secs: u64,
    channel: &ReleaseChannel,
) -> Result<GithubRelease> {
    let timeout = effective_timeout(timeout_secs);
    match channel {
        ReleaseChannel::Stable | ReleaseChannel::Unknown => {
            let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases/latest");
            parse_release(&try_fetch_json_with_auth_fallback(&url, timeout).await?)
        }
        ReleaseChannel::Beta | ReleaseChannel::Nightly => {
            let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases?per_page=20");
            let json = try_fetch_json_with_auth_fallback(&url, timeout).await?;
            let releases = json.as_array().context("Expected array of releases")?;
            let mut best: Option<GithubRelease> = None;

            for value in releases {
                let release_json: GithubReleaseJson = match serde_json::from_value(value.clone()) {
                    Ok(release) => release,
                    Err(_) => continue,
                };
                let version = match Version::parse(release_json.tag_name.trim_start_matches('v')) {
                    Ok(version) => version,
                    Err(_) => continue,
                };
                let is_prerelease = value.get("prerelease").and_then(|value| value.as_bool()).unwrap_or(false);
                let matches_channel = match channel {
                    ReleaseChannel::Beta => is_prerelease,
                    ReleaseChannel::Nightly => {
                        let tag = release_json.tag_name.to_ascii_lowercase();
                        tag.contains("nightly") || version.pre.as_str().starts_with("nightly")
                    }
                    ReleaseChannel::Stable | ReleaseChannel::Unknown => !is_prerelease,
                };
                if !matches_channel || best.as_ref().is_some_and(|best| best.version >= version) {
                    continue;
                }
                best = Some(GithubRelease {
                    version,
                    release_notes: release_json.body.unwrap_or_else(|| "See release notes on GitHub".to_string()),
                    assets: release_json.assets,
                });
            }

            best.context(format!("No {channel} releases found on GitHub for {REPO_SLUG}"))
        }
    }
}

pub(super) async fn fetch_latest_release(
    updater: &Updater,
    timeout_secs: u64,
    channel: &ReleaseChannel,
) -> Result<Option<UpdateInfo>> {
    let latest = fetch_latest_for_channel(timeout_secs, channel).await?;
    if latest.version > updater.current_version {
        Ok(Some(latest))
    } else {
        Ok(None)
    }
}

pub(super) async fn fetch_latest_release_info(timeout_secs: u64) -> Result<UpdateInfo> {
    let release = fetch_latest_release_metadata(timeout_secs, &ReleaseChannel::Stable).await?;
    Ok(UpdateInfo {
        version: release.version,
        release_notes: release.release_notes,
    })
}

pub(super) async fn list_versions(limit: usize, timeout_secs: u64) -> Result<Vec<VersionInfo>> {
    let url = format!("https://api.github.com/repos/{REPO_SLUG}/releases?per_page={limit}");
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
            let is_prerelease = release.get("prerelease").and_then(|v| v.as_bool()).unwrap_or(false);
            let published_at = release.get("published_at").and_then(|v| v.as_str()).map(|s| s.to_string());

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

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            download_url: format!("https://example.test/{name}"),
        }
    }

    /// Total backoff the policy sleeps across all attempts after the first.
    fn total_backoff(policy: &RetryPolicy) -> Duration {
        (0..policy.max_attempts.saturating_sub(1))
            .map(|attempt| policy.delay_for_attempt(attempt))
            .sum()
    }

    /// GitHub's release-asset CDN 504'd continuously for ~2.5 minutes on
    /// 2026-11-09. The updater budget must span minutes, not seconds, or a
    /// transient CDN outage fails an update whose archive was already
    /// downloaded. Each attempt still carries its own request timeout, so the
    /// budget bounds only the sleeps.
    #[test]
    fn updater_retry_budget_survives_a_multi_minute_cdn_outage() {
        let policy = updater_retry_policy();

        assert!(policy.max_attempts >= 5, "attempts: {}", policy.max_attempts);
        assert!(total_backoff(&policy) >= Duration::from_secs(120), "backoff: {:?}", total_backoff(&policy));
    }

    #[test]
    fn selects_macos_tar_gz_for_exact_target() {
        let assets = vec![
            asset("checksums.txt"),
            asset("vtcode-1.0.0-x86_64-apple-darwin.sha256"),
            asset("vtcode-1.0.0-aarch64-apple-darwin.tar.gz"),
            asset("vtcode-1.0.0-x86_64-apple-darwin.tar.gz"),
        ];

        let (selected, kind) = select_archive_asset(&assets, "aarch64-apple-darwin").expect("asset");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(kind, ArchiveKind::TarGz);
    }

    #[test]
    fn accepts_github_release_asset_url() {
        let url =
            "https://github.com/vinhnx/VTCode/releases/download/0.150.0/vtcode-0.150.0-aarch64-apple-darwin.tar.gz";

        assert!(validate_asset_download_url(url).is_ok());
    }

    #[test]
    fn rejects_release_asset_urls_outside_expected_github_endpoint() {
        let urls = [
            "http://github.com/vinhnx/VTCode/releases/download/0.150.0/vtcode.tar.gz",
            "https://example.com/vinhnx/VTCode/releases/download/0.150.0/vtcode.tar.gz",
            "https://github.com/other/repo/releases/download/0.150.0/vtcode.tar.gz",
            "https://github.com/vinhnx/VTCode/releases/download/0.150.0/vtcode.tar.gz?token=secret",
            "https://github.com:444/vinhnx/VTCode/releases/download/0.150.0/vtcode.tar.gz",
        ];

        for url in urls {
            assert!(validate_asset_download_url(url).is_err(), "accepted unsafe URL: {url}");
        }
    }

    #[test]
    fn selects_linux_tar_gz_and_windows_zip() {
        let linux = vec![asset("vtcode-1.0.0-x86_64-unknown-linux-musl.tar.gz")];
        let windows = vec![asset("vtcode-1.0.0-x86_64-pc-windows-msvc.zip")];

        assert_eq!(select_archive_asset(&linux, "x86_64-unknown-linux-musl").expect("linux").1, ArchiveKind::TarGz);
        assert_eq!(select_archive_asset(&windows, "x86_64-pc-windows-msvc").expect("windows").1, ArchiveKind::Zip);
    }

    #[test]
    fn rejects_checksums_signatures_and_wrong_targets() {
        let assets = vec![
            asset("checksums.txt"),
            asset("vtcode-1.0.0-x86_64-pc-windows-msvc.sha256"),
            asset("vtcode-1.0.0-x86_64-pc-windows-msvc.zip.sig"),
            asset("other-1.0.0-aarch64-pc-windows-msvc.zip"),
        ];

        let error = select_archive_asset(&assets, "aarch64-pc-windows-msvc").expect_err("no archive");

        assert!(error.to_string().contains("no compatible archive"));
    }

    #[test]
    fn ignores_legacy_compat_assets_and_selects_real_archive() {
        // The legacy updater compatibility bridge publishes a raw executable
        // named `compat-vtcode-<v>-<target>.tar.gz.compat` (it sorts before the
        // normal archive so the broken v0.141.0-v0.141.4 `self_update` matcher
        // picks it). The v0.141.5+ matcher must IGNORE it: the name does not
        // `starts_with("vtcode-")` and does not `ends_with("{target}.tar.gz")`,
        // so exactly one candidate (the real archive) remains.
        let assets = vec![
            asset("checksums.txt"),
            asset("compat-vtcode-1.0.0-aarch64-apple-darwin.tar.gz.compat"),
            asset("vtcode-1.0.0-aarch64-apple-darwin.sha256"),
            asset("vtcode-1.0.0-aarch64-apple-darwin.tar.gz"),
        ];

        let (selected, kind) = select_archive_asset(&assets, "aarch64-apple-darwin").expect("asset");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");
        assert_eq!(kind, ArchiveKind::TarGz);
    }

    #[test]
    fn ignores_legacy_compat_assets_for_windows() {
        let assets = vec![
            asset("compat-vtcode-1.0.0-x86_64-pc-windows-msvc.tar.gz.compat"),
            asset("vtcode-1.0.0-x86_64-pc-windows-msvc.sha256"),
            asset("vtcode-1.0.0-x86_64-pc-windows-msvc.zip"),
        ];

        let (selected, kind) = select_archive_asset(&assets, "x86_64-pc-windows-msvc").expect("asset");

        assert_eq!(selected.name, "vtcode-1.0.0-x86_64-pc-windows-msvc.zip");
        assert_eq!(kind, ArchiveKind::Zip);
    }
}
