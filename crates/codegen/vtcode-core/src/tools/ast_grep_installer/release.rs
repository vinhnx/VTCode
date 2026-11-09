use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::time::Duration;
use vtcode_commons::http::send_with_retry;
use vtcode_commons::retry::RetryPolicy;
use vtcode_commons::utils::calculate_sha256;

const AST_GREP_RELEASE_API: &str = "https://api.github.com/repos/ast-grep/ast-grep/releases/latest";

/// Retry budget for installer fetches: a single GitHub flake must not fail the
/// install (failures arm a 24h cooldown), but the install must not hang either:
/// 3 attempts over ~3s of jittered backoff, then the existing error path.
fn installer_retry_policy() -> RetryPolicy {
    RetryPolicy::from_retries(2, Duration::from_secs(1), Duration::from_secs(30), 2.0)
        .with_jitter(RetryPolicy::DEFAULT_JITTER)
}

#[derive(Debug, Deserialize)]
pub(super) struct ReleaseInfo {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReleaseAsset {
    pub(super) name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlatformAssetSpec {
    candidate_triples: &'static [&'static str],
    archive_exts: &'static [&'static str],
}

#[derive(Debug, Clone)]
pub(super) struct SelectedAsset {
    pub(super) tag_name: String,
    pub(super) asset: ReleaseAsset,
}

pub(super) async fn fetch_latest_release(client: &reqwest::Client) -> Result<ReleaseInfo> {
    fetch_latest_release_from(client, AST_GREP_RELEASE_API).await
}

pub(super) async fn fetch_latest_release_from(client: &reqwest::Client, url: &str) -> Result<ReleaseInfo> {
    send_with_retry(&installer_retry_policy(), || client.get(url).send())
        .await
        .context("Failed to fetch ast-grep release metadata")?
        .error_for_status()
        .context("ast-grep release endpoint returned non-success status")?
        .json::<ReleaseInfo>()
        .await
        .context("Failed to parse ast-grep release metadata")
}

/// Actionable manual install steps shown whenever automatic installation fails.
pub(super) fn manual_install_guide() -> &'static str {
    "Install ast-grep manually and rerun `vtcode dependencies status ast-grep`:\n\
     - macOS: brew install ast-grep\n\
     - Any OS with Rust: cargo install ast-grep --locked\n\
     - Or download app-<target>.zip from https://github.com/ast-grep/ast-grep/releases/latest, \
     unpack the ast-grep binary into ~/.local/bin (or set VTCODE_AST_GREP_BIN to its path)"
}

pub(super) fn current_platform_asset_spec() -> Result<PlatformAssetSpec> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-apple-darwin"],
            archive_exts: &[".zip", ".tar.gz"],
        }),
        ("macos", "aarch64") => Ok(PlatformAssetSpec {
            candidate_triples: &["aarch64-apple-darwin"],
            archive_exts: &[".zip", ".tar.gz"],
        }),
        ("linux", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        }),
        ("linux", "aarch64") => Ok(PlatformAssetSpec {
            candidate_triples: &["aarch64-unknown-linux-musl", "aarch64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        }),
        ("windows", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-pc-windows-msvc"],
            archive_exts: &[".zip"],
        }),
        ("windows", "aarch64") => Ok(PlatformAssetSpec {
            candidate_triples: &["aarch64-pc-windows-msvc"],
            archive_exts: &[".zip"],
        }),
        _ => bail!(
            "Unsupported platform for VT Code-managed ast-grep install ({} {}).\n{}",
            std::env::consts::OS,
            std::env::consts::ARCH,
            manual_install_guide()
        ),
    }
}

pub(super) fn select_release_asset(release: &ReleaseInfo, platform: &PlatformAssetSpec) -> Result<SelectedAsset> {
    for triple in platform.candidate_triples {
        if let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset_matches_target(&asset.name, triple, platform.archive_exts))
        {
            return Ok(SelectedAsset {
                tag_name: release.tag_name.clone(),
                asset: asset.clone(),
            });
        }
    }

    let mut available: Vec<&str> = release.assets.iter().map(|asset| asset.name.as_str()).collect();
    available.sort_unstable();
    const MAX_LISTED: usize = 12;
    let listed = if available.len() > MAX_LISTED {
        format!("{} (and {} more)", available[..MAX_LISTED].join(", "), available.len() - MAX_LISTED)
    } else {
        available.join(", ")
    };
    bail!(
        "No ast-grep release asset matched the current platform ({}) in {} (available: {}).\n{}",
        platform.candidate_triples.join(", "),
        release.tag_name,
        if listed.is_empty() { "none".to_string() } else { listed },
        manual_install_guide()
    )
}

