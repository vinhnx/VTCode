use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWriteExt, BufWriter};

use super::github::{self, ReleaseAsset};

pub(super) async fn download_asset<F>(
    asset: &ReleaseAsset,
    destination: &Path,
    timeout: Duration,
    show_progress: bool,
    mut on_progress: Option<&mut F>,
) -> Result<()>
where
    F: FnMut(u64, Option<u64>) + Send,
{
    let response = response_for_url(&asset.download_url, timeout)
        .await
        .with_context(|| format!("failed to download update asset {}", asset.name))?;
    let response = response
        .error_for_status()
        .with_context(|| format!("GitHub asset download returned an error for {}", asset.name))?;
    let total = response.content_length();
    let mut stream = response;
    // Batch streaming network chunks through a `BufWriter` so many small
    // `write_all` calls coalesce into a few large `spawn_blocking` writes
    // (see `tokio::fs` "Tuning your file IO"). The buffer flushes on `flush()`
    // below; each flush stays well under `File::set_max_buf_size` (2 MiB).
    let file = tokio::fs::File::create(destination)
        .await
        .with_context(|| format!("failed to create downloaded archive {}", destination.display()))?;
    let mut file = BufWriter::with_capacity(64 * 1024, file);
    let mut downloaded = 0_u64;
    let mut last_percent: Option<u8> = None;
    let mut last_report = std::time::Instant::now();

    while let Some(chunk) = stream
        .chunk()
        .await
        .with_context(|| format!("failed while downloading {}", asset.name))?
    {
        file.write_all(&chunk)
            .await
            .with_context(|| format!("failed to write downloaded archive {}", destination.display()))?;
        downloaded += chunk.len() as u64;
        if show_progress {
            print_progress(&asset.name, downloaded, total);
        }
        if let Some(progress) = on_progress.as_mut()
            && should_report_download(downloaded, total, &mut last_percent, &mut last_report)
        {
            progress(downloaded, total);
        }
    }
    file.flush().await.context("failed to flush downloaded archive")?;
    if show_progress {
        println!();
    }
    // Final report so callers see the completed byte count / 100%.
    if let Some(progress) = on_progress.as_mut() {
        progress(downloaded, total);
    }
    Ok(())
}

pub(super) async fn download_checksum(asset: &ReleaseAsset, timeout: Duration) -> Result<String> {
    let response = response_for_url(&asset.download_url, timeout)
        .await
        .with_context(|| format!("failed to download checksum asset {}", asset.name))?;
    response
        .error_for_status()
        .with_context(|| format!("checksum download returned an error for {}", asset.name))?
        .text()
        .await
        .with_context(|| format!("failed to read checksum metadata {}", asset.name))
}

async fn response_for_url(url: &str, timeout: Duration) -> Result<reqwest::Response> {
    github::validate_asset_download_url(url)?;

    github::unauthenticated_client()?
        .get(url)
        .timeout(timeout)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await
        .with_context(|| format!("failed to download update asset from {url}"))
}

pub(super) fn checksum_asset<'a>(assets: &'a [ReleaseAsset], archive_name: &str) -> Option<&'a ReleaseAsset> {
    let archive_name = archive_name.to_ascii_lowercase();
    // Modern sidecar: `archive.tar.gz.sha256` / `archive.zip.sha256`.
    let modern_sidecar = format!("{archive_name}.sha256");
    // Legacy extension-stripped sidecar: `vtcode-<v>-<target>.sha256`
    // (the `.tar.gz`/`.zip` suffix removed). This is the convention the
    // release pipeline publishes today, so prefer it over the aggregate file.
    let legacy_sidecar = legacy_checksum_name(&archive_name);

    assets
        .iter()
        .find(|asset| asset.name.to_ascii_lowercase() == modern_sidecar)
        .or_else(|| {
            let legacy = legacy_sidecar.as_deref();
            assets
                .iter()
                .find(|asset| legacy.is_some_and(|name| asset.name.to_ascii_lowercase() == name))
        })
        .or_else(|| {
            assets.iter().find(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name == "checksums.txt" || name == "sha256sums.txt"
            })
        })
}

