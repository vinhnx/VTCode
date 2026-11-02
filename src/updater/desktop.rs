//! Prebuilt macOS desktop app install from GitHub release assets.
//!
//! The release pipeline publishes `vtcode-desktop-<version>-<target>.zip`
//! alongside the CLI archives (see `desktop_archive_name` in
//! `scripts/release-assets.sh`). Installing from that asset needs no build
//! toolchain: download, checksum-verify, extract with `ditto`, and copy the
//! bundle to the chosen Applications directory.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use semver::Version;

use super::Updater;
use super::download;
use super::github::{self, ReleaseAsset};
use super::install_source;

const DESKTOP_APP_NAME: &str = "VT Code.app";
const DESKTOP_ASSET_PREFIX: &str = "vtcode-desktop-";
const DESKTOP_ASSET_SUFFIX: &str = ".zip";
const DESKTOP_PLIST_RELATIVE_PATH: &str = "Contents/Info.plist";
const DESKTOP_EXECUTABLE_RELATIVE_PATH: &str = "Contents/MacOS/vtcode-desktop";
const DESKTOP_HELPER_RELATIVE_PATH: &str = "Contents/Helpers/vtcode";

pub(crate) enum DesktopInstallOutcome {
    Installed(String),
    UpToDate(String),
}

/// Whether this host can install the desktop app (macOS only).
pub(crate) fn desktop_supported() -> bool {
    std::env::consts::OS == "macos"
}

/// Installed macOS major version, if detectable. `None` on other platforms
/// and when `sw_vers` is missing or unparseable (callers fail closed).
pub(crate) fn desktop_macos_major() -> Option<u32> {
    if !desktop_supported() {
        return None;
    }
    let output = Command::new("sw_vers").arg("-productVersion").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_macos_major(&String::from_utf8_lossy(&output.stdout))
}

/// Parse `sw_vers -productVersion` output (`15.1`) into a major version.
fn parse_macos_major(output: &str) -> Option<u32> {
    output.lines().next()?.split('.').next()?.trim().parse().ok()
}

/// Install the desktop app from the latest release's prebuilt asset into
/// `install_dir` (`~/Applications` for fresh installs, or the existing
/// install's directory for updates).
///
/// Re-checks the installed version against the live latest release so a stale
/// prompt-time check never triggers a redundant download.
pub(crate) async fn install_desktop_app(updater: &Updater, install_dir: &Path) -> Result<DesktopInstallOutcome> {
    if !desktop_supported() {
        bail!("the VT Code desktop app requires macOS");
    }
    // Fail closed on old or unreadable macOS: the bundle cannot launch there
    // (`LSMinimumSystemVersion` 15.0), so never install it.
    let macos_major = tokio::task::spawn_blocking(desktop_macos_major)
        .await
        .context("desktop OS check panicked")?;
    if !macos_major.is_some_and(|major| major >= 15) {
        bail!("VT Code Desktop requires macOS 15 or newer.");
    }
    let target = install_source::get_target_triple().context("unsupported platform for desktop install")?;
    let timeout = Duration::from_secs(updater.config().download_timeout_secs.max(1));
    let release =
        github::fetch_latest_release_metadata(updater.config().download_timeout_secs, &updater.config().channel)
            .await?;

    let installed = installed_desktop_version().await;
    if let Some(installed) = installed.as_ref().filter(|version| **version >= release.version) {
        return Ok(DesktopInstallOutcome::UpToDate(installed.to_string()));
    }

    let (asset, matched_target) = select_desktop_asset(&release.assets, target)?;
    let asset_version = desktop_asset_version(&asset.name, matched_target)?;
    if asset_version != release.version {
        bail!("desktop asset {} does not match release {}", asset.name, release.version);
    }
    let checksum_asset = download::checksum_asset(&release.assets, &asset.name)
        .with_context(|| format!("no checksum metadata was published for desktop archive {}", asset.name))?;

    let temporary_directory = tempfile::tempdir().context("failed to create desktop install temporary directory")?;
    // Keep remote asset names out of filesystem paths; GitHub metadata is
    // untrusted even though the selected URL and format were validated.
    let archive_path = temporary_directory.path().join("downloaded-desktop-archive");
    download::download_asset(asset, &archive_path, timeout, true, None::<&mut fn(u64, Option<u64>)>)
        .await
        .context("failed to download desktop app archive")?;

    let metadata = download::download_checksum(checksum_asset, timeout)
        .await
        .with_context(|| format!("failed to download checksum metadata for desktop archive {}", asset.name))?;
    let expected = download::parse_checksum_metadata(&metadata, &asset.name)
        .with_context(|| format!("checksum metadata did not contain desktop archive {}", asset.name))?;
    // `verify_file_checksum` does blocking `std::fs::read` + hashing; run it
    // on the blocking pool so the async runtime is not stalled.
    let archive_path_for_checksum = archive_path.clone();
    tokio::task::spawn_blocking(move || download::verify_file_checksum(&archive_path_for_checksum, &expected))
        .await
        .context("desktop checksum task panicked")?
        .context("downloaded desktop archive failed checksum verification")?;

    let extract_directory = temporary_directory.path().join("extracted");
    let destination = install_dir.join(DESKTOP_APP_NAME);
    let installed_path =
        tokio::task::spawn_blocking(move || install_desktop_bundle(&archive_path, &extract_directory, &destination))
            .await
            .context("desktop install task panicked")??;

    tracing::info!("desktop app installed to {}", installed_path.display());
    Ok(DesktopInstallOutcome::Installed(release.version.to_string()))
}

