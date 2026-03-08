use std::fs::{self, File, OpenOptions};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;
use tempfile::TempDir;
use vtcode_core::tools::ast_grep_binary::{
    alias_ast_grep_binary_name, canonical_ast_grep_binary_name, managed_ast_grep_bin_dir,
    resolve_ast_grep_binary_from_env_and_fs,
};
use zip::ZipArchive;

const AST_GREP_RELEASE_API: &str = "https://api.github.com/repos/ast-grep/ast-grep/releases/latest";
const INSTALL_LOCK_MAX_AGE_SECS: u64 = 1_800;
const INSTALL_CACHE_STALE_AFTER_SECS: u64 = 86_400;

#[derive(Debug, Clone, PartialEq)]
pub enum AstGrepStatus {
    Available {
        version: String,
        binary: PathBuf,
        managed: bool,
    },
    NotFound,
    Error {
        reason: String,
    },
}

#[derive(Debug, Clone)]
pub struct AstGrepInstallOutcome {
    pub version: String,
    pub binary_path: PathBuf,
    pub alias_path: Option<PathBuf>,
    pub managed_bin_dir: PathBuf,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallationCache {
    last_attempt: u64,
    status: String,
    release_tag: Option<String>,
    failure_reason: Option<String>,
}

#[derive(Debug, Clone)]
struct InstallPaths {
    state_dir: PathBuf,
    bin_dir: PathBuf,
    cache_path: PathBuf,
    lock_path: PathBuf,
    binary_path: PathBuf,
    alias_path: Option<PathBuf>,
}

#[derive(Debug)]
struct InstallLockGuard {
    path: PathBuf,
    _file: File,
}

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Clone, Copy)]
struct PlatformAssetSpec {
    candidate_triples: &'static [&'static str],
    archive_ext: &'static str,
}

#[derive(Debug, Clone)]
struct SelectedAsset {
    tag_name: String,
    asset: ReleaseAsset,
}

impl AstGrepStatus {
    pub fn check() -> Self {
        let Some(binary) = resolve_ast_grep_binary_from_env_and_fs() else {
            return Self::NotFound;
        };

        match ast_grep_version(&binary) {
            Ok(version) => Self::Available {
                version,
                managed: binary.starts_with(managed_ast_grep_bin_dir()),
                binary,
            },
            Err(err) => Self::Error {
                reason: err.to_string(),
            },
        }
    }

    pub async fn install() -> Result<AstGrepInstallOutcome> {
        let paths = InstallPaths::discover()?;
        let _lock = InstallLockGuard::acquire(&paths)?;

        if !InstallationCache::is_stale(&paths)
            && let Ok(cache) = InstallationCache::load(&paths)
            && cache.status == "failed"
        {
            let reason = cache.failure_reason.as_deref().unwrap_or("unknown reason");
            bail!(
                "Previous ast-grep installation attempt failed ({}). Not retrying for 24 hours.",
                reason
            );
        }

        let install_result: Result<AstGrepInstallOutcome> = async {
            let client = reqwest::Client::builder()
                .user_agent("vtcode-ast-grep-installer")
                .build()
                .context("Failed to create HTTP client")?;

            let release = fetch_latest_release(&client).await?;
            let platform = current_platform_asset_spec()?;
            let selected_asset = select_release_asset(&release, &platform)?;
            let archive_bytes = download_release_asset(&client, &selected_asset.asset).await?;
            let warning =
                verify_checksum_if_available(&client, &release, &selected_asset, &archive_bytes)
                    .await?;

            install_archive(&paths, &selected_asset.asset.name, &archive_bytes)?;
            let version = ast_grep_version(&paths.binary_path)
                .context("Installed ast-grep failed version check")?;
            InstallationCache::mark_success(&paths, &selected_asset.tag_name);

            Ok(AstGrepInstallOutcome {
                version,
                binary_path: paths.binary_path.clone(),
                alias_path: paths.alias_path.clone(),
                managed_bin_dir: paths.bin_dir.clone(),
                warning,
            })
        }
        .await;

        match install_result {
            Ok(outcome) => Ok(outcome),
            Err(err) => {
                InstallationCache::mark_failure(&paths, &err.to_string());
                Err(err)
            }
        }
    }
}

impl InstallationCache {
    fn load(paths: &InstallPaths) -> Result<Self> {
        let content = fs::read_to_string(&paths.cache_path)
            .with_context(|| format!("Failed to read {}", paths.cache_path.display()))?;
        serde_json::from_str(&content).context("Failed to parse ast-grep install cache")
    }

