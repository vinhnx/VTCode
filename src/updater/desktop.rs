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
use super::progress::UpdateProgress;

const DESKTOP_APP_NAME: &str = "VT Code.app";
const DESKTOP_ASSET_PREFIX: &str = "vtcode-desktop-";
const DESKTOP_ASSET_SUFFIX: &str = ".zip";
const DESKTOP_PLIST_RELATIVE_PATH: &str = "Contents/Info.plist";
const DESKTOP_EXECUTABLE_RELATIVE_PATH: &str = "Contents/MacOS/vtcode-desktop";
const DESKTOP_HELPER_RELATIVE_PATH: &str = "Contents/Helpers/vtcode";
/// Gatekeeper first-launch steps shipped at the archive root by ad hoc signed
/// builds (notarized builds omit it).
const DESKTOP_FIRST_LAUNCH_NOTE: &str = "FIRST-LAUNCH.txt";
/// Sibling name for the known-good bundle kept aside during replacement.
/// A fixed name is safe here: it is derived from a fixed root, never user
/// input, and [`replace_installed_bundle`] decides which of the two bundles
/// is authoritative on every run.
const DESKTOP_BACKUP_SUFFIX: &str = ".previous";

pub(crate) enum DesktopInstallOutcome {
    Installed {
        version: String,
        /// Absolute path of the bundle that was just written to disk.
        path: PathBuf,
        needs_gatekeeper_approval: bool,
    },
    UpToDate {
        version: String,
        /// Absolute path of the existing bundle, so callers can point at it.
        path: PathBuf,
    },
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
    install_desktop_app_with_progress(updater, install_dir, |_| {}).await
}

/// [`install_desktop_app`] with per-phase progress reporting.
///
/// The desktop app drives its install UI from these events instead of a bare
/// spinner, so a multi-minute download stays legible. The CLI passes a no-op
/// and keeps its own byte-progress rendering.
pub(crate) async fn install_desktop_app_with_progress<F>(
    updater: &Updater,
    install_dir: &Path,
    mut on_progress: F,
) -> Result<DesktopInstallOutcome>
where
    F: FnMut(UpdateProgress) + Send,
{
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

    if let Some((installed, path)) = installed_desktop_bundle()
        .await
        .filter(|(installed, _)| *installed >= release.version)
    {
        return Ok(DesktopInstallOutcome::UpToDate { version: installed.to_string(), path });
    }

    let (asset, matched_target) = select_desktop_asset(&release.assets, target)?;
    let asset_version = desktop_asset_version(&asset.name, matched_target)?;
    if asset_version != release.version {
        bail!("desktop asset {} does not match release {}", asset.name, release.version);
    }

    let temporary_directory = tempfile::tempdir().context("failed to create desktop install temporary directory")?;
    // Keep remote asset names out of filesystem paths; GitHub metadata is
    // untrusted even though the selected URL and format were validated.
    let archive_path = temporary_directory.path().join("downloaded-desktop-archive");
    // Resolve the checksum before the ~29 MB bundle: it is a few hundred
    // bytes, so a checksum outage — or a release that published none — fails
    // before the archive is downloaded and discarded.
    let expected = download::download_expected_checksum(&release.assets, &asset.name, timeout).await?;
    download::download_asset(
        asset,
        &archive_path,
        timeout,
        true,
        Some(&mut |downloaded, total| {
            on_progress(UpdateProgress::Downloading { downloaded, total });
        }),
    )
    .await
    .context("failed to download desktop app archive")?;

    on_progress(UpdateProgress::VerifyingChecksum);
    // Stream and hash the staged file on the blocking pool.
    let archive_path_for_checksum = archive_path.clone();
    tokio::task::spawn_blocking(move || download::verify_file_checksum(&archive_path_for_checksum, &expected))
        .await
        .context("desktop checksum task panicked")?
        .context("downloaded desktop archive failed checksum verification")?;

    on_progress(UpdateProgress::Extracting);
    let extract_directory = temporary_directory.path().join("extracted");
    let destination = install_dir.join(DESKTOP_APP_NAME);
    let installed =
        tokio::task::spawn_blocking(move || install_desktop_bundle(&archive_path, &extract_directory, &destination))
            .await
            .context("desktop install task panicked")??;

    tracing::info!("desktop app installed to {}", installed.path.display());
    // Seed the shared update cache like the CLI installer does, so the first
    // launch of the new bundle finds its notes instead of a stale check slot
    // (the N vs N+1 race when a newer release lands between check and install).
    super::seed_release_notes_for_installed_version(&release.version, &release.release_notes);
    Ok(DesktopInstallOutcome::Installed {
        version: release.version.to_string(),
        path: installed.path,
        needs_gatekeeper_approval: installed.needs_gatekeeper_approval,
    })
}

