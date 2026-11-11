use super::*;
use tempfile::tempdir;

fn migration_paths(temp: &tempfile::TempDir) -> VtCodePaths {
    VtCodePaths {
        config_dir: temp.path().join("config"),
        data_dir: temp.path().join("data"),
        state_dir: temp.path().join("state"),
        cache_dir: temp.path().join("cache"),
        runtime_dir: temp.path().join("runtime"),
        executable_dir: temp.path().join("bin"),
        system_config_dirs: Vec::new(),
        system_data_dirs: Vec::new(),
        legacy_home_dir: temp.path().join("legacy"),
    }
}

#[test]
fn resolver_honors_explicit_overrides() {
    let paths = VtCodePaths::from_environment(&[
        ("VTCODE_HOME", "/ignored"),
        ("VTCODE_CONFIG", "/config"),
        ("VTCODE_DATA", "/data"),
    ])
    .expect("resolve paths");
    assert_eq!(paths.config_dir(), Path::new("/config"));
    assert_eq!(paths.data_dir(), Path::new("/data"));
    assert_eq!(paths.legacy_home_dir(), Path::new("/ignored"));
    assert_ne!(paths.auth_dir(), Path::new("/data/auth"));
}

#[test]
fn resolver_defaults_are_absolute_and_categories_are_separate() {
    let paths = VtCodePaths::from_environment(&[]).expect("resolve defaults");
    assert!(paths.config_dir().is_absolute());
    assert!(paths.data_dir().is_absolute());
    assert!(paths.runtime_dir().is_absolute());
    assert_eq!(paths.auth_file(), paths.auth_dir().join("auth.json"));
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
#[test]
fn resolver_ignores_relative_xdg_inputs() {
    let paths = VtCodePaths::from_environment(&[
        ("HOME", "/tmp/vtcode-home"),
        ("XDG_CONFIG_HOME", "relative/config"),
        ("XDG_RUNTIME_DIR", "relative/runtime"),
        ("XDG_BIN_HOME", "relative/bin"),
    ])
    .expect("relative XDG root should be ignored");
    assert_eq!(paths.config_dir(), Path::new("/tmp/vtcode-home/.config/vtcode"));
    assert_eq!(paths.runtime_dir(), Path::new("/tmp/vtcode-home/.local/state/vtcode/runtime"));
    assert_eq!(paths.executable_dir(), Path::new("/tmp/vtcode-home/.local/bin"));
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
#[test]
fn resolver_preserves_xdg_search_order_and_defaults_empty_values() {
    let paths = VtCodePaths::from_environment(&[
        ("HOME", "/tmp/vtcode-home"),
        ("XDG_CONFIG_DIRS", "/first:/second"),
        ("XDG_DATA_DIRS", " "),
    ])
    .expect("resolve search roots");
    assert_eq!(paths.system_config_dirs(), &[PathBuf::from("/first"), PathBuf::from("/second")]);
    assert_eq!(paths.system_data_dirs(), &[PathBuf::from("/usr/local/share"), PathBuf::from("/usr/share")]);
}

#[cfg(any(
    target_os = "linux",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd",
    target_os = "dragonfly"
))]
#[test]
fn system_config_paths_convert_xdg_preference_order_to_layer_order() {
    let paths = VtCodePaths::from_environment(&[("HOME", "/tmp/vtcode-home"), ("XDG_CONFIG_DIRS", "/first:/second")])
        .expect("resolve search roots");

    assert_eq!(
        paths.system_config_paths("vtcode.toml").expect("resolve system config paths"),
        vec![
            PathBuf::from("/etc/vtcode/vtcode.toml"),
            PathBuf::from("/second/vtcode/vtcode.toml"),
            PathBuf::from("/first/vtcode/vtcode.toml"),
        ]
    );
}

#[cfg(target_os = "macos")]
#[test]
fn native_macos_resolution_ignores_xdg_roots() {
    let paths = VtCodePaths::from_environment(&[
        ("HOME", "/tmp/vtcode-home"),
        ("XDG_CONFIG_HOME", "/tmp/xdg/config"),
        ("XDG_DATA_HOME", "/tmp/xdg/data"),
        ("XDG_STATE_HOME", "/tmp/xdg/state"),
        ("XDG_CACHE_HOME", "/tmp/xdg/cache"),
    ])
    .expect("resolve native macOS paths");
    assert!(!paths.config_dir().starts_with("/tmp/xdg"));
    assert!(!paths.data_dir().starts_with("/tmp/xdg"));
    assert!(paths.config_dir().to_string_lossy().contains("com.vinhnx.vtcode"));
}