/// Read the installed desktop app's version from its bundled CLI helper.
///
/// The bundle ships its matching `vtcode` helper (`Contents/Helpers/vtcode`),
/// built from the same source tree, so `helper --version` is the bundle
/// version. Returns `None` when no app is installed or its version cannot be
/// determined; callers treat that as "needs install". The helper run is
/// time-bounded so a hung helper can never hang the update.
pub(crate) async fn installed_desktop_version() -> Option<Version> {
    installed_desktop_version_with_timeout(Duration::from_secs(10)).await
}

async fn installed_desktop_version_with_timeout(timeout: Duration) -> Option<Version> {
    let bundle = crate::cli::desktop::resolve_desktop_app(None).ok()?;
    let helper = bundle.join(DESKTOP_HELPER_RELATIVE_PATH);
    let child = tokio::process::Command::new(&helper)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()?;
    let output = tokio::time::timeout(timeout, child.wait_with_output()).await.ok()?.ok()?;
    if !output.status.success() {
        return None;
    }
    parse_helper_version(&String::from_utf8_lossy(&output.stdout))
}

/// Parse the bundled helper's `--version` output into a version.
///
/// Only the first line carries the version (`vtcode-core 0.181.0`); the rest
/// is a multi-line environment report. Strict: anything but exactly two
/// tokens on that line yields `None` (unknown version), which makes callers
/// proceed with the install. A lenient parse could mistake unexpected helper
/// output for an up-to-date app and skip a needed update. The first token is
/// the clap binary name, `vtcode-core` today (`vtcode` is accepted too in case
/// the display name is ever aligned with the binary).
fn parse_helper_version(output: &str) -> Option<Version> {
    let first_line = output.lines().next().unwrap_or_default();
    let mut tokens = first_line.split_whitespace();
    match (tokens.next(), tokens.next(), tokens.next()) {
        (Some("vtcode" | "vtcode-core"), Some(version), None) => Version::parse(version).ok(),
        _ => None,
    }
}

/// Candidate desktop asset targets, preferred first: the native target, then
/// the Intel target on Apple Silicon (Rosetta 2). Intel hosts only try their
/// native target; they cannot run arm64 binaries.
fn candidate_targets(native_target: &str) -> Vec<&str> {
    let mut targets = vec![native_target];
    if native_target == "aarch64-apple-darwin" {
        targets.push("x86_64-apple-darwin");
    }
    targets
}

/// Select the desktop app archive for this host, returning the asset and the
/// target it matched (native or Rosetta fallback).
fn select_desktop_asset<'a>(assets: &'a [ReleaseAsset], native_target: &'a str) -> Result<(&'a ReleaseAsset, &'a str)> {
    for target in candidate_targets(native_target) {
        let target_suffix = format!("{target}{DESKTOP_ASSET_SUFFIX}");
        let candidates: Vec<_> = assets
            .iter()
            .filter(|asset| {
                let name = asset.name.to_ascii_lowercase();
                name.starts_with(DESKTOP_ASSET_PREFIX) && name.ends_with(&target_suffix)
            })
            .collect();
        match candidates.as_slice() {
            [asset] => return Ok((asset, target)),
            [] => continue,
            _ => bail!("multiple desktop app archives found for target {target}"),
        }
    }
    bail!(
        "no desktop app archive found for target {native_target} (this release ships no prebuilt macOS app for it); build it with scripts/package-desktop-macos.sh or re-run with --without-desktop to skip"
    )
}

