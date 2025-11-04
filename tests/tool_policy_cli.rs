use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn tool_policy_status_lists_run_terminal_cmd() {
    let mut cmd = Command::cargo_bin("vtcode").expect("vtcode binary");
    cmd.arg("tool-policy").arg("status");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("run_terminal_cmd"));
}