pub(super) fn asset_matches_target(asset_name: &str, triple: &str, archive_exts: &[&str]) -> bool {
    let name = asset_name.to_ascii_lowercase();
    name.contains(triple)
        && archive_exts.iter().any(|archive_ext| name.ends_with(archive_ext))
        && !name.ends_with(".sha256")
        && !name.contains("checksum")
        && !name.ends_with(".sig")
}

pub(super) async fn download_release_asset(client: &reqwest::Client, asset: &ReleaseAsset) -> Result<Vec<u8>> {
    send_with_retry(&installer_retry_policy(), || client.get(&asset.browser_download_url).send())
        .await
        .with_context(|| format!("Failed to download {}", asset.name))?
        .error_for_status()
        .with_context(|| format!("Download failed for {}", asset.name))?
        .bytes()
        .await
        .with_context(|| format!("Failed to read {}", asset.name))
        .map(|bytes| bytes.to_vec())
}

pub(super) async fn verify_checksum_if_available(
    client: &reqwest::Client,
    release: &ReleaseInfo,
    selected: &SelectedAsset,
    archive_bytes: &[u8],
) -> Result<Option<String>> {
    let Some(checksum_asset) = find_checksum_asset(release, &selected.asset.name) else {
        return Ok(Some(format!(
            "Checksum metadata was not published for {}. Continuing without checksum verification.",
            selected.asset.name
        )));
    };

    let checksum_response =
        match send_with_retry(&installer_retry_policy(), || client.get(&checksum_asset.browser_download_url).send())
            .await
        {
            Ok(response) => response,
            Err(err) => {
                return Ok(Some(format!(
                    "Checksum metadata for {} was unavailable ({}). Continuing without checksum verification.",
                    selected.asset.name, err
                )));
            }
        };
    let checksum_response = match checksum_response.error_for_status() {
        Ok(response) => response,
        Err(err) => {
            return Ok(Some(format!(
                "Checksum metadata for {} was unavailable ({}). Continuing without checksum verification.",
                selected.asset.name, err
            )));
        }
    };

    let checksum_text = match checksum_response
        .text()
        .await
        .with_context(|| format!("Failed to read checksum metadata {}", checksum_asset.name))
    {
        Ok(text) => text,
        Err(err) => {
            return Ok(Some(format!(
                "Checksum metadata for {} was unavailable ({}). Continuing without checksum verification.",
                selected.asset.name, err
            )));
        }
    };

    let Some(expected_checksum) = parse_expected_checksum(&checksum_text, &checksum_asset.name, &selected.asset.name)
    else {
        return Ok(Some(format!(
            "Checksum metadata for {} did not contain an entry for {}. Continuing without checksum verification.",
            checksum_asset.name, selected.asset.name
        )));
    };

    let actual_checksum = sha256_hex(archive_bytes);
    if actual_checksum != expected_checksum {
        bail!(
            "Checksum mismatch for {} (expected {}, got {})",
            selected.asset.name,
            expected_checksum,
            actual_checksum
        );
    }

    Ok(None)
}

fn find_checksum_asset<'a>(release: &'a ReleaseInfo, archive_name: &str) -> Option<&'a ReleaseAsset> {
    let archive_sha_name = format!("{archive_name}.sha256").to_ascii_lowercase();
    release.assets.iter().find(|asset| {
        let name = asset.name.to_ascii_lowercase();
        name == archive_sha_name || name == "checksums.txt" || name == "sha256sums.txt"
    })
}

