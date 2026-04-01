use assert_cmd::Command;
use tempfile::TempDir;

fn isolated_vtcode_command() -> (TempDir, Command) {
    let home = tempfile::tempdir().expect("create temp home");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("vtcode");
    cmd.env("HOME", home.path())
        .env("VTCODE_CONFIG", home.path())
        .env_remove("VTCODE_CONFIG_PATH")
        .current_dir(home.path())
        .env("NO_COLOR", "1");
    (home, cmd)
}

#[test]
fn vtcode_help_command_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    cmd.arg("--help");
    cmd.assert().success();
}

#[test]
fn vtcode_tool_policy_status_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    cmd.arg("tool-policy").arg("status");
    cmd.assert().success();
}

#[test]
fn vtcode_tool_policy_help_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    cmd.arg("tool-policy").arg("--help");
    cmd.assert().success();
}