/// Extract the version embedded in a desktop asset name
/// (`vtcode-desktop-<version>-<target>.zip`). Case-insensitive like the
/// selection filter, so a matched asset always parses.
fn desktop_asset_version(asset_name: &str, target: &str) -> Result<Version> {
    let suffix = format!("-{target}{DESKTOP_ASSET_SUFFIX}");
    let lowered = asset_name.to_ascii_lowercase();
    let version_str = lowered
        .strip_prefix(DESKTOP_ASSET_PREFIX)
        .and_then(|rest| rest.strip_suffix(suffix.as_str()))
        .with_context(|| format!("unexpected desktop asset name: {asset_name}"))?;
    Version::parse(version_str).with_context(|| format!("invalid version in desktop asset name: {asset_name}"))
}

/// Default fresh-install directory (`~/Applications`).
pub(crate) fn default_install_dir() -> Result<PathBuf> {
    let expanded = vtcode_commons::paths::expand_tilde("~/Applications");
    if expanded == Path::new("~/Applications") {
        bail!("could not resolve the home directory for the desktop app install");
    }
    Ok(expanded)
}

/// Directory holding the currently installed desktop app, if any. Updates
/// install in place so a system-wide install is never duplicated per-user.
pub(crate) fn existing_install_dir() -> Option<PathBuf> {
    let bundle = crate::cli::desktop::resolve_desktop_app(None).ok()?;
    bundle.parent().map(Path::to_path_buf)
}

/// Validate, extract, and install a desktop archive (blocking: zip IO plus
/// `ditto` subprocesses; callers must run this on the blocking pool).
fn install_desktop_bundle(archive_path: &Path, extract_directory: &Path, destination: &Path) -> Result<PathBuf> {
    validate_desktop_archive(archive_path)?;
    std::fs::create_dir_all(extract_directory)
        .with_context(|| format!("failed to create desktop extraction directory {}", extract_directory.display()))?;
    run_ditto(&["-x", "-k"], archive_path, extract_directory, "failed to extract the desktop app archive", false)?;

    let staged_app = extract_directory.join(DESKTOP_APP_NAME);
    validate_staged_bundle(&staged_app)?;

    let install_dir = destination
        .parent()
        .context("desktop install destination has no parent directory")?;
    // Elevate the remaining steps when the destination needs it (a system-wide
    // install by a user without write access); admin users writing to a
    // group-writable /Applications stay on the direct path (no password prompt).
    let elevate = ensure_install_dir(install_dir)?;
    remove_previous_install(destination, elevate)?;
    run_ditto(&[], &staged_app, destination, "failed to install the desktop app", elevate)?;
    validate_staged_bundle(destination)?;
    Ok(destination.to_path_buf())
}

/// Ensure the install directory exists and decide whether the remaining steps
/// must elevate. Returns `true` when a writability probe fails, i.e. only a
/// privileged copy can succeed.
fn ensure_install_dir(install_dir: &Path) -> Result<bool> {
    match std::fs::create_dir_all(install_dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            let output = run_sudo("mkdir", &["-p".as_ref(), install_dir.as_os_str()])
                .with_context(|| format!("failed to create desktop install directory {}", install_dir.display()))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                bail!("failed to create desktop install directory {}: {}", install_dir.display(), stderr.trim());
            }
        }
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to create desktop install directory {}", install_dir.display()));
        }
    }
    // `create_dir_all` succeeds on an existing directory without checking
    // writability, so probe with a real write. The probe name is unique per
    // run and auto-removed on drop, so it can neither collide nor litter.
    match tempfile::NamedTempFile::new_in(install_dir) {
        Ok(_) => Ok(false),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => Ok(true),
        Err(err) => {
            Err(err).with_context(|| format!("failed to probe desktop install directory {}", install_dir.display()))
        }
    }
}