/// Derive the legacy extension-stripped checksum sidecar name for an archive.
///
/// `vtcode-1.0.0-aarch64-apple-darwin.tar.gz` -> `vtcode-1.0.0-aarch64-apple-darwin.sha256`.
/// Returns `None` for archives that are neither `.tar.gz` nor `.zip`.
fn legacy_checksum_name(archive_name: &str) -> Option<String> {
    archive_name
        .strip_suffix(".tar.gz")
        .or_else(|| archive_name.strip_suffix(".zip"))
        .map(|stem| format!("{stem}.sha256"))
}

pub(super) fn parse_checksum_metadata(metadata: &str, archive_name: &str) -> Option<String> {
    let archive_name = archive_name.to_ascii_lowercase();
    metadata.lines().find_map(|line| {
        let tokens: Vec<_> = line.split_whitespace().collect();
        let digest = tokens.iter().find(|token| is_sha256(token))?;
        let matches_archive = tokens.iter().any(|token| {
            let filename = token.trim_start_matches('*').rsplit(['/', '\\']).next().unwrap_or_default();
            filename.eq_ignore_ascii_case(&archive_name)
        });
        if tokens.len() == 1 || matches_archive {
            Some(digest.to_ascii_lowercase())
        } else {
            None
        }
    })
}

