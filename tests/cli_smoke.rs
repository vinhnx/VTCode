use assert_cmd::Command;

#[test]
fn vtcode_help_command_succeeds() {
    let mut cmd = Command::cargo_bin("vtcode").expect("vtcode binary should build");
    cmd.arg("--help").env("NO_COLOR", "1");
    cmd.assert().success();
}

#[test]
fn vtcode_tool_policy_status_succeeds() {
    let mut cmd = Command::cargo_bin("vtcode").expect("vtcode binary should build");
    cmd.arg("tool-policy")
        .arg("status")
        .env("NO_COLOR", "1");
    cmd.assert().success();
}

#[test]
fn vtcode_tool_policy_help_succeeds() {
    let mut cmd = Command::cargo_bin("vtcode").expect("vtcode binary should build");
    cmd.arg("tool-policy")
        .arg("--help")
        .env("NO_COLOR", "1");
    cmd.assert().success();
}