/// Remove any previous install first so stale bundle files never survive an update.
/// `destination` is constructed from fixed roots or a validated bundle path,
/// never raw user input, which is what makes the elevated `rm -rf` safe.
fn remove_previous_install(destination: &Path, elevate: bool) -> Result<()> {
    if elevate {
        let output = run_sudo("rm", &["-rf".as_ref(), destination.as_os_str()])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to remove previous desktop app at {}: {}", destination.display(), stderr.trim());
        }
        return Ok(());
    }
    match std::fs::remove_dir_all(destination) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to remove previous desktop app at {}", destination.display()));
        }
    }
    Ok(())
}

/// Fail closed on archives with unsafe paths or an unexpected layout before
/// `ditto` touches them. Symlinks are allowed (app bundles legitimately
/// contain them); `ditto` preserves them.
fn validate_desktop_archive(archive_path: &Path) -> Result<()> {
    let expected_plist = format!("{DESKTOP_APP_NAME}/{DESKTOP_PLIST_RELATIVE_PATH}");
    let archive =
        std::fs::File::open(archive_path).with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(archive).context("failed to read desktop app archive")?;
    let mut has_plist = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).context("failed to read desktop app archive entry")?;
        let name = entry.name().context("invalid desktop app archive entry name")?.to_string();
        if entry.enclosed_name().is_none() {
            bail!("unsafe archive path: {name}");
        }
        if name == expected_plist {
            has_plist = true;
        }
    }
    if !has_plist {
        bail!("desktop archive is missing {expected_plist}");
    }
    Ok(())
}

fn validate_staged_bundle(bundle: &Path) -> Result<()> {
    if !bundle.join(DESKTOP_PLIST_RELATIVE_PATH).is_file() {
        bail!("{} is not a valid app bundle (missing {})", bundle.display(), DESKTOP_PLIST_RELATIVE_PATH);
    }
    if !bundle.join(DESKTOP_EXECUTABLE_RELATIVE_PATH).is_file() {
        bail!("{} is not a valid app bundle (missing {})", bundle.display(), DESKTOP_EXECUTABLE_RELATIVE_PATH);
    }
    Ok(())
}

fn run_ditto(args: &[&str], source: &Path, destination: &Path, context: &str, elevate: bool) -> Result<()> {
    let output = if elevate {
        let mut sudo_args: Vec<&std::ffi::OsStr> = args.iter().map(AsRef::as_ref).collect();
        sudo_args.push(source.as_os_str());
        sudo_args.push(destination.as_os_str());
        run_sudo("ditto", &sudo_args)?
    } else {
        Command::new("ditto")
            .args(args)
            .arg(source)
            .arg(destination)
            .output()
            .context("ditto is required to install the desktop app but was not found")?
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{context}: ditto exited with {}: {}", output.status, stderr.trim());
    }
    Ok(())
}

