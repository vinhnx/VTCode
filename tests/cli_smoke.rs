#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

fn isolated_vtcode_command() -> (TempDir, Command) {
    let home = tempfile::tempdir().expect("create temp home");
    let mut cmd = assert_cmd::cargo::cargo_bin_cmd!("vtcode");
    let _configured_command = cmd
        .env("HOME", home.path())
        .env("VTCODE_CONFIG", home.path())
        .env_remove("VTCODE_CONFIG_PATH")
        .current_dir(home.path())
        .env("NO_COLOR", "1");
    (home, cmd)
}

#[test]
fn vtcode_help_command_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    let _argument = cmd.arg("--help");
    let _assertion = cmd.assert().success();
}

#[test]
fn vtcode_debug_capabilities_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    let _argument = cmd.arg("debug").arg("capabilities");
    let _assertion = cmd.assert().success().stdout(contains("VT Code Sandbox Capabilities"));
}

#[test]
fn vtcode_tool_policy_status_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    let _argument = cmd.arg("tool-policy").arg("status");
    let _assertion = cmd.assert().success();
}

#[test]
fn vtcode_tool_policy_help_succeeds() {
    let (_home, mut cmd) = isolated_vtcode_command();
    let _argument = cmd.arg("tool-policy").arg("--help");
    let _assertion = cmd.assert().success();
}

#[test]
fn vtcode_positional_workspace_with_tool_policy_status_succeeds() {
    let (home, mut cmd) = isolated_vtcode_command();
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).expect("create workspace");

    // Workspace is a `last = true` positional: command first, path after `--`.
    let _argument = cmd.arg("tool-policy").arg("status").arg("--").arg(&workspace);
    let _assertion = cmd.assert().success();
}

#[test]
fn vtcode_invalid_option_shows_usage_help() {
    let (_home, mut cmd) = isolated_vtcode_command();
    let _argument = cmd.arg("--definitely-invalid-option");
    let _assertion = cmd.assert().failure().stderr(contains("Usage: vtcode"));
}
