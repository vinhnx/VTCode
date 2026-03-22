use predicates::str::contains;
use tempfile::tempdir;

#[test]
fn pods_list_dispatches_through_the_binary() {
    let home = tempdir().expect("create temp home");

    assert_cmd::cargo::cargo_bin_cmd!("vtcode")
        .env("HOME", home.path())
        .args(["pods", "list"])
        .assert()
        .failure()
        .stderr(contains("no active pod configured"));
}
