//! Desktop-bundle updater tests: asset selection, archive staging, and
//! the installed-version probe.

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
    assert!(desktop_asset_version("vtcode-desktop-0.181.0-aarch64-apple-darwin.zip", "x86_64-apple-darwin").is_err());
    assert!(desktop_asset_version("vtcode-0.181.0-aarch64-apple-darwin.tar.gz", "aarch64-apple-darwin").is_err());
}

#[test]
fn parses_helper_version_output() {
    let current = Some(Version::parse("0.181.0").expect("semver"));
    // The shipped helper prints its clap name (`vtcode-core`) plus a
    // multi-line environment report; only the first line carries the version.
    assert_eq!(parse_helper_version("vtcode-core 0.181.0\n\nAuthors: Someone\n"), current);
    assert_eq!(parse_helper_version("vtcode 0.181.0\n"), current);
    // Stamped dev builds stay parseable and compare equal to their
    // release (build metadata is ignored in precedence), so the
    // `installed >= release` up-to-date check never loops on them.
    let stamped = parse_helper_version("vtcode-core 0.181.0+f19a52f2f1a2b\n\nAuthors: Someone\n");
    assert_eq!(stamped, Some(Version::parse("0.181.0+f19a52f2f1a2b").expect("semver")));
    assert!(stamped.is_some_and(|version| version >= Version::parse("0.181.0").expect("semver")));
    assert_eq!(
        parse_helper_version("vtcode-core 0.181.0+f19a52f2f1a2b.dirty\n"),
        Some(Version::parse("0.181.0+f19a52f2f1a2b.dirty").expect("semver"))
    );
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
    let error = validate_staged_bundle(&bundle).expect_err("missing helper");
    assert!(error.to_string().contains("Contents/Helpers/vtcode"), "{error}");

    let helper = bundle.join(DESKTOP_HELPER_RELATIVE_PATH);
    std::fs::create_dir_all(helper.parent().expect("helper parent")).expect("helper dirs");
    std::fs::write(&helper, b"#!/bin/sh\nexit 0\n").expect("helper");
    validate_staged_bundle(&bundle).expect("valid bundle");

    // A copy that stopped mid-write can leave an empty file behind.
    std::fs::write(&helper, b"").expect("empty helper");
    let error = validate_staged_bundle(&bundle).expect_err("empty helper");
    assert!(error.to_string().contains("missing or empty"), "{error}");
}

fn write_bundle(path: &Path, binary: &[u8]) {
    let macos = path.join("Contents/MacOS");
    std::fs::create_dir_all(&macos).expect("dirs");
    std::fs::write(path.join(DESKTOP_PLIST_RELATIVE_PATH), b"plist").expect("plist");
    std::fs::write(macos.join("vtcode-desktop"), binary).expect("binary");
    let helper = path.join(DESKTOP_HELPER_RELATIVE_PATH);
    std::fs::create_dir_all(helper.parent().expect("helper parent")).expect("helper dirs");
    std::fs::write(helper, b"#!/bin/sh\nexit 0\n").expect("helper");
}

fn bundle_binary(path: &Path) -> Vec<u8> {
    std::fs::read(path.join("Contents/MacOS/vtcode-desktop")).expect("binary")
}

/// The swap must keep none of the previous bundle's stale files, and must not
/// leave its backup behind once the validated replacement is in place.
#[test]
fn replacing_a_bundle_discards_stale_files_and_the_backup() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    write_bundle(&destination, b"old-binary");
    std::fs::write(destination.join("Contents/stale.txt"), b"stale").expect("stale");

    replace_installed_bundle(&destination, false, |target| {
        write_bundle(target, b"new-binary");
        Ok(())
    })
    .expect("swap");

    assert_eq!(bundle_binary(&destination), b"new-binary");
    assert!(!destination.join("Contents/stale.txt").exists());
    assert!(!backup_path(&destination).exists());
}

/// A failed copy must restore the previous bundle byte-for-byte, remove the
/// partial replacement, and still report the underlying error to the caller.
#[test]
fn a_failed_install_restores_the_previous_bundle() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    write_bundle(&destination, b"old-binary");

    let error = replace_installed_bundle(&destination, false, |target| {
        write_bundle(target, b"partial");
        std::fs::write(target.join("Contents/torn.txt"), b"torn").expect("torn");
        anyhow::bail!("ditto failed")
    })
    .expect_err("install must fail");

    assert_eq!(bundle_binary(&destination), b"old-binary");
    assert!(!destination.join("Contents/torn.txt").exists());
    assert!(!backup_path(&destination).exists());
    assert!(format!("{error:#}").contains("ditto failed"), "{error:#}");
    assert!(format!("{error:#}").contains("restored"), "{error:#}");
}

