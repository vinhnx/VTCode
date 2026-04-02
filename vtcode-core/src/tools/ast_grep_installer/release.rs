use anyhow::{Context, Result, bail};
use serde::Deserialize;
use vtcode_commons::utils::calculate_sha256;

const AST_GREP_RELEASE_API: &str = "https://api.github.com/repos/ast-grep/ast-grep/releases/latest";

#[derive(Debug, Deserialize)]
pub(super) struct ReleaseInfo {
    pub(super) tag_name: String,
    pub(super) assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ReleaseAsset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PlatformAssetSpec {
    pub(super) candidate_triples: &'static [&'static str],
    pub(super) archive_ext: &'static str,
}

#[derive(Debug, Clone)]
pub(super) struct SelectedAsset {
    pub(super) tag_name: String,
    pub(super) asset: ReleaseAsset,
}

pub(super) async fn fetch_latest_release(client: &reqwest::Client) -> Result<ReleaseInfo> {
    client
        .get(AST_GREP_RELEASE_API)
        .send()
        .await
        .context("Failed to fetch ast-grep release metadata")?
        .error_for_status()
        .context("ast-grep release endpoint returned non-success status")?
        .json::<ReleaseInfo>()
        .await
        .context("Failed to parse ast-grep release metadata")
}

pub(super) fn current_platform_asset_spec() -> Result<PlatformAssetSpec> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-apple-darwin"],
            archive_ext: ".tar.gz",
        }),
        ("macos", "aarch64") => Ok(PlatformAssetSpec {
            candidate_triples: &["aarch64-apple-darwin"],
            archive_ext: ".tar.gz",
        }),
        ("linux", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-unknown-linux-musl", "x86_64-unknown-linux-gnu"],
            archive_ext: ".tar.gz",
        }),
        ("linux", "aarch64") => Ok(PlatformAssetSpec {
            candidate_triples: &["aarch64-unknown-linux-musl", "aarch64-unknown-linux-gnu"],
            archive_ext: ".tar.gz",
        }),
        ("windows", "x86_64") => Ok(PlatformAssetSpec {
            candidate_triples: &["x86_64-pc-windows-msvc"],
            archive_ext: ".zip",
        }),
        _ => bail!("Unsupported platform for VT Code-managed ast-grep install"),
    }
}

pub(super) fn select_release_asset(
    release: &ReleaseInfo,
    platform: &PlatformAssetSpec,
) -> Result<SelectedAsset> {
    for triple in platform.candidate_triples {
        if let Some(asset) = release
            .assets
            .iter()
            .find(|asset| asset_matches_target(&asset.name, triple, platform.archive_ext))
        {
            return Ok(SelectedAsset {
                tag_name: release.tag_name.clone(),
                asset: asset.clone(),
            });
        }
    }

    bail!(
        "No ast-grep release asset matched the current platform ({})",
        platform.candidate_triples.join(", ")
    )
}

pub(super) fn asset_matches_target(asset_name: &str, triple: &str, archive_ext: &str) -> bool {
    let name = asset_name.to_ascii_lowercase();
    name.contains(triple)
        && name.ends_with(archive_ext)
        && !name.ends_with(".sha256")
        && !name.contains("checksum")
        && !name.ends_with(".sig")
}

pub(super) async fn download_release_asset(
    client: &reqwest::Client,
    asset: &ReleaseAsset,
) -> Result<Vec<u8>> {
    client
        .get(&asset.browser_download_url)
        .send()
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

    let checksum_response = match client
        .get(&checksum_asset.browser_download_url)
        .send()
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

    let Some(expected_checksum) =
        parse_expected_checksum(&checksum_text, &checksum_asset.name, &selected.asset.name)
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

fn find_checksum_asset<'a>(
    release: &'a ReleaseInfo,
    archive_name: &str,
) -> Option<&'a ReleaseAsset> {
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
        return checksum_text
            .split_whitespace()
            .next()
            .map(|value| value.to_string());
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
        PlatformAssetSpec, ReleaseAsset, ReleaseInfo, asset_matches_target,
        parse_expected_checksum, select_release_asset,
    };

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
            archive_ext: ".tar.gz",
        };

        let selected = select_release_asset(&release, &platform).expect("selected asset");
        assert_eq!(
            selected.asset.browser_download_url,
            "https://example.com/musl"
        );
    }

    #[test]
    fn asset_matching_filters_checksum_files() {
        assert!(asset_matches_target(
            "app-aarch64-apple-darwin.tar.gz",
            "aarch64-apple-darwin",
            ".tar.gz"
        ));
        assert!(!asset_matches_target(
            "app-aarch64-apple-darwin.tar.gz.sha256",
            "aarch64-apple-darwin",
            ".tar.gz"
        ));
        assert!(!asset_matches_target(
            "checksums.txt",
            "aarch64-apple-darwin",
            ".tar.gz"
        ));
    }

    #[test]
    fn checksum_parser_supports_sha256_sidecars() {
        let parsed =
            parse_expected_checksum("abc123  app.tar.gz", "app.tar.gz.sha256", "app.tar.gz");
        assert_eq!(parsed.as_deref(), Some("abc123"));
    }

    #[test]
    fn checksum_parser_supports_checksums_txt() {
        let parsed = parse_expected_checksum(
            "abc123  app.tar.gz\nfff999 other.tar.gz",
            "checksums.txt",
            "app.tar.gz",
        );
        assert_eq!(parsed.as_deref(), Some("abc123"));
    }
}