pub(super) fn parse_expected_checksum(
    checksum_text: &str,
    checksum_asset_name: &str,
    archive_name: &str,
) -> Option<String> {
    if checksum_asset_name.ends_with(".sha256") {
        return checksum_text.split_whitespace().next().map(|value| value.to_string());
    }

    checksum_text
        .lines()
        .find(|line| line.contains(archive_name))
        .and_then(|line| line.split_whitespace().next())
        .map(|value| value.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    calculate_sha256(bytes)
}

#[cfg(test)]
mod tests {
    use super::{
        PlatformAssetSpec, ReleaseAsset, ReleaseInfo, asset_matches_target, fetch_latest_release_from,
        manual_install_guide, parse_expected_checksum, select_release_asset,
    };

    fn real_release_0_51_1() -> ReleaseInfo {
        ReleaseInfo {
            tag_name: "0.51.1".to_string(),
            assets: [
                "app-aarch64-apple-darwin.zip",
                "app-aarch64-pc-windows-msvc.zip",
                "app-aarch64-unknown-linux-gnu.zip",
                "app-i686-pc-windows-msvc.zip",
                "app-x86_64-apple-darwin.zip",
                "app-x86_64-pc-windows-msvc.zip",
                "app-x86_64-unknown-linux-gnu.zip",
            ]
            .into_iter()
            .map(|name| ReleaseAsset {
                name: name.to_string(),
                browser_download_url: format!("https://example.com/{name}"),
            })
            .collect(),
        }
    }

    #[test]
    fn release_asset_selection_uses_first_supported_triple() {
        let release = ReleaseInfo {
            tag_name: "v0.40.0".to_string(),
            assets: vec![
                ReleaseAsset {
                    name: "app-x86_64-unknown-linux-gnu.tar.gz".to_string(),
                    browser_download_url: "https://example.com/gnu".to_string(),
                },
                ReleaseAsset {
                    name: "app-x86_64-unknown-linux-musl.tar.gz".to_string(),
                    browser_download_url: "https://example.com/musl".to_string(),
                },
            ],
        };
        let platform = PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        };

        let selected = select_release_asset(&release, &platform).expect("selected asset");
        assert_eq!(selected.asset.browser_download_url, "https://example.com/musl");
    }

    #[test]
    fn release_asset_selection_accepts_zip_assets() {
        let release = ReleaseInfo {
            tag_name: "v0.42.1".to_string(),
            assets: vec![ReleaseAsset {
                name: "app-aarch64-apple-darwin.zip".to_string(),
                browser_download_url: "https://example.com/macos".to_string(),
            }],
        };
        let platform = PlatformAssetSpec {
            candidate_triples: &["aarch64-apple-darwin"],
            archive_exts: &[".zip", ".tar.gz"],
        };

        let selected = select_release_asset(&release, &platform).expect("selected asset");
        assert_eq!(selected.asset.browser_download_url, "https://example.com/macos");
    }

    #[test]
    fn asset_matching_filters_checksum_files() {
        assert!(asset_matches_target(
            "app-aarch64-apple-darwin.tar.gz",
            "aarch64-apple-darwin",
            &[".zip", ".tar.gz"]
        ));
        assert!(asset_matches_target("app-aarch64-apple-darwin.zip", "aarch64-apple-darwin", &[".zip", ".tar.gz"]));
        assert!(!asset_matches_target(
            "app-aarch64-apple-darwin.tar.gz.sha256",
            "aarch64-apple-darwin",
            &[".zip", ".tar.gz"]
        ));
        assert!(!asset_matches_target("checksums.txt", "aarch64-apple-darwin", &[".zip", ".tar.gz"]));
    }

    #[test]
    fn release_asset_selection_matches_real_linux_gnu_zip() {
        // Regression test for issue #802: the real ast-grep 0.51.1 release
        // ships only app-*.zip assets (no musl, no tar.gz). The Linux x86_64
        // spec must still resolve via the gnu fallback triple.
        let release = real_release_0_51_1();
        let platform = PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        };
        let selected = select_release_asset(&release, &platform).expect("gnu fallback must match");
        assert!(selected.asset.name.contains("x86_64-unknown-linux-gnu"), "{}", selected.asset.name);
    }

    #[test]
    fn release_asset_selection_matches_windows_aarch64() {
        let release = real_release_0_51_1();
        let platform = PlatformAssetSpec {
            candidate_triples: &["aarch64-pc-windows-msvc"],
            archive_exts: &[".zip"],
        };
        let selected = select_release_asset(&release, &platform).expect("win aarch64 must match");
        assert_eq!(selected.asset.name, "app-aarch64-pc-windows-msvc.zip");
    }

    #[test]
    fn asset_mismatch_error_lists_assets_and_manual_guide() {
        let release = ReleaseInfo {
            tag_name: "v9.9.9".to_string(),
            assets: vec![ReleaseAsset {
                name: "app-x86_64-apple-darwin.zip".to_string(),
                browser_download_url: "https://example.com/macos".to_string(),
            }],
        };
        let platform = PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        };
        let err = select_release_asset(&release, &platform).expect_err("must fail without a linux asset");
        let message = format!("{err:#}");
        assert!(message.contains("No ast-grep release asset matched"), "{message}");
        assert!(message.contains("app-x86_64-apple-darwin.zip"), "{message}");
        assert!(message.contains(manual_install_guide()), "{message}");
    }

    #[test]
    fn checksum_parser_supports_sha256_sidecars() {
        let parsed = parse_expected_checksum("abc123  app.tar.gz", "app.tar.gz.sha256", "app.tar.gz");
        assert_eq!(parsed.as_deref(), Some("abc123"));
    }

    #[test]
    fn checksum_parser_supports_checksums_txt() {
        let parsed = parse_expected_checksum("abc123  app.tar.gz\nfff999 other.tar.gz", "checksums.txt", "app.tar.gz");
        assert_eq!(parsed.as_deref(), Some("abc123"));
    }

    #[test]
    fn asset_mismatch_error_truncates_long_asset_lists() {
        let release_with = |count: usize| ReleaseInfo {
            tag_name: "v9.9.9".to_string(),
            assets: (0..count)
                .map(|index| ReleaseAsset {
                    name: format!("app-unknown-platform-{index:02}.zip"),
                    browser_download_url: format!("https://example.com/{index}"),
                })
                .collect(),
        };
        let platform = PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-gnu"],
            archive_exts: &[".zip", ".tar.gz"],
        };

        let full = select_release_asset(&release_with(12), &platform).expect_err("must fail without a match");
        let full_message = format!("{full:#}");
        assert!(!full_message.contains("(and "), "{full_message}");

        let truncated = select_release_asset(&release_with(13), &platform).expect_err("must fail without a match");
        let truncated_message = format!("{truncated:#}");
        assert!(truncated_message.contains("(and 1 more)"), "{truncated_message}");
    }

    /// Start a wiremock server, or skip when the sandbox forbids binding.
    /// Mirrors the `bind_fixture_listener` precedent in `vtcode-commons`.
    async fn start_mock_server_or_skip() -> Option<wiremock::MockServer> {
        match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(probe) => {
                drop(probe);
                Some(wiremock::MockServer::start().await)
            }
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => None,
            Err(err) => panic!("mock server probe failed: {err}"),
        }
    }

    #[tokio::test]
    async fn fetch_recovers_from_a_transient_503() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        let Some(server) = start_mock_server_or_skip().await else {
            return;
        };
        // First-mounted mock wins while it matches: the one-shot 503 answers
        // the first attempt, then the installer's retry transparently recovers
        // instead of failing the install into the 24h cooldown.
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "tag_name": "1.2.3",
                "assets": []
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = vtcode_commons::http::create_download_client("vtcode-test");
        let url = format!("{}/releases/latest", server.uri());
        let release = fetch_latest_release_from(&client, &url)
            .await
            .expect("transient 503 must be retried");
        assert_eq!(release.tag_name, "1.2.3");
    }

    #[test]
    fn manual_guide_covers_fallback_methods() {
        let guide = manual_install_guide();
        // "manually" is the marker the install-cache dedup checks for.
        assert!(guide.contains("manually"), "{guide}");
        assert!(guide.contains("brew install ast-grep"), "{guide}");
        assert!(guide.contains("cargo install ast-grep"), "{guide}");
        assert!(guide.contains("ast-grep/ast-grep/releases"), "{guide}");
        assert!(guide.contains("VTCODE_AST_GREP_BIN"), "{guide}");
    }
}
