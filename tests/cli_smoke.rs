use assert_cmd::Command;

#[test]
fn vtcode_help_command_succeeds() {
    let mut cmd = Command::cargo_bin("vtcode").expect("vtcode binary should build");
    cmd.arg("--help").env("NO_COLOR", "1");
    cmd.assert().success();
}