#[cfg(target_os = "windows")]
#[test]
fn native_windows_resolution_ignores_xdg_roots() {
    let paths = VtCodePaths::from_environment(&[
        ("HOME", r"C:\\Users\\vtcode"),
        ("XDG_CONFIG_HOME", r"C:\\xdg\\config"),
        ("XDG_DATA_HOME", r"C:\\xdg\\data"),
        ("XDG_STATE_HOME", r"C:\\xdg\\state"),
        ("XDG_CACHE_HOME", r"C:\\xdg\\cache"),
    ])
    .expect("resolve native Windows paths");
    assert!(!paths.config_dir().to_string_lossy().contains("xdg"));
    assert!(!paths.data_dir().to_string_lossy().contains("xdg"));
}

#[cfg(unix)]
#[test]
fn runtime_and_auth_storage_are_private() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    let runtime = paths.ensure_runtime_dir().expect("create runtime");
    let auth = paths.ensure_auth_dir().expect("create auth");
    let auth_file = paths.create_auth_file("credentials.json").expect("create auth file");

    assert_eq!(fs::metadata(runtime).expect("runtime metadata").permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(auth).expect("auth metadata").permissions().mode() & 0o777, 0o700);
    assert_eq!(fs::metadata(auth_file).expect("auth file metadata").permissions().mode() & 0o777, 0o600);
}

#[cfg(unix)]
#[test]
fn newly_created_user_directories_are_private_but_existing_modes_are_preserved() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    fs::create_dir_all(paths.config_dir()).expect("existing config directory");
    fs::set_permissions(paths.config_dir(), fs::Permissions::from_mode(0o755)).expect("set existing mode");

    let _ = paths.ensure_config_dir().expect("preserve existing config directory");
    let _ = paths.ensure_data_dir().expect("create data directory");
    let _ = paths.ensure_state_child_dir("sessions").expect("create state child");
    let _ = paths.ensure_cache_child_dir("prompts").expect("create cache child");
    let _ = paths.ensure_executable_dir().expect("create executable directory");

    assert_eq!(fs::metadata(paths.config_dir()).expect("config metadata").permissions().mode() & 0o777, 0o755);
    assert_eq!(fs::metadata(paths.data_dir()).expect("data metadata").permissions().mode() & 0o777, 0o700);
    assert_eq!(
        fs::metadata(paths.state_dir().join("sessions"))
            .expect("state child metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(paths.cache_dir().join("prompts"))
            .expect("cache child metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(paths.executable_dir())
            .expect("executable metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    symlink(temp.path().join("outside"), paths.cache_dir().join("unsafe")).expect("create cache symlink");
    assert!(paths.ensure_cache_child_dir("unsafe/nested").is_err());
}

#[test]
fn migration_copies_explicit_categories_once_and_preserves_sources() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    fs::create_dir_all(paths.legacy_home_dir().join("plugins")).expect("legacy plugins");
    fs::write(paths.legacy_home_dir().join("vtcode.toml"), "theme = 'dark'").expect("legacy config");
    fs::write(paths.legacy_home_dir().join("plugins/example"), "plugin").expect("legacy plugin");

    let first = paths.migrate_legacy().expect("migrate legacy data");
    let second = paths.migrate_legacy().expect("migrate idempotently");

    assert_eq!(fs::read_to_string(paths.config_file()).expect("migrated config"), "theme = 'dark'");
    assert_eq!(fs::read_to_string(paths.plugins_dir().join("example")).expect("migrated plugin"), "plugin");
    assert!(paths.legacy_home_dir().join("vtcode.toml").exists());
    assert_eq!(first.migrated.len(), 2);
    assert!(first.marker_written);
    assert!(paths.migration_report_path().is_file());
    assert!(second.already_completed);
}

#[test]
fn migration_copies_user_guidance_and_prompt_configuration_to_config() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    fs::create_dir_all(paths.legacy_home_dir().join("prompts/examples")).expect("legacy prompts");
    fs::write(paths.legacy_home_dir().join("AGENTS.md"), "user guidance").expect("legacy guidance");
    fs::write(paths.legacy_home_dir().join("config.toml"), "enabled = true").expect("legacy dot config");
    fs::write(paths.legacy_home_dir().join("prompts/examples/example.md"), "# Example").expect("legacy prompt example");

    let report = paths.migrate_legacy().expect("migrate user configuration");

    assert_eq!(
        fs::read_to_string(paths.config_dir().join("AGENTS.md")).expect("migrated guidance"),
        "user guidance"
    );
    assert_eq!(
        fs::read_to_string(paths.config_dir().join("config.toml")).expect("migrated dot config"),
        "enabled = true"
    );
    assert_eq!(
        fs::read_to_string(paths.config_dir().join("prompts/examples/example.md")).expect("migrated prompt"),
        "# Example"
    );
    assert!(report.migrated.len() >= 3);
    assert!(paths.legacy_home_dir().join("prompts/examples/example.md").is_file());
}

#[test]
fn migration_copies_pre_xdg_config_root_cache_and_state() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    let old_cache_file = paths.config_dir().join("cache/models/dynamic_local_models.json");
    let old_log_file = paths.config_dir().join("logs/session.log");
    let old_session_file = paths.config_dir().join("sessions/session.jsonl");
    let old_backup_file = paths.config_dir().join("backups/config.toml");
    for path in [&old_cache_file, &old_log_file, &old_session_file, &old_backup_file] {
        fs::create_dir_all(path.parent().expect("legacy parent")).expect("create legacy parent");
        fs::write(path, path.file_name().expect("file name").to_string_lossy().as_bytes()).expect("write legacy file");
    }

    let report = paths.migrate_legacy().expect("migrate pre-XDG config data");

    for (source, destination) in [
        (old_cache_file, paths.cache_dir().join("models/dynamic_local_models.json")),
        (old_log_file, paths.state_dir().join("logs/session.log")),
        (old_session_file, paths.state_dir().join("sessions/session.jsonl")),
        (old_backup_file, paths.state_dir().join("backups/config.toml")),
    ] {
        assert!(source.is_file());
        assert_eq!(fs::read(&source).expect("read source"), fs::read(&destination).expect("read destination"));
    }
    assert!(report.migrated.len() >= 4);
}

