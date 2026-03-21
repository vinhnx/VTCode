use assert_cmd::Command;
use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn pods_list_dispatches_through_the_binary() {
    let home = tempdir().expect("create temp home");

    Command::cargo_bin("vtcode")
        .expect("binary exists")
        .env("HOME", home.path())
        .args(["pods", "list"])
        .assert()
        .failure()
        .stderr(contains("no active pod configured"));
}
