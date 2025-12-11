use assert_cmd::prelude::*;
use assert_fs::TempDir;
use predicates::prelude::*;
use std::process::Command;

#[test]
fn print_mode_requires_prompt_or_stdin() {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    cmd.arg("--print")
        .env("OLLAMA_API_KEY", "test-key")
        .env("NO_COLOR", "1");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No prompt provided"));
}

#[test]
fn config_override_failure_is_reported() {
    let temp_dir = TempDir::new().expect("failed to create temp workspace");
    let missing_config = temp_dir.path().join("missing-config.toml");

    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    cmd.arg("--workspace")
        .arg(temp_dir.path())
        .arg("--config")
        .arg(&missing_config)
        .arg("--print")
        .arg("hello")
        .env("OLLAMA_API_KEY", "test-key")
        .env("NO_COLOR", "1")
        .current_dir(temp_dir.path());

    cmd.assert().failure().stderr(
        predicate::str::contains("failed to initialize VTCode startup context")
            .and(predicate::str::contains(missing_config.to_string_lossy())),
    );
}
