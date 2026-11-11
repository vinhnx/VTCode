#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
#![cfg(unix)]
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Writes an executable stub that records the arguments it was called with.
fn create_fake_ast_grep(temp_dir: &Path, name: &str, args_log: &Path) -> PathBuf {
    let binary = temp_dir.join(name);
    // The log is appended to so every invocation is recorded, not just the last.
    let body = format!(
        "#!/bin/sh\n\
printf '%s\\n' \"$@\" >> '{}'\n\
exit 0\n",
        args_log.display(),
    );
    fs::write(&binary, body).expect("write fake ast-grep");

    let mut permissions = fs::metadata(&binary).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&binary, permissions).expect("chmod");

    binary
}

#[test]
fn check_script_runs_ast_grep_test_and_scan_directly() {
    let fake_bin = TempDir::new().expect("fake bin");
    let args_log = fake_bin.path().join("ast-grep-args.log");
    let _fake_ast_grep = create_fake_ast_grep(fake_bin.path(), "ast-grep", &args_log);
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut command = std::process::Command::new("bash");
    let _configured_command = command.current_dir(&workspace).arg("scripts/check.sh").arg("ast-grep").env(
        "PATH",
        format!("{}:{}", fake_bin.path().display(), std::env::var_os("PATH").unwrap_or_default().to_string_lossy()),
    );

    let status = command.status().expect("run check.sh ast-grep");
    assert!(status.success(), "the ast-grep gate must succeed when the stub passes");

    let args = fs::read_to_string(&args_log).expect("read args log");
    let calls: Vec<&str> = args.lines().collect();
    let subcommands: Vec<&str> = calls.iter().copied().filter(|arg| *arg == "test" || *arg == "scan").collect();
    assert_eq!(subcommands, ["test", "scan"], "the rule tests run first, then the repository scan");
}
