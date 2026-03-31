#![cfg(unix)]

use assert_cmd::Command;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn create_fake_vtcode(temp_dir: &TempDir) -> PathBuf {
    let fake_vtcode = temp_dir.path().join("vtcode");
    let body = format!(
        "#!/bin/sh\n\
printf '%s\\n' \"$@\" > '{args_log}'\n\
pwd > '{cwd_log}'\n",
        args_log = temp_dir.path().join("vtcode-args.log").display(),
        cwd_log = temp_dir.path().join("vtcode-cwd.log").display(),
    );
    fs::write(&fake_vtcode, body).expect("write fake vtcode");

    use std::os::unix::fs::PermissionsExt;

    let mut permissions = fs::metadata(&fake_vtcode).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_vtcode, permissions).expect("chmod");

    fake_vtcode
}

#[test]
fn check_script_delegates_ast_grep_to_vtcode_command() {
    let fake_bin = TempDir::new().expect("fake bin");
    let _fake_vtcode = create_fake_vtcode(&fake_bin);
    let current_path = std::env::var_os("PATH").unwrap_or_default();
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let mut command = Command::new("bash");
    command
        .current_dir(&workspace)
        .arg("scripts/check.sh")
        .arg("ast-grep")
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.path().display(),
                current_path.to_string_lossy()
            ),
        );

    command.assert().success();

    let args = fs::read_to_string(fake_bin.path().join("vtcode-args.log")).expect("read args log");
    assert_eq!(args.lines().collect::<Vec<_>>(), ["check", "ast-grep"]);

    let cwd = fs::read_to_string(fake_bin.path().join("vtcode-cwd.log")).expect("read cwd log");
    assert_eq!(cwd.trim(), workspace.display().to_string());
}