/// A crash after setting the old bundle aside but before the copy starts
/// leaves no bundle at the destination and the only launchable copy in the
/// backup. The next run must recover it before starting a new install. A copy
/// that was already partly written is covered by
/// `an_incomplete_live_bundle_never_loses_the_backup`.
#[test]
fn an_interrupted_update_is_recovered_from_the_backup() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    let backup = backup_path(&destination);
    write_bundle(&backup, b"known-good");

    // The replacement fails; recovery must still restore the old app and the
    // caller must see the install error rather than a silent success.
    let error = replace_installed_bundle(&destination, false, |_| anyhow::bail!("download torn"))
        .expect_err("install must fail");

    assert_eq!(bundle_binary(&destination), b"known-good");
    assert!(!backup.exists());
    assert!(format!("{error:#}").contains("download torn"), "{error:#}");
}

/// Recovery must also allow a fresh attempt to succeed: the recovered bundle
/// is set aside again, then replaced by the validated new one.
#[test]
fn an_interrupted_update_can_be_replaced_on_the_next_run() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    let backup = backup_path(&destination);
    write_bundle(&backup, b"known-good");

    replace_installed_bundle(&destination, false, |target| {
        write_bundle(target, b"new-binary");
        Ok(())
    })
    .expect("swap");

    assert_eq!(bundle_binary(&destination), b"new-binary");
    assert!(!backup.exists());
}

/// An interrupted copy can leave the plist and GUI binary in place but not the
/// helper the GUI needs to spawn its backend. That partial bundle must not be
/// treated as "live and valid" at recovery time, or the only working backup is
/// deleted and the user is left with an app that cannot start.
#[test]
fn an_incomplete_live_bundle_never_loses_the_backup() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    let backup = backup_path(&destination);
    write_bundle(&backup, b"known-good");
    std::fs::create_dir_all(destination.join("Contents/MacOS")).expect("dirs");
    std::fs::write(destination.join(DESKTOP_PLIST_RELATIVE_PATH), b"plist").expect("plist");
    std::fs::write(destination.join("Contents/MacOS/vtcode-desktop"), b"partial").expect("binary");

    let error =
        replace_installed_bundle(&destination, false, |_| anyhow::bail!("disk full")).expect_err("install must fail");

    assert_eq!(bundle_binary(&destination), b"known-good");
    assert!(!backup.exists());
    assert!(format!("{error:#}").contains("disk full"), "{error:#}");
}

/// Validation belongs to the swap, not the writer: a payload that returned
/// Ok but is missing bundle files must still roll back to the known-good app.
#[test]
fn an_invalid_replacement_rolls_back_to_the_previous_bundle() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");
    write_bundle(&destination, b"old-binary");

    let error = replace_installed_bundle(&destination, false, |target| {
        std::fs::create_dir_all(target).expect("dir");
        std::fs::write(target.join("not-a-bundle.txt"), b"junk").expect("junk");
        Ok(())
    })
    .expect_err("invalid bundle must be rejected");

    assert_eq!(bundle_binary(&destination), b"old-binary");
    assert!(!destination.join("not-a-bundle.txt").exists());
    assert!(!backup_path(&destination).exists());
    assert!(format!("{error:#}").contains("not a valid app bundle"), "{error:#}");
}