fn verify_checksum(contents: &[u8], expected: &str) -> Result<()> {
    if !is_sha256(expected) {
        bail!("invalid SHA-256 checksum metadata");
    }
    let actual = digest_hex(contents);
    if actual != expected.to_ascii_lowercase() {
        bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

pub(super) fn verify_file_checksum(path: &Path, expected: &str) -> Result<()> {
    let contents =
        std::fs::read(path).with_context(|| format!("failed to read {} for checksum verification", path.display()))?;
    verify_checksum(&contents, expected)
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn digest_hex(contents: &[u8]) -> String {
    Sha256::digest(contents).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn print_progress(name: &str, downloaded: u64, total: Option<u64>) {
    let mut output = std::io::stdout().lock();
    match total {
        Some(total) if total > 0 => {
            let percent = downloaded.saturating_mul(100) / total;
            let _ = write!(output, "\rDownloading {name}: {percent}% ({downloaded}/{total} bytes)");
        }
        _ => {
            let _ = write!(output, "\rDownloading {name}: {downloaded} bytes");
        }
    }
    let _ = output.flush();
}

/// Throttle download progress callbacks so the UI is not flooded with one event
/// per network chunk. Reports at most every whole-percent change or 100 ms
/// (200 ms when no `Content-Length` is available).
fn should_report_download(
    downloaded: u64,
    total: Option<u64>,
    last_percent: &mut Option<u8>,
    last_report: &mut std::time::Instant,
) -> bool {
    const PERCENT_INTERVAL: Duration = Duration::from_millis(100);
    const BYTE_INTERVAL: Duration = Duration::from_millis(200);
    match total {
        Some(t) if t > 0 => {
            let percent = ((downloaded * 100) / t).min(100) as u8;
            let first_report = last_percent.is_none();
            let percent_changed = last_percent.is_some_and(|p| p != percent);
            let interval_elapsed = last_report.elapsed() >= PERCENT_INTERVAL;
            if first_report || percent_changed || interval_elapsed {
                *last_percent = Some(percent);
                *last_report = std::time::Instant::now();
                true
            } else {
                false
            }
        }
        _ => {
            if last_percent.is_none() || last_report.elapsed() >= BYTE_INTERVAL {
                // Mark first-report as done; the value is not used as a
                // percentage in this branch (total is unknown).
                *last_percent = Some(0);
                *last_report = std::time::Instant::now();
                true
            } else {
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checksum_for_named_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  vtcode-1.0.0.tar.gz\n";
        assert_eq!(
            parse_checksum_metadata(checksum, "vtcode-1.0.0.tar.gz"),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string())
        );
    }

    #[test]
    fn ignores_checksum_for_another_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  other.tar.gz\n";
        assert!(parse_checksum_metadata(checksum, "vtcode.tar.gz").is_none());
    }

    #[test]
    fn selects_exact_archive_over_compatibility_asset() {
        let checksum = "\
83bcccc61f7dac396a2ffd31bcd6f2dbdc46363b7551e3dd8fc2dedfe5546cb7  compat-vtcode-0.141.7-aarch64-apple-darwin.tar.gz.compat
b9362df9124a6180c5cf0787d6159ff525e7caee6ceb51a0987fb827205df91e  vtcode-0.141.7-aarch64-apple-darwin.tar.gz
";

        assert_eq!(
            parse_checksum_metadata(checksum, "vtcode-0.141.7-aarch64-apple-darwin.tar.gz"),
            Some("b9362df9124a6180c5cf0787d6159ff525e7caee6ceb51a0987fb827205df91e".to_string())
        );
    }

    #[test]
    fn verifies_checksum_and_rejects_mismatch() {
        let expected = digest_hex(b"archive");
        verify_checksum(b"archive", &expected).expect("checksum");
        let error = verify_checksum(b"tampered", &expected).expect_err("mismatch");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn parse_checksum_metadata_returns_none_for_missing_archive() {
        assert!(parse_checksum_metadata("signature only", "vtcode.tar.gz").is_none());
    }

    #[test]
    fn ignores_unrelated_architecture_checksum_sidecars() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-apple-darwin.tar.gz.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        assert!(checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").is_none());
    }

    #[test]
    fn selects_legacy_extension_stripped_checksum_sidecar() {
        // The release pipeline publishes `vtcode-<v>-<target>.sha256`
        // (extension-stripped) alongside the `.tar.gz` archive.
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.sha256");
    }

    #[test]
    fn selects_legacy_extension_stripped_checksum_sidecar_for_zip() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-pc-windows-msvc.zip".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-pc-windows-msvc.sha256".to_string(),
                download_url: "https://example.test/checksum".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-x86_64-pc-windows-msvc.zip").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-x86_64-pc-windows-msvc.sha256");
    }

    #[test]
    fn prefers_modern_sidecar_over_legacy_stripped_sidecar() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz.sha256".to_string(),
                download_url: "https://example.test/modern".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.sha256".to_string(),
                download_url: "https://example.test/legacy".to_string(),
            },
        ];

        let selected = checksum_asset(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").expect("sidecar");

        assert_eq!(selected.name, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz.sha256");
    }

    #[tokio::test]
    async fn download_rejects_untrusted_asset_url_with_asset_context() {
        let asset = ReleaseAsset {
            name: "vtcode.tar.gz".to_string(),
            download_url: "https://example.com/vtcode.tar.gz".to_string(),
        };
        let temp = tempfile::tempdir().expect("temp");
        let error = download_asset(
            &asset,
            &temp.path().join("archive"),
            Duration::from_secs(1),
            false,
            None::<&mut fn(u64, Option<u64>)>,
        )
        .await
        .expect_err("untrusted URL");

        assert!(error.to_string().contains("vtcode.tar.gz"));
    }

    #[tokio::test]
    async fn response_rejects_non_github_asset_url_before_network_request() {
        let error = response_for_url("http://127.0.0.1:1/slow", Duration::from_millis(20))
            .await
            .expect_err("untrusted URL");

        assert!(error.to_string().contains("must use HTTPS"));
    }

    #[test]
    fn should_report_emits_on_percent_change() {
        let mut last_percent = None;
        let mut last_report = std::time::Instant::now();
        // First call always reports (percent goes from None to 0).
        assert!(should_report_download(0, Some(1000), &mut last_percent, &mut last_report));
        // Same percent, no time elapsed → suppressed.
        assert!(!should_report_download(5, Some(1000), &mut last_percent, &mut last_report));
        // Percent changes 0 → 1 → reports.
        assert!(should_report_download(10, Some(1000), &mut last_percent, &mut last_report));
    }

    #[test]
    fn should_report_suppresses_without_total_until_interval() {
        let mut last_percent = None;
        let mut last_report = std::time::Instant::now();
        // First call with no total reports (first-report guard).
        assert!(should_report_download(100, None, &mut last_percent, &mut last_report));
        // Immediately after: suppressed (interval not elapsed).
        assert!(!should_report_download(200, None, &mut last_percent, &mut last_report));
        // Simulate interval elapse by backdating the last report.
        last_report = std::time::Instant::now()
            .checked_sub(Duration::from_millis(250))
            .expect("now minus 250ms is representable");
        assert!(should_report_download(300, None, &mut last_percent, &mut last_report));
    }
}