#[test]
fn migration_reports_conflicts_and_excludes_tmp() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    fs::create_dir_all(paths.legacy_home_dir()).expect("legacy root");
    fs::write(paths.legacy_home_dir().join("vtcode.toml"), "legacy").expect("legacy config");
    fs::write(paths.legacy_home_dir().join("tmp"), "temporary").expect("legacy temporary file");
    fs::create_dir_all(paths.config_dir()).expect("config root");
    fs::write(paths.config_file(), "current").expect("current config");

    let report = paths.migrate_legacy().expect("migrate with conflict");

    assert_eq!(fs::read_to_string(paths.config_file()).expect("current config"), "current");
    assert!(
        report
            .skipped
            .iter()
            .any(|skip| skip.reason == MigrationSkipReason::DestinationExists)
    );
    assert!(report.skipped.iter().any(|skip| skip.reason == MigrationSkipReason::Excluded));
    assert!(!paths.runtime_dir().join("tmp").exists());
}

#[test]
fn migration_does_not_trust_legacy_migration_metadata() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    let legacy_migration = paths.legacy_home_dir().join("state/migration");
    fs::create_dir_all(&legacy_migration).expect("legacy migration directory");
    fs::write(legacy_migration.join("legacy-v1.complete"), "spoofed\n").expect("spoofed marker");

    let report = paths.migrate_legacy().expect("migrate legacy metadata");

    assert!(report.marker_written);
    assert_eq!(
        fs::read_to_string(paths.migration_marker_path()).expect("current migration marker"),
        "legacy migration completed\n"
    );
    assert!(
        !report
            .migrated
            .iter()
            .any(|entry| entry.destination == paths.migration_marker_path())
    );
    assert!(report.skipped.iter().any(|skip| {
        skip.path == paths.legacy_home_dir().join("state/migration") && skip.reason == MigrationSkipReason::Excluded
    }));
}

#[cfg(unix)]
#[test]
fn private_file_writer_rejects_symlink_escape_and_final_symlink() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("tempdir");
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).expect("outside directory");
    let escaped_parent = temp.path().join("escaped");
    symlink(&outside, &escaped_parent).expect("escape symlink");

    assert!(VtCodePaths::write_private_file_atomic(escaped_parent.join("data"), b"blocked").is_err());
    assert!(!outside.join("data").exists());

    let safe_parent = temp.path().join("safe");
    fs::create_dir_all(&safe_parent).expect("safe directory");
    let destination = safe_parent.join("data");
    fs::write(&destination, "original").expect("destination");
    let linked = safe_parent.join("linked");
    symlink(&destination, &linked).expect("final symlink");
    assert!(VtCodePaths::write_private_file_atomic(&linked, b"blocked").is_err());
    assert_eq!(fs::read_to_string(destination).expect("original data"), "original");
}

#[test]
fn private_file_writer_replaces_existing_file_without_temporary_leaks() {
    let temp = tempdir().expect("tempdir");
    let destination = temp.path().join("cache/data");

    VtCodePaths::write_private_file_atomic(&destination, b"old committed state").expect("first write");
    VtCodePaths::write_private_file_atomic(&destination, b"new").expect("replace existing file");

    assert_eq!(fs::read(&destination).expect("read replacement"), b"new");
    assert_eq!(fs::read_dir(destination.parent().expect("parent")).expect("directory").count(), 1);
}