/// A first install has nothing to roll back to, so a failure just leaves no
/// bundle at the destination and no backup claiming to be one.
#[test]
fn a_failed_fresh_install_leaves_no_bundle_or_backup() {
    let temp = tempfile::tempdir().expect("temp");
    let destination = temp.path().join("Applications").join("VT Code.app");

    let error = replace_installed_bundle(&destination, false, |target| {
        write_bundle(target, b"partial");
        anyhow::bail!("checksum mismatch")
    })
    .expect_err("install must fail");

    assert!(!destination.exists());
    assert!(!backup_path(&destination).exists());
    assert!(format!("{error:#}").contains("checksum mismatch"), "{error:#}");
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

/// The up-to-date branch needs both the version and the bundle path so the
/// CLI can report where the app lives. An unreadable version still counts
/// as "needs install" (see `installed_version_is_none_without_helper`).
#[cfg(unix)]
#[tokio::test]
async fn installed_bundle_reports_version_and_path() {
    let temp = tempfile::tempdir().expect("temp");
    let bundle = temp.path().join("VT Code.app");
    let helpers = bundle.join("Contents/Helpers");
    std::fs::create_dir_all(&helpers).expect("dirs");
    std::fs::write(bundle.join("Contents/Info.plist"), b"plist").expect("plist");
    let helper = helpers.join("vtcode");
    std::fs::write(&helper, b"#!/bin/sh\necho 'vtcode-core 0.181.0'\n").expect("helper");
    set_mode(&helper, 0o755);

    let (env, previous) = with_desktop_app_env(&bundle);
    let installed = installed_desktop_bundle().await;
    match previous {
        Some(value) => env.set_var("VTCODE_DESKTOP_APP", value),
        None => env.remove_var("VTCODE_DESKTOP_APP"),
    }

    let (version, path) = installed.expect("bundle with helper");
    assert_eq!(version, Version::parse("0.181.0").expect("semver"));
    // The resolver canonicalizes (e.g. /var -> /private/var on macOS).
    assert_eq!(path, vtcode_commons::canonicalize(&bundle).expect("canonical"));
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

/// The elevated half of the swap uses different syscalls (`sudo mv`, `sudo
/// rm -rf`) from the non-elevated tests. Drive the whole rollback through the
/// sudo stub so a failed install still restores the previous bundle there.
#[cfg(unix)]
#[test]
fn elevated_bundle_swap_routes_through_sudo_and_rolls_back() {
    let temp = tempfile::tempdir().expect("temp");
    let stub_dir = temp.path().join("stub-bin");
    std::fs::create_dir(&stub_dir).expect("stub dir");
    write_sudo_stub(&stub_dir);
    let log = temp.path().join("sudo.log");
    let destination = temp.path().join("Applications").join("VT Code.app");
    write_bundle(&destination, b"old-binary");

    let _guard = PathGuard::with_stub_dir(&stub_dir, &log);
    let error = replace_installed_bundle(&destination, true, |_| anyhow::bail!("install failed"))
        .expect_err("install must fail");

    assert_eq!(bundle_binary(&destination), b"old-binary");
    assert!(!backup_path(&destination).exists());
    let logged = std::fs::read_to_string(&log).expect("log");
    assert!(logged.contains("mv "), "{logged}");
    assert!(logged.contains("rm -rf"), "{logged}");
    assert!(logged.contains(DESKTOP_BACKUP_SUFFIX), "{logged}");
    assert!(format!("{error:#}").contains("install failed"), "{error:#}");
}

#[test]
fn move_bundle_refuses_to_nest_into_an_existing_target() {
    let temp = tempfile::tempdir().expect("temp");
    let from = temp.path().join("from.app");
    let to = temp.path().join("to.app");
    write_bundle(&from, b"source");
    write_bundle(&to, b"target");

    let error = move_bundle(&from, &to, false).expect_err("target is occupied");

    assert!(error.to_string().contains("refusing to move"), "{error:#}");
    assert_eq!(bundle_binary(&from), b"source");
    assert_eq!(bundle_binary(&to), b"target");
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
    std::fs::write(&stub, "#!/bin/sh\nprintf '%s\\n' \"$*\" >>\"$VTCODE_TEST_SUDO_LOG\"\nexec \"$@\"\n").expect("stub");
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
            ("VT Code.app/Contents/Helpers/vtcode", b"helper"),
        ],
    );
    let extract = temp.path().join("extracted");
    let destination = temp.path().join("Applications").join("VT Code.app");
    // A stale previous install must not survive the update.
    std::fs::create_dir_all(destination.join("Contents/MacOS")).expect("stale dirs");
    std::fs::write(destination.join("Contents/MacOS/vtcode-desktop"), b"old-binary").expect("stale binary");
    std::fs::write(destination.join("Contents/stale.txt"), b"stale").expect("stale file");

    let installed = install_desktop_bundle(&archive, &extract, &destination).expect("install");

    assert_eq!(installed.path, destination);
    assert!(!installed.needs_gatekeeper_approval);
    assert_eq!(std::fs::read(destination.join("Contents/MacOS/vtcode-desktop")).expect("read"), b"new-binary");
    assert!(!destination.join("Contents/stale.txt").exists());
}

/// An archive shipping FIRST-LAUNCH.txt (ad hoc signed) flags the install
/// so callers print the manual-approval steps; notarized layouts omit it.
#[cfg(target_os = "macos")]
#[test]
fn ad_hoc_archive_flags_gatekeeper_approval() {
    let temp = tempfile::tempdir().expect("temp");
    let archive = temp.path().join("desktop.zip");
    write_zip(
        &archive,
        &[
            ("VT Code.app/Contents/Info.plist", b"plist"),
            ("VT Code.app/Contents/MacOS/vtcode-desktop", b"binary"),
            ("VT Code.app/Contents/Helpers/vtcode", b"helper"),
            ("FIRST-LAUNCH.txt", b"first launch note"),
        ],
    );
    let extract = temp.path().join("extracted");
    let destination = temp.path().join("Applications").join("VT Code.app");

    let installed = install_desktop_bundle(&archive, &extract, &destination).expect("install");

    assert_eq!(installed.path, destination);
    assert!(installed.needs_gatekeeper_approval);
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
