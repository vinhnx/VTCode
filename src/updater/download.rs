use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncWriteExt, BufWriter};
use vtcode_commons::http::send_with_retry;

use super::github::{self, ReleaseAsset};

const MAX_CHECKSUM_BYTES: usize = 64 * 1024;

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

async fn download_checksum(asset: &ReleaseAsset, timeout: Duration) -> Result<String> {
    let response = response_for_url(&asset.download_url, timeout)
        .await
        .with_context(|| format!("failed to download checksum asset {}", asset.name))?;
    let mut response = response
        .error_for_status()
        .with_context(|| format!("checksum download returned an error for {}", asset.name))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CHECKSUM_BYTES as u64)
    {
        bail!("checksum metadata {} exceeds {MAX_CHECKSUM_BYTES} bytes", asset.name);
    }
    let mut metadata = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("failed to read checksum metadata {}", asset.name))?
    {
        if chunk.len() > MAX_CHECKSUM_BYTES - metadata.len() {
            bail!("checksum metadata {} exceeds {MAX_CHECKSUM_BYTES} bytes", asset.name);
        }
        metadata.extend_from_slice(&chunk);
    }
    String::from_utf8(metadata).with_context(|| format!("checksum metadata {} is not UTF-8", asset.name))
}

/// Every checksum source the release publishes for `archive_name`, in
/// preference order: the per-archive sidecar (modern `archive.sha256`, then
/// the legacy extension-stripped name the release pipeline publishes today),
/// then the aggregate `checksums.txt` / `sha256sums.txt`.
///
/// Releases publish several of these at once and GitHub serves each asset as
/// its own CDN object, so a 504 for one says nothing about the others:
/// callers walk the whole list before giving up.
pub(super) fn checksum_assets<'a>(assets: &'a [ReleaseAsset], archive_name: &str) -> Vec<&'a ReleaseAsset> {
    let archive_name = archive_name.to_ascii_lowercase();
    let mut names = vec![format!("{archive_name}.sha256")];
    if let Some(legacy) = legacy_checksum_name(&archive_name) {
        names.push(legacy);
    }
    names.push("checksums.txt".to_string());
    names.push("sha256sums.txt".to_string());

    names
        .iter()
        .filter_map(|name| assets.iter().find(|asset| asset.name.eq_ignore_ascii_case(name)))
        .collect()
}

/// Resolve the expected SHA-256 for `archive_name` from the release metadata.
///
/// Each published checksum source is tried in [`checksum_assets`] order, so a
/// CDN failure on the per-archive sidecar still falls back to the aggregate
/// `checksums.txt`. Verification stays fail-closed: a release that publishes
/// no checksum, or whose files never list this archive, is an error rather
/// than a silently unverified install.
pub(super) async fn download_expected_checksum(
    assets: &[ReleaseAsset],
    archive_name: &str,
    timeout: Duration,
) -> Result<String> {
    let candidates = checksum_assets(assets, archive_name);
    let mut download_failure: Option<anyhow::Error> = None;

    for asset in &candidates {
        match download_checksum(asset, timeout).await {
            Ok(metadata) => {
                if let Some(expected) = parse_checksum_metadata(&metadata, archive_name, &asset.name) {
                    return Ok(expected);
                }
            }
            // Keep the last failure as the error source so the reported chain
            // still carries the HTTP status and asset URL.
            Err(err) => download_failure = Some(err),
        }
    }

    match download_failure {
        Some(err) => Err(err).context(format!(
            "no usable checksum metadata for update archive {archive_name} (tried {} published checksum file(s))",
            candidates.len()
        )),
        None if candidates.is_empty() => bail!("no checksum metadata was published for update archive {archive_name}"),
        None => bail!("no published checksum metadata listed update archive {archive_name}"),
    }
}