    fn save(&self, paths: &InstallPaths) -> Result<()> {
        fs::create_dir_all(&paths.state_dir)
            .with_context(|| format!("Failed to create {}", paths.state_dir.display()))?;
        let content =
            serde_json::to_string(self).context("Failed to serialize ast-grep install cache")?;
        fs::write(&paths.cache_path, content)
            .with_context(|| format!("Failed to write {}", paths.cache_path.display()))?;
        Ok(())
    }

    fn is_stale(paths: &InstallPaths) -> bool {
        let Ok(cache) = Self::load(paths) else {
            return true;
        };
        let now = unix_timestamp_now();
        now.saturating_sub(cache.last_attempt) > INSTALL_CACHE_STALE_AFTER_SECS
    }

    fn mark_success(paths: &InstallPaths, release_tag: &str) {
        let cache = Self {
            last_attempt: unix_timestamp_now(),
            status: "success".to_string(),
            release_tag: Some(release_tag.to_string()),
            failure_reason: None,
        };
        let _ = cache.save(paths);
    }

    fn mark_failure(paths: &InstallPaths, reason: &str) {
        let cache = Self {
            last_attempt: unix_timestamp_now(),
            status: "failed".to_string(),
            release_tag: None,
            failure_reason: Some(reason.to_string()),
        };
        let _ = cache.save(paths);
    }
}

impl InstallPaths {
    fn discover() -> Result<Self> {
        let home = dirs::home_dir()
            .context("Cannot determine home directory for VT Code-managed ast-grep install")?;
        Ok(Self::from_home(&home))
    }

    fn from_home(home: &Path) -> Self {
        let state_dir = home.join(".vtcode");
        let bin_dir = state_dir.join("bin");
        Self {
            cache_path: state_dir.join("ast_grep_install_cache.json"),
            lock_path: state_dir.join("ast_grep.lock"),
            binary_path: bin_dir.join(canonical_ast_grep_binary_name()),
            alias_path: alias_ast_grep_binary_name().map(|name| bin_dir.join(name)),
            state_dir,
            bin_dir,
        }
    }
}

impl InstallLockGuard {
    fn acquire(paths: &InstallPaths) -> Result<Self> {
        if Self::is_install_in_progress(paths) {
            bail!("ast-grep installation already in progress");
        }

        fs::create_dir_all(&paths.state_dir)
            .with_context(|| format!("Failed to create {}", paths.state_dir.display()))?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&paths.lock_path)
            .with_context(|| format!("Failed to create {}", paths.lock_path.display()))?;

        Ok(Self {
            path: paths.lock_path.clone(),
            _file: file,
        })
    }

    fn is_install_in_progress(paths: &InstallPaths) -> bool {
        let Ok(metadata) = fs::metadata(&paths.lock_path) else {
            return false;
        };
        let Ok(modified) = metadata.modified() else {
            return false;
        };
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default()
            .as_secs();
        age < INSTALL_LOCK_MAX_AGE_SECS
    }
}

impl Drop for InstallLockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