/// Resolve the installed bundle and read its version from the bundled helper,
/// returning both so the up-to-date report can name where the app lives.
/// `None` when no bundle resolves or its version is unreadable, which callers
/// treat as "needs install".
async fn installed_desktop_bundle() -> Option<(Version, PathBuf)> {
    let bundle = crate::cli::desktop::resolve_desktop_app(None).ok()?;
    let version = desktop_bundle_version(&bundle, Duration::from_secs(10)).await?;
    Some((version, bundle))
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
    desktop_bundle_version(&bundle, timeout).await
}

/// Run the bundle's bundled helper to read its version, time-bounded so a hung
/// helper can never hang the update.
async fn desktop_bundle_version(bundle: &Path, timeout: Duration) -> Option<Version> {
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
/// the display name is ever aligned with the binary). A `+sha` build-metadata
/// suffix (stamped dev builds) parses and compares equal to the bare release,
/// since semver ignores build metadata in precedence.
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

/// A freshly installed bundle plus whether macOS Gatekeeper will block its
/// first launch (ad hoc signed builds ship the note; notarized ones don't).
struct InstalledBundle {
    path: PathBuf,
    needs_gatekeeper_approval: bool,
}

/// Validate, extract, and install a desktop archive (blocking: zip IO plus
/// `ditto` subprocesses; callers must run this on the blocking pool).
fn install_desktop_bundle(
    archive_path: &Path,
    extract_directory: &Path,
    destination: &Path,
) -> Result<InstalledBundle> {
    validate_desktop_archive(archive_path)?;
    std::fs::create_dir_all(extract_directory)
        .with_context(|| format!("failed to create desktop extraction directory {}", extract_directory.display()))?;
    run_ditto(&["-x", "-k"], archive_path, extract_directory, "failed to extract the desktop app archive", false)?;

    let staged_app = extract_directory.join(DESKTOP_APP_NAME);
    validate_staged_bundle(&staged_app)?;
    let needs_gatekeeper_approval = extract_directory.join(DESKTOP_FIRST_LAUNCH_NOTE).is_file();

    let install_dir = destination
        .parent()
        .context("desktop install destination has no parent directory")?;
    // Elevate the remaining steps when the destination needs it (a system-wide
    // install by a user without write access); admin users writing to a
    // group-writable /Applications stay on the direct path (no password prompt).
    let elevate = ensure_install_dir(install_dir)?;
    // Keep the old bundle recoverable until the new one is fully unpacked and
    // validated. A failed `ditto` (disk full, interruption, permission) must
    // not leave the user with no installed app.
    replace_installed_bundle(destination, elevate, |target| {
        run_ditto(&[], &staged_app, target, "failed to install the desktop app", elevate)
    })?;
    Ok(InstalledBundle {
        path: destination.to_path_buf(),
        needs_gatekeeper_approval,
    })
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

/// Sibling path used to hold the previous bundle while a replacement runs.
fn backup_path(destination: &Path) -> PathBuf {
    let mut name = destination.as_os_str().to_os_string();
    name.push(DESKTOP_BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Move a bundle aside (or back) within the same install directory. Renaming
/// is atomic on one filesystem, so the live install is never half-swapped.
/// `elevate` uses a fixed `mv` argv — `destination` comes from a fixed root or
/// a validated bundle path, never raw user input.
fn move_bundle(from: &Path, to: &Path, elevate: bool) -> Result<()> {
    if !from.exists() {
        bail!("cannot move missing desktop app at {}", from.display());
    }
    // `mv` moves *into* an existing directory instead of replacing it; a
    // stale target would silently nest one bundle inside another.
    if to.exists() {
        bail!("refusing to move desktop app onto existing path {}", to.display());
    }
    if elevate {
        let output = run_sudo("mv", &[from.as_os_str(), to.as_os_str()])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to move desktop app from {} to {}: {}", from.display(), to.display(), stderr.trim());
        }
        return Ok(());
    }
    std::fs::rename(from, to)
        .with_context(|| format!("failed to move desktop app from {} to {}", from.display(), to.display()))
}

/// Remove a bundle directory, tolerating an already-absent path. Only reaches
/// `rm -rf` when a direct removal proved the destination is not user-writable;
/// all paths are fixed-root derived, which is what makes the elevated form safe.
fn remove_bundle(path: &Path, elevate: bool) -> Result<()> {
    if elevate {
        let output = run_sudo("rm", &["-rf".as_ref(), path.as_os_str()])?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to remove desktop app at {}: {}", path.display(), stderr.trim());
        }
        return Ok(());
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err).with_context(|| format!("failed to remove desktop app at {}", path.display())),
    }
    Ok(())
}

/// Replace the installed bundle with one produced by `install`, without ever
/// leaving the user with no app.
///
/// The previous bundle is moved to a sibling backup first, `install` writes
/// and validates the replacement, and only then is the backup removed. If
/// `install` fails, a partial replacement is removed and the backup moved
/// back. A crash during the copy is recoverable too: the next run sees a
/// valid backup beside an absent/invalid bundle and restores it before doing
/// anything else. The whole swap stays inside one install directory, so both
/// moves are atomic renames on the same filesystem.
///
/// `install` receives the destination path and only writes the bundle; this
/// helper validates it before discarding the backup, so a bad payload can
/// never be kept as the live app. Returning `Err` from either step triggers
/// the rollback. The callback is kept small and injectable so failure and
/// recovery paths are testable without `ditto` or macOS.
fn replace_installed_bundle(
    destination: &Path,
    elevate: bool,
    install: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let backup = backup_path(destination);

    // Recovery from an interrupted earlier run takes priority over a new
    // install: the backup may be the only launchable copy in that state.
    if backup.exists() {
        if validate_staged_bundle(destination).is_ok() {
            // Both present and valid: the live bundle is authoritative. A
            // backup we cannot clear is an abnormal filesystem state; fail
            // closed here rather than risk overwriting a working install.
            remove_bundle(&backup, elevate).with_context(|| {
                format!("could not clear leftover update backup at {}; remove it before retrying", backup.display())
            })?;
        } else {
            if validate_staged_bundle(&backup).is_ok() {
                tracing::warn!(path = %backup.display(), "restoring desktop app left in backup by an interrupted update");
                remove_bundle(destination, elevate)?;
                move_bundle(&backup, destination, elevate)?;
            } else {
                // Neither copy is launchable; do not promote a broken backup.
                remove_bundle(&backup, elevate)?;
            }
        }
    }

    let had_previous = destination.exists();
    if had_previous {
        move_bundle(destination, &backup, elevate)
            .with_context(|| format!("failed to set aside previous desktop app at {}", destination.display()))?;
    }

    let result = install(destination).and_then(|()| validate_staged_bundle(destination));
    match result {
        Ok(()) => {
            if had_previous && let Err(error) = remove_bundle(&backup, elevate) {
                // The new bundle is valid, so keep it. The leftover backup is
                // cleaned up (or restored from) on the next update run.
                tracing::warn!(%error, "could not remove previous desktop app backup");
            }
            Ok(())
        }
        Err(error) => {
            // Never leave a half-written bundle at the live path.
            if let Err(cleanup) = remove_bundle(destination, elevate) {
                tracing::warn!(%cleanup, "could not remove incomplete desktop app after failed install");
            }
            if !had_previous {
                return Err(error)
                    .with_context(|| format!("failed to install desktop app at {}", destination.display()));
            }
            match move_bundle(&backup, destination, elevate) {
                Ok(()) => Err(error).with_context(|| {
                    format!("desktop app update failed; previous version at {} restored", destination.display())
                }),
                Err(restore) => Err(error).with_context(|| {
                    format!(
                        "desktop app update failed and the previous version at {} could not be restored: {restore}",
                        backup.display()
                    )
                }),
            }
        }
    }
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

/// A bundle is only usable when all three load-bearing files are present and
/// non-empty: the plist, the GUI binary, and the CLI helper the GUI spawns as
/// its backend. `replace_installed_bundle` uses this check to decide whether
/// to discard the backup of a previous install, so accepting a bundle whose
/// copy stopped after the first two files would delete the only working copy.
fn validate_staged_bundle(bundle: &Path) -> Result<()> {
    for relative in [
        DESKTOP_PLIST_RELATIVE_PATH,
        DESKTOP_EXECUTABLE_RELATIVE_PATH,
        DESKTOP_HELPER_RELATIVE_PATH,
    ] {
        let path = bundle.join(relative);
        let incomplete = !path.is_file() || std::fs::metadata(&path).map(|meta| meta.len() == 0).unwrap_or(true);
        if incomplete {
            bail!("{} is not a valid app bundle (missing or empty {})", bundle.display(), relative);
        }
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
mod tests;
