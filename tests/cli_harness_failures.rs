use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;

#[path = "../vtcode-core/tests/support/mod.rs"]
mod support;

use support::TestHarness;

fn base_command(harness: &TestHarness) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    cmd.env("OLLAMA_API_KEY", "test-key")
        .env("OPENAI_API_KEY", "test-key")
        .env("NO_COLOR", "1")
        .current_dir(harness.workspace());
    cmd
}

#[test]
fn print_mode_requires_prompt_or_stdin() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    harness
        .write_file(".vtcode/.keep", "")
        .expect("failed to mark workspace initialized");
    let mut cmd = base_command(&harness);
    cmd.arg("--print");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("No prompt provided"));
}

#[test]
fn config_override_failure_is_reported() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    let missing_config = harness.workspace().join("missing-config.toml");

    let mut cmd = base_command(&harness);
    cmd.arg("--workspace")
        .arg(harness.workspace())
        .arg("--config")
        .arg(&missing_config)
        .arg("--print")
        .arg("hello")
        .current_dir(harness.workspace());

    cmd.assert().failure().stderr(
        predicate::str::contains("failed to initialize VT Code startup context")
            .and(predicate::str::contains(missing_config.to_string_lossy())),
    );
}

#[test]
fn unknown_positional_token_fails_without_forwarding_prompt_to_llm() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    harness
        .write_file(".vtcode/.keep", "")
        .expect("failed to mark workspace initialized");

    let mut cmd = base_command(&harness);
    cmd.arg("hellp");

    cmd.assert().failure().stderr(
        predicate::str::contains("invalid value")
            .and(predicate::str::contains("Workspace path does not exist"))
            .and(predicate::str::contains("try '--help'"))
            .and(predicate::str::contains("Sending prompt to").not()),
    );
}