#[cfg(windows)]
#[test]
fn private_file_writer_failed_replacement_preserves_committed_file_and_cleans_temporary() {
    use std::os::windows::fs::OpenOptionsExt;

    let temp = tempdir().expect("tempdir");
    let destination = temp.path().join("data");
    VtCodePaths::write_private_file_atomic(&destination, b"committed").expect("committed write");
    let locked = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&destination)
        .expect("lock destination against replacement");

    let error = VtCodePaths::write_private_file_atomic(&destination, b"rejected replacement")
        .expect_err("replacement must fail while destination is locked");
    assert!(error.to_string().contains("could not atomically write"));
    drop(locked);

    assert_eq!(fs::read(&destination).expect("read committed state"), b"committed");
    assert_eq!(fs::read_dir(temp.path()).expect("directory").count(), 1);
}

#[test]
fn private_file_writer_if_absent_does_not_replace_existing_file() {
    let temp = tempdir().expect("tempdir");
    let destination = temp.path().join("cache/data");

    assert!(VtCodePaths::write_private_file_atomic_if_absent(&destination, b"first").expect("create file"));
    assert!(!VtCodePaths::write_private_file_atomic_if_absent(&destination, b"second").expect("keep file"));
    assert_eq!(fs::read(&destination).expect("read file"), b"first");
}

#[test]
fn private_file_lock_releases_after_operation() {
    let temp = tempdir().expect("tempdir");
    let destination = temp.path().join("cache/data");
    let lock_path = destination.parent().expect("cache parent").join(".data.lock");

    let result =
        VtCodePaths::with_private_file_lock(&destination, || Ok::<_, anyhow::Error>(17)).expect("lock operation");

    assert_eq!(result, 17);
    assert!(lock_path.is_file());
    assert_eq!(
        VtCodePaths::with_private_file_lock(&destination, || Ok::<_, anyhow::Error>(23)).expect("lock can be reused"),
        23
    );
}

#[test]
fn private_file_lock_serializes_concurrent_operations() {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    let temp = tempdir().expect("tempdir");
    let destination = Arc::new(temp.path().join("cache/data"));
    let start = Arc::new(Barrier::new(4));
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let handles = (0..4)
        .map(|_| {
            let destination = Arc::clone(&destination);
            let start = Arc::clone(&start);
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            std::thread::spawn(move || {
                let _ = start.wait();
                VtCodePaths::with_private_file_lock(destination.as_ref(), || {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    let _ = max_active.fetch_max(current, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(25));
                    let _ = active.fetch_sub(1, Ordering::SeqCst);
                    Ok::<_, anyhow::Error>(())
                })
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("lock thread panicked").expect("lock operation");
    }
    assert_eq!(max_active.load(Ordering::SeqCst), 1);
}

#[cfg(unix)]
#[test]
fn migration_skips_symlinks_and_special_files_without_traversing_them() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    let outside = temp.path().join("outside");
    fs::create_dir_all(&outside).expect("outside root");
    fs::write(outside.join("secret"), "secret").expect("outside secret");
    fs::create_dir_all(paths.legacy_home_dir().join("plugins")).expect("legacy plugins");
    symlink(&outside, paths.legacy_home_dir().join("plugins/link")).expect("legacy symlink");
    let socket = paths.legacy_home_dir().join("plugins/socket");
    let _listener = std::os::unix::net::UnixListener::bind(&socket).expect("create unix socket");

    let report = paths.migrate_legacy().expect("migrate safely");

    assert!(report.skipped.iter().any(|skip| skip.reason == MigrationSkipReason::Symlink));
    assert!(
        report
            .skipped
            .iter()
            .any(|skip| skip.reason == MigrationSkipReason::SpecialFile)
    );
    assert!(!paths.plugins_dir().join("link/secret").exists());
}

#[test]
fn migration_retries_destination_failures_before_writing_marker() {
    let temp = tempdir().expect("tempdir");
    let paths = migration_paths(&temp);
    fs::create_dir_all(paths.legacy_home_dir()).expect("legacy root");
    fs::write(paths.legacy_home_dir().join("vtcode.toml"), "legacy").expect("legacy config");
    fs::write(paths.config_dir(), "unsafe root").expect("unsafe config root");

    let first_report = paths.migrate_legacy().expect("migration report");

    assert!(!first_report.failures.is_empty());
    assert!(!first_report.marker_written);

    fs::remove_file(paths.config_dir()).expect("remove blocked config root");
    let second_report = paths.migrate_legacy().expect("retry migration");

    assert!(second_report.marker_written);
    assert_eq!(fs::read_to_string(paths.config_file()).expect("migrated config"), "legacy");
}