async fn fetch_latest_release(client: &reqwest::Client) -> Result<ReleaseInfo> {
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

fn current_platform_asset_spec() -> Result<PlatformAssetSpec> {
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

fn select_release_asset(
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

fn asset_matches_target(asset_name: &str, triple: &str, archive_ext: &str) -> bool {
    let name = asset_name.to_ascii_lowercase();
    name.contains(triple)
        && name.ends_with(archive_ext)
        && !name.ends_with(".sha256")
        && !name.contains("checksum")
        && !name.ends_with(".sig")
}

async fn download_release_asset(client: &reqwest::Client, asset: &ReleaseAsset) -> Result<Vec<u8>> {
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

async fn verify_checksum_if_available(
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

fn parse_expected_checksum(
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

fn install_archive(paths: &InstallPaths, archive_name: &str, archive_bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(&paths.bin_dir)
        .with_context(|| format!("Failed to create {}", paths.bin_dir.display()))?;

    let temp_dir =
        TempDir::new().context("Failed to create temp directory for ast-grep install")?;
    let extract_dir = temp_dir.path().join("extract");
    fs::create_dir_all(&extract_dir)
        .with_context(|| format!("Failed to create {}", extract_dir.display()))?;

    extract_archive(archive_name, archive_bytes, &extract_dir)?;
    let extracted_binary = find_extracted_binary(&extract_dir)?;

    fs::copy(&extracted_binary, &paths.binary_path).with_context(|| {
        format!(
            "Failed to install ast-grep to {}",
            paths.binary_path.display()
        )
    })?;
    set_executable_permissions(&paths.binary_path)?;

    if cfg!(target_os = "linux") {
        let stale_alias = paths.bin_dir.join("sg");
        if stale_alias.exists() {
            let _ = fs::remove_file(stale_alias);
        }
    } else if let Some(alias_path) = &paths.alias_path {
        fs::copy(&paths.binary_path, alias_path).with_context(|| {
            format!(
                "Failed to install ast-grep alias to {}",
                alias_path.display()
            )
        })?;
        set_executable_permissions(alias_path)?;
    }

    Ok(())
}

fn extract_archive(archive_name: &str, archive_bytes: &[u8], destination: &Path) -> Result<()> {
    if archive_name.ends_with(".tar.gz") {
        let decoder = GzDecoder::new(Cursor::new(archive_bytes));
        let mut archive = Archive::new(decoder);
        archive
            .unpack(destination)
            .with_context(|| format!("Failed to unpack {}", archive_name))?;
        return Ok(());
    }

    if archive_name.ends_with(".zip") {
        let cursor = Cursor::new(archive_bytes);
        let mut archive =
            ZipArchive::new(cursor).with_context(|| format!("Failed to open {}", archive_name))?;
        for index in 0..archive.len() {
            let mut file = archive
                .by_index(index)
                .with_context(|| format!("Failed to read {} entry {}", archive_name, index))?;
            let outpath = destination.join(file.mangled_name());

            if file.is_dir() {
                fs::create_dir_all(&outpath)
                    .with_context(|| format!("Failed to create {}", outpath.display()))?;
                continue;
            }

            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            let mut outfile = File::create(&outpath)
                .with_context(|| format!("Failed to create {}", outpath.display()))?;
            std::io::copy(&mut file, &mut outfile)
                .with_context(|| format!("Failed to extract {}", outpath.display()))?;
        }
        return Ok(());
    }

    bail!("Unsupported ast-grep archive format: {}", archive_name);
}

fn find_extracted_binary(root: &Path) -> Result<PathBuf> {
    let alias_name = alias_ast_grep_binary_name();
    walkdir::WalkDir::new(root)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .find_map(|entry| {
            let name = entry.file_name().to_string_lossy();
            if name == canonical_ast_grep_binary_name()
                || alias_name.is_some_and(|alias| name == alias)
            {
                Some(entry.into_path())
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow!("ast-grep binary not found in release archive"))
}

fn ast_grep_version(binary: &Path) -> Result<String> {
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .with_context(|| format!("Failed to run {}", binary.display()))?;
    if !output.status.success() {
        bail!(
            "{} --version exited with status {}",
            binary.display(),
            output.status
        );
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        bail!("{} --version returned empty output", binary.display());
    }
    Ok(version)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn set_executable_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = fs::metadata(path)
            .with_context(|| format!("Failed to inspect {}", path.display()))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions)
            .with_context(|| format!("Failed to update permissions for {}", path.display()))?;
    }

    #[cfg(not(unix))]
    {
        let _ = path;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        InstallLockGuard, InstallPaths, InstallationCache, PlatformAssetSpec, ReleaseAsset,
        ReleaseInfo, asset_matches_target, parse_expected_checksum, select_release_asset,
    };
    use tempfile::TempDir;

    #[test]
    fn install_paths_live_under_vtcode_home() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        let expected_state_dir = temp_dir.path().join(".vtcode");
        let expected_bin_dir = expected_state_dir.join("bin");
        assert_eq!(paths.state_dir, expected_state_dir);
        assert_eq!(paths.bin_dir, expected_bin_dir);
        assert_eq!(
            paths.cache_path,
            temp_dir.path().join(".vtcode/ast_grep_install_cache.json")
        );
        assert_eq!(
            paths.lock_path,
            temp_dir.path().join(".vtcode/ast_grep.lock")
        );
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

    #[test]
    fn install_lock_detects_recent_lockfile() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        std::fs::create_dir_all(&paths.state_dir).expect("state dir");
        std::fs::write(&paths.lock_path, "lock").expect("lock file");

        assert!(InstallLockGuard::is_install_in_progress(&paths));
    }

    #[test]
    fn installation_cache_round_trips() {
        let temp_dir = TempDir::new().expect("temp dir");
        let paths = InstallPaths::from_home(temp_dir.path());
        let cache = InstallationCache {
            last_attempt: 42,
            status: "failed".to_string(),
            release_tag: None,
            failure_reason: Some("boom".to_string()),
        };

        cache.save(&paths).expect("save cache");
        let loaded = InstallationCache::load(&paths).expect("load cache");
        assert_eq!(loaded.last_attempt, 42);
        assert_eq!(loaded.status, "failed");
        assert_eq!(loaded.failure_reason.as_deref(), Some("boom"));
    }
}