/// Run `tool` under `sudo` for a system-wide install step. Only reached after
/// a direct attempt proved the destination is not user-writable.
fn run_sudo(tool: &str, args: &[&std::ffi::OsStr]) -> Result<std::process::Output> {
    Command::new("sudo")
        .arg(tool)
        .args(args)
        .output()
        .context("sudo is required for the system-wide desktop install but was not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            download_url: format!("https://example.test/{name}"),
        }
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("archive");
        let mut writer = ZipWriter::new(file);
        for (name, contents) in entries {
            writer.start_file(*name, SimpleFileOptions::default()).expect("entry");
            writer.write_all(contents).expect("contents");
        }
        writer.finish().expect("zip");
    }

    #[test]
    fn selects_native_desktop_asset_and_ignores_cli_archives() {
        let assets = vec![
            asset("checksums.txt"),
            asset("vtcode-0.181.0-aarch64-apple-darwin.tar.gz"),
            asset("vtcode-desktop-0.181.0-x86_64-apple-darwin.zip"),
            asset("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip"),
            asset("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip.sha256"),
        ];

        let (selected, target) = select_desktop_asset(&assets, "aarch64-apple-darwin").expect("asset");

        assert_eq!(selected.name, "vtcode-desktop-0.181.0-aarch64-apple-darwin.zip");
        assert_eq!(target, "aarch64-apple-darwin");
    }

    #[test]
    fn falls_back_to_intel_asset_on_apple_silicon() {
        let assets = vec![asset("vtcode-desktop-0.181.0-x86_64-apple-darwin.zip")];

        let (selected, target) = select_desktop_asset(&assets, "aarch64-apple-darwin").expect("asset");

        assert_eq!(selected.name, "vtcode-desktop-0.181.0-x86_64-apple-darwin.zip");
        assert_eq!(target, "x86_64-apple-darwin");
    }

    #[test]
    fn intel_host_never_selects_arm_asset() {
        let assets = vec![asset("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip")];

        let error = select_desktop_asset(&assets, "x86_64-apple-darwin").expect_err("no asset");

        assert!(error.to_string().contains("no desktop app archive found"));
    }

    #[test]
    fn rejects_ambiguous_desktop_assets() {
        let assets = vec![
            asset("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip"),
            asset("VTCode-Desktop-0.181.0-aarch64-apple-darwin.zip"),
        ];

        let error = select_desktop_asset(&assets, "aarch64-apple-darwin").expect_err("ambiguous");

        assert!(error.to_string().contains("multiple desktop app archives"));
    }

    #[test]
    fn extracts_asset_version_for_either_target() {
        assert_eq!(
            desktop_asset_version("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip", "aarch64-apple-darwin")
                .expect("version"),
            Version::parse("0.181.0").expect("semver")
        );
        assert_eq!(
            desktop_asset_version("vtcode-desktop-0.181.0-x86_64-apple-darwin.zip", "x86_64-apple-darwin")
                .expect("version"),
            Version::parse("0.181.0").expect("semver")
        );
        // Selection matches case-insensitively, so parsing must too: a matched
        // asset always yields its version.
        assert_eq!(
            desktop_asset_version("VTCode-Desktop-0.181.0-AARCH64-APPLE-DARWIN.ZIP", "aarch64-apple-darwin")
                .expect("version"),
            Version::parse("0.181.0").expect("semver")
        );
        assert!(
            desktop_asset_version("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip", "x86_64-apple-darwin").is_err()
        );
        assert!(desktop_asset_version("vtcode-0.181.0-aarch64-apple-darwin.tar.gz", "aarch64-apple-darwin").is_err());
    }

    #[test]
    fn parses_helper_version_output() {
        let current = Some(Version::parse("0.181.0").expect("semver"));
        // The shipped helper prints its clap name (`vtcode-core`) plus a
        // multi-line environment report; only the first line carries the version.
        assert_eq!(parse_helper_version("vtcode-core 0.181.0\n\nAuthors: Someone\n"), current);
        assert_eq!(parse_helper_version("vtcode 0.181.0\n"), current);
        assert_eq!(parse_helper_version("vtcode 0.181.0-extra words here\n"), None);
        assert_eq!(parse_helper_version(""), None);
        assert_eq!(parse_helper_version("something else 1.2.3"), None);
        assert_eq!(parse_helper_version("vtcode not-a-version"), None);
    }

    #[test]
    fn accepts_well_formed_desktop_archive() {
        let temp = tempfile::tempdir().expect("temp");
        let archive = temp.path().join("desktop.zip");
        write_zip(
            &archive,
            &[
                ("VT Code.app/Contents/Info.plist", b"plist"),
                ("VT Code.app/Contents/MacOS/vtcode-desktop", b"binary"),
                ("FIRST-LAUNCH.txt", b"note"),
            ],
        );

        validate_desktop_archive(&archive).expect("valid archive");
    }

    #[test]
    fn rejects_archive_without_bundle_or_with_escape_paths() {
        let temp = tempfile::tempdir().expect("temp");
        let no_bundle = temp.path().join("no-bundle.zip");
        write_zip(&no_bundle, &[("junk.txt", b"junk")]);
        let error = validate_desktop_archive(&no_bundle).expect_err("missing bundle");
        assert!(error.to_string().contains("missing VT Code.app/Contents/Info.plist"));

        let traversal = temp.path().join("traversal.zip");
        write_zip(&traversal, &[("VT Code.app/Contents/Info.plist", b"plist"), ("../../evil", b"evil")]);
        let error = validate_desktop_archive(&traversal).expect_err("traversal");
        assert!(error.to_string().contains("unsafe archive path"));
        // Absolute entry names are rejected by the same `enclosed_name`
        // check, but `ZipWriter` sanitizes them at write time so no fixture
        // can express that case through this writer.
    }

    #[test]
    fn validates_staged_bundle_layout() {
        let temp = tempfile::tempdir().expect("temp");
        let bundle = temp.path().join("VT Code.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).expect("dirs");
        std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");
        let error = validate_staged_bundle(&bundle).expect_err("missing executable");
        assert!(error.to_string().contains("Contents/MacOS/vtcode-desktop"));

        std::fs::write(bundle.join("Contents/MacOS/vtcode-desktop"), b"binary").expect("binary");
        validate_staged_bundle(&bundle).expect("valid bundle");
    }

    /// Point `VTCODE_DESKTOP_APP` at a fixture bundle for the duration of the
    /// test, restoring the previous value afterwards.
    #[cfg(unix)]
    fn with_desktop_app_env(bundle: &Path) -> (vtcode_commons::env_lock::EnvGuard, Option<std::ffi::OsString>) {
        let env = vtcode_commons::env_lock::lock();
        let previous = std::env::var_os("VTCODE_DESKTOP_APP");
        env.set_var("VTCODE_DESKTOP_APP", bundle);
        (env, previous)
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_version_reads_bundled_helper() {
        let temp = tempfile::tempdir().expect("temp");
        let bundle = temp.path().join("VT Code.app");
        let helpers = bundle.join("Contents/Helpers");
        std::fs::create_dir_all(&helpers).expect("dirs");
        std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");
        let helper = helpers.join("vtcode");
        std::fs::write(&helper, b"#!/bin/sh\necho 'vtcode-core 0.181.0'\n").expect("helper");
        set_mode(&helper, 0o755);

        let (env, previous) = with_desktop_app_env(&bundle);
        let version = installed_desktop_version().await;
        match previous {
            Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
            None => env.remove_var("VTCODE_DESKTOP_APP"),
        }

        assert_eq!(version, Some(Version::parse("0.181.0").expect("semver")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn installed_version_is_none_without_helper() {
        let temp = tempfile::tempdir().expect("temp");
        let bundle = temp.path().join("VT Code.app");
        std::fs::create_dir_all(bundle.join("Contents")).expect("dirs");
        std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");

        let (env, previous) = with_desktop_app_env(&bundle);
        let version = installed_desktop_version().await;
        match previous {
            Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
            None => env.remove_var("VTCODE_DESKTOP_APP"),
        }

        assert_eq!(version, None);
    }

    /// A hung helper must time out (fail open to install) instead of hanging
    /// the update; `kill_on_drop` reaps the orphaned child.
    #[cfg(unix)]
    #[tokio::test]
    async fn installed_version_times_out_on_hung_helper() {
        let temp = tempfile::tempdir().expect("temp");
        let bundle = temp.path().join("VT Code.app");
        let helpers = bundle.join("Contents/Helpers");
        std::fs::create_dir_all(&helpers).expect("dirs");
        std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");
        let helper = helpers.join("vtcode");
        std::fs::write(&helper, b"#!/bin/sh\nexec sleep 30\n").expect("helper");
        set_mode(&helper, 0o755);

        let (env, previous) = with_desktop_app_env(&bundle);
        let version = tokio::time::timeout(
            Duration::from_secs(20),
            installed_desktop_version_with_timeout(Duration::from_millis(100)),
        )
        .await
        .expect("test must not hang");
        match previous {
            Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
            None => env.remove_var("VTCODE_DESKTOP_APP"),
        }

        assert_eq!(version, None);
    }

    #[test]
    fn default_install_dir_points_at_user_applications() {
        let dir = default_install_dir().expect("home dir");
        assert!(dir.is_absolute(), "{dir:?}");
        assert_eq!(dir.file_name().and_then(|name| name.to_str()), Some("Applications"));
    }

    #[cfg(unix)]
    #[test]
    fn existing_install_dir_reports_bundle_parent() {
        let temp = tempfile::tempdir().expect("temp");
        let bundle = temp.path().join("Apps").join("VT Code.app");
        std::fs::create_dir_all(bundle.join("Contents")).expect("dirs");
        std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");

        let (env, previous) = with_desktop_app_env(&bundle);
        let dir = existing_install_dir();
        match previous {
            Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
            None => env.remove_var("VTCODE_DESKTOP_APP"),
        }

        // The resolver canonicalizes (e.g. /var -> /private/var on macOS).
        let expected = vtcode_commons::canonicalize(temp.path().join("Apps")).expect("canonical");
        assert_eq!(dir, Some(expected));
    }

    #[cfg(unix)]
    #[test]
    fn existing_install_dir_is_none_without_bundle() {
        // The override shadows any real install, so a non-bundle path is
        // deterministically None on every machine.
        let temp = tempfile::tempdir().expect("temp");
        let not_bundle = temp.path().join("missing.app");

        let (env, previous) = with_desktop_app_env(&not_bundle);
        let dir = existing_install_dir();
        match previous {
            Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
            None => env.remove_var("VTCODE_DESKTOP_APP"),
        }

        assert_eq!(dir, None);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_install_dir_reports_elevation_need() {
        let temp = tempfile::tempdir().expect("temp");
        // Writable: direct path, and the writability probe (NamedTempFile,
        // auto-removed on drop) leaves no litter.
        let writable = temp.path().join("writable");
        assert!(!ensure_install_dir(&writable).expect("writable dir"));
        assert_eq!(std::fs::read_dir(&writable).expect("read dir").count(), 0);

        // Existing but non-writable: only a privileged copy can succeed.
        let locked = temp.path().join("locked");
        std::fs::create_dir(&locked).expect("dir");
        set_mode(&locked, 0o555);
        assert!(ensure_install_dir(&locked).expect("locked dir"));
        set_mode(&locked, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn ensure_install_dir_elevates_mkdir_when_denied() {
        let temp = tempfile::tempdir().expect("temp");
        let stub_dir = temp.path().join("stub-bin");
        std::fs::create_dir(&stub_dir).expect("stub dir");
        write_sudo_stub(&stub_dir);
        let log = temp.path().join("sudo.log");

        // Missing dir under a locked parent: direct mkdir is denied, so sudo
        // mkdir is attempted; the stub runs the real mkdir, which still fails.
        let parent = temp.path().join("locked-parent");
        std::fs::create_dir(&parent).expect("parent");
        set_mode(&parent, 0o555);
        let _guard = PathGuard::with_stub_dir(&stub_dir, &log);
        let error = ensure_install_dir(&parent.join("Apps")).expect_err("denied");
        assert!(error.to_string().contains("failed to create desktop install directory"), "{error:#}");
        let logged = std::fs::read_to_string(&log).expect("log");
        assert!(logged.contains("mkdir -p"), "{logged}");
        set_mode(&parent, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn elevated_ditto_routes_through_sudo() {
        let temp = tempfile::tempdir().expect("temp");
        let stub_dir = temp.path().join("stub-bin");
        std::fs::create_dir(&stub_dir).expect("stub dir");
        write_sudo_stub(&stub_dir);
        let log = temp.path().join("sudo.log");

        let _guard = PathGuard::with_stub_dir(&stub_dir, &log);
        let error = run_ditto(&[], Path::new("/nonexistent-src"), &temp.path().join("dst"), "ctx-message", true)
            .expect_err("ditto fails");
        assert!(error.to_string().contains("ctx-message"), "{error:#}");
        let logged = std::fs::read_to_string(&log).expect("log");
        assert!(logged.contains("ditto /nonexistent-src"), "{logged}");
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(mode);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }

    /// A `sudo` test double: logs its arguments, then runs the real command
    /// (which still fails without privileges, so elevation can never hang or
    /// succeed by accident in tests).
    #[cfg(unix)]
    fn write_sudo_stub(dir: &Path) {
        let stub = dir.join("sudo");
        std::fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"$VTCODE_TEST_SUDO_LOG\"\nexec \"$@\"\n")
            .expect("stub");
        set_mode(&stub, 0o755);
    }

    /// Prepends a stub dir to `PATH` and restores the environment on drop.
    #[cfg(unix)]
    struct PathGuard {
        env: vtcode_commons::env_lock::EnvGuard,
        previous_path: Option<std::ffi::OsString>,
        previous_log: Option<std::ffi::OsString>,
    }

    #[cfg(unix)]
    impl PathGuard {
        fn with_stub_dir(stub_dir: &Path, log_file: &Path) -> Self {
            let env = vtcode_commons::env_lock::lock();
            let previous_path = std::env::var_os("PATH");
            let previous_log = std::env::var_os("VTCODE_TEST_SUDO_LOG");
            let joined = match previous_path.as_ref() {
                Some(path) => format!("{}:{}", stub_dir.display(), path.to_string_lossy()),
                None => stub_dir.display().to_string(),
            };
            env.set_var("PATH", joined);
            env.set_var("VTCODE_TEST_SUDO_LOG", log_file);
            Self { env, previous_path, previous_log }
        }
    }

    #[cfg(unix)]
    impl Drop for PathGuard {
        fn drop(&mut self) {
            match self.previous_path.take() {
                Some(value) => self.env.set_var("PATH", value),
                None => self.env.remove_var("PATH"),
            }
            match self.previous_log.take() {
                Some(value) => self.env.set_var("VTCODE_TEST_SUDO_LOG", value),
                None => self.env.remove_var("VTCODE_TEST_SUDO_LOG"),
            }
        }
    }

    /// End-to-end bundle install through `ditto` into a temp Applications dir.
    /// macOS-only: `ditto` is a system tool there and absent elsewhere.
    #[cfg(target_os = "macos")]
    #[test]
    fn installs_bundle_with_ditto_and_replaces_stale_install() {
        let temp = tempfile::tempdir().expect("temp");
        let archive = temp.path().join("desktop.zip");
        write_zip(
            &archive,
            &[
                ("VT Code.app/Contents/Info.plist", b"plist"),
                ("VT Code.app/Contents/MacOS/vtcode-desktop", b"new-binary"),
            ],
        );
        let extract = temp.path().join("extracted");
        let destination = temp.path().join("Applications").join("VT Code.app");
        // A stale previous install must not survive the update.
        std::fs::create_dir_all(destination.join("Contents/MacOS")).expect("stale dirs");
        std::fs::write(destination.join("Contents/MacOS/vtcode-desktop"), b"old-binary").expect("stale binary");
        std::fs::write(destination.join("Contents/stale.txt"), b"stale").expect("stale file");

        let installed = install_desktop_bundle(&archive, &extract, &destination).expect("install");

        assert_eq!(installed, destination);
        assert_eq!(std::fs::read(destination.join("Contents/MacOS/vtcode-desktop")).expect("read"), b"new-binary");
        assert!(!destination.join("Contents/stale.txt").exists());
    }

    #[test]
    fn parses_macos_major_from_product_version() {
        assert_eq!(parse_macos_major("15.1\n"), Some(15));
        assert_eq!(parse_macos_major("14.7.1"), Some(14));
        assert_eq!(parse_macos_major("26.0.1"), Some(26));
        assert_eq!(parse_macos_major(""), None);
        assert_eq!(parse_macos_major("Sequoia"), None);
        assert_eq!(parse_macos_major(" 15.2 "), Some(15));
        assert_eq!(parse_macos_major("x.1"), None);
    }

    /// End-to-end through a stubbed `sw_vers`: the boundary (14 vs 15) and
    /// a nonzero exit both drive `desktop_macos_major`. macOS-only: the gate
    /// short-circuits to `None` on other platforms.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_gate_reads_major_from_sw_vers() {
        fn stub_sw_vers(dir: &Path, body: &str) {
            let stub = dir.join("sw_vers");
            std::fs::write(&stub, format!("#!/bin/sh\n{body}\n")).expect("stub");
            set_mode(&stub, 0o755);
        }

        let temp = tempfile::tempdir().expect("temp");
        let stub_dir = temp.path().join("stub-bin");
        std::fs::create_dir(&stub_dir).expect("stub dir");
        let log = temp.path().join("unused.log");
        let _guard = PathGuard::with_stub_dir(&stub_dir, &log);

        stub_sw_vers(&stub_dir, "echo 15.1");
        assert_eq!(desktop_macos_major(), Some(15));
        stub_sw_vers(&stub_dir, "echo 14.7.1");
        assert_eq!(desktop_macos_major(), Some(14));
        stub_sw_vers(&stub_dir, "exit 1");
        assert_eq!(desktop_macos_major(), None);
    }
}