async fn response_for_url(url: &str, timeout: Duration) -> Result<reqwest::Response> {
    github::validate_asset_download_url(url)?;

    let client = github::unauthenticated_client()?;
    send_with_retry(&github::updater_retry_policy(), || {
        client
            .get(url)
            .timeout(timeout)
            .header(reqwest::header::ACCEPT, "application/octet-stream")
            .send()
    })
    .await
    .with_context(|| format!("failed to download update asset from {url}"))
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

fn parse_checksum_metadata(metadata: &str, archive_name: &str, checksum_name: &str) -> Option<String> {
    let sidecar = checksum_name.to_ascii_lowercase().ends_with(".sha256");
    let mut expected: Option<String> = None;
    for line in metadata.lines() {
        let mut tokens = line.split_whitespace();
        let Some(digest) = tokens.next().filter(|token| is_sha256(token)) else {
            continue;
        };
        let matches_archive = match tokens.next() {
            Some(filename) => {
                let basename = filename
                    .strip_prefix('*')
                    .unwrap_or(filename)
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or_default();
                basename.eq_ignore_ascii_case(archive_name) && tokens.next().is_none()
            }
            None => sidecar && metadata.trim() == digest,
        };
        if matches_archive {
            if let Some(prior) = &expected {
                if !prior.eq_ignore_ascii_case(digest) {
                    return None;
                }
            } else {
                expected = Some(digest.to_ascii_lowercase());
            }
        }
    }
    expected
}

pub(super) fn verify_file_checksum(path: &Path, expected: &str) -> Result<()> {
    if !is_sha256(expected) {
        bail!("invalid SHA-256 checksum metadata");
    }
    let read_error = || format!("failed to read {} for checksum verification", path.display());
    let mut file = BufReader::new(std::fs::File::open(path).with_context(read_error)?);
    let mut hasher = Sha256::new();
    loop {
        let buffer = file.fill_buf().with_context(read_error)?;
        if buffer.is_empty() {
            break;
        }
        hasher.update(buffer);
        let count = buffer.len();
        file.consume(count);
    }
    let actual: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("checksum mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
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

    const AGGREGATE: &str = "checksums.txt";
    const SIDECAR: &str = "vtcode.tar.gz.sha256";

    #[test]
    fn parses_checksum_for_named_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  vtcode-1.0.0.tar.gz\n";
        assert_eq!(
            parse_checksum_metadata(checksum, "vtcode-1.0.0.tar.gz", AGGREGATE),
            Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string())
        );
    }

    #[test]
    fn ignores_checksum_for_another_archive() {
        let checksum = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  other.tar.gz\n";
        assert!(parse_checksum_metadata(checksum, "vtcode.tar.gz", AGGREGATE).is_none());
    }

    #[test]
    fn selects_exact_archive_over_compatibility_asset() {
        let checksum = "\
83bcccc61f7dac396a2ffd31bcd6f2dbdc46363b7551e3dd8fc2dedfe5546cb7  compat-vtcode-0.141.7-aarch64-apple-darwin.tar.gz.compat
b9362df9124a6180c5cf0787d6159ff525e7caee6ceb51a0987fb827205df91e  vtcode-0.141.7-aarch64-apple-darwin.tar.gz
";

        assert_eq!(
            parse_checksum_metadata(checksum, "vtcode-0.141.7-aarch64-apple-darwin.tar.gz", AGGREGATE),
            Some("b9362df9124a6180c5cf0787d6159ff525e7caee6ceb51a0987fb827205df91e".to_string())
        );
    }

    #[test]
    fn verifies_checksum_of_file_across_multiple_reads() {
        let file = tempfile::NamedTempFile::new().expect("temp file");
        let mut contents = vec![b'a'; 32 * 1024];
        std::fs::write(file.path(), &contents).expect("write archive");
        let expected = "b217b65e6f205f41b3fb8ef90cf7c44da93f630ca03965273485bbb21a5cccf5";
        verify_file_checksum(file.path(), expected).expect("checksum");

        contents.push(b'b');
        std::fs::write(file.path(), &contents).expect("write changed archive");
        let error = verify_file_checksum(file.path(), expected).expect_err("mismatch");
        assert!(error.to_string().contains("checksum mismatch"));
    }

    #[test]
    fn parse_checksum_metadata_returns_none_for_missing_archive() {
        assert!(parse_checksum_metadata("signature only", "vtcode.tar.gz", AGGREGATE).is_none());
    }

    #[test]
    fn bare_digest_is_accepted_only_from_a_single_digest_sidecar() {
        let digest = "a".repeat(64);
        let metadata = format!("{digest}\n");
        assert_eq!(parse_checksum_metadata(&metadata, "vtcode.tar.gz", SIDECAR), Some(digest.clone()));
        assert!(parse_checksum_metadata(&metadata, "vtcode.tar.gz", AGGREGATE).is_none());

        let multiple = format!("{digest}\n{digest}  other.tar.gz\n");
        assert!(parse_checksum_metadata(&multiple, "vtcode.tar.gz", SIDECAR).is_none());
    }

    #[test]
    fn named_checksums_reject_ambiguous_or_malformed_records() {
        let first = "a".repeat(64);
        let second = "b".repeat(64);
        let named = format!("{first} *vtcode.tar.gz\n");
        let unrelated = format!("{named}{second}  other.tar.gz\n");
        assert_eq!(parse_checksum_metadata(&unrelated, "vtcode.tar.gz", AGGREGATE), Some(first.clone()));
        assert!(
            parse_checksum_metadata(&format!("{named}{second}  vtcode.tar.gz\n"), "vtcode.tar.gz", AGGREGATE).is_none()
        );
        assert!(
            parse_checksum_metadata(&format!("{first}  other.tar.gz  vtcode.tar.gz\n"), "vtcode.tar.gz", AGGREGATE)
                .is_none()
        );
    }

    /// Real `checksums.txt` body from release 0.181.7 (digests included so the
    /// fixture is the published format, not a paraphrase). The aggregate lists
    /// both the CLI archives and the desktop bundle, which is what makes it a
    /// usable fallback when a per-archive sidecar is unavailable.
    #[test]
    fn parses_real_aggregate_checksums_for_cli_and_desktop_archives() {
        let aggregate = "\
2643377532a0e1667998ecbf66ba9e027c2dc2e54123aa89c7d48a6f0f45be73  vtcode-0.181.7-aarch64-apple-darwin.tar.gz
4d4b679b35a190eca6c03c3a5514116909be489f85bfec73c65e35452c1d3498  vtcode-0.181.7-aarch64-unknown-linux-gnu.tar.gz
2fb7050e4c9635fa6e2c8756201e843ef4937bc4a825be7b09a80e34a81236d4  vtcode-0.181.7-x86_64-apple-darwin.tar.gz
bfb93bcb69083cfb9f2f7f216f650281e2a3acb13321a4896d320f090e997f55  vtcode-0.181.7-x86_64-unknown-linux-gnu.tar.gz
0af917ab9fc69f5ec7649b91af14a2d2c561e986be882eb2028fa5fd621cfcba  vtcode-0.181.7-x86_64-unknown-linux-musl.tar.gz
e070e9e47c8a50bd5984926d2799a05352281c131716df09c52468ce0eabdcd7  vtcode-0.181.7-x86_64-pc-windows-msvc.zip
41cbc573cbe7d803f76adc8f894bf42af5510c45a6309d9ba6c9dbc47ef6e0a5  vtcode-desktop-0.181.7-aarch64-apple-darwin.zip
";

        assert_eq!(
            parse_checksum_metadata(aggregate, "vtcode-0.181.7-aarch64-apple-darwin.tar.gz", AGGREGATE).as_deref(),
            Some("2643377532a0e1667998ecbf66ba9e027c2dc2e54123aa89c7d48a6f0f45be73")
        );
        assert_eq!(
            parse_checksum_metadata(aggregate, "vtcode-desktop-0.181.7-aarch64-apple-darwin.zip", AGGREGATE).as_deref(),
            Some("41cbc573cbe7d803f76adc8f894bf42af5510c45a6309d9ba6c9dbc47ef6e0a5")
        );
        assert!(
            parse_checksum_metadata(aggregate, "vtcode-0.181.7-riscv64-unknown-linux-gnu.tar.gz", AGGREGATE).is_none()
        );
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

        assert!(checksum_assets(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz").is_empty());
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

        let selected = checksum_assets(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "vtcode-1.0.0-aarch64-apple-darwin.sha256");
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

        let selected = checksum_assets(&assets, "vtcode-1.0.0-x86_64-pc-windows-msvc.zip");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "vtcode-1.0.0-x86_64-pc-windows-msvc.sha256");
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

        let selected = checksum_assets(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");

        assert_eq!(selected.len(), 2, "every published source is a fallback candidate");
        assert_eq!(selected[0].name, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz.sha256");
        assert_eq!(selected[1].name, "vtcode-1.0.0-aarch64-apple-darwin.sha256");
    }

    /// The aggregate `checksums.txt` is a real release asset (0.181.7 ships
    /// one), so a per-archive sidecar outage must not end the update: the
    /// aggregate is the last candidate.
    #[test]
    fn aggregate_checksums_file_is_the_last_fallback() {
        let assets = vec![
            ReleaseAsset {
                name: "vtcode-1.0.0-aarch64-apple-darwin.tar.gz".to_string(),
                download_url: "https://example.test/archive".to_string(),
            },
            ReleaseAsset {
                name: "checksums.txt".to_string(),
                download_url: "https://example.test/aggregate".to_string(),
            },
            ReleaseAsset {
                name: "vtcode-1.0.0-x86_64-apple-darwin.sha256".to_string(),
                download_url: "https://example.test/other-target".to_string(),
            },
        ];

        let selected = checksum_assets(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");

        assert_eq!(selected.len(), 1, "only the aggregate lists this archive");
        assert_eq!(selected[0].name, "checksums.txt");
    }

    /// Case-insensitive matching: the release API returns asset names verbatim
    /// and GitHub URLs are case-insensitive, so a differently-cased sidecar is
    /// still the same source.
    #[test]
    fn checksum_source_matching_is_case_insensitive() {
        let assets = vec![ReleaseAsset {
            name: "VTCODE-1.0.0-AARCH64-APPLE-DARWIN.SHA256".to_string(),
            download_url: "https://example.test/checksum".to_string(),
        }];

        let selected = checksum_assets(&assets, "vtcode-1.0.0-aarch64-apple-darwin.tar.gz");

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "VTCODE-1.0.0-AARCH64-APPLE-DARWIN.SHA256");
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
