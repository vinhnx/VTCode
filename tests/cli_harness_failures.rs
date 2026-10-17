#![allow(
    missing_docs,
    clippy::expect_used,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
use assert_cmd::prelude::*;
use predicates::prelude::*;
use std::process::Command;
use tempfile::TempDir;
use vtcode_core::config::constants::models::openai::DEFAULT_MODEL;

#[path = "../crates/codegen/vtcode-core/tests/support/mod.rs"]
mod support;

use support::TestHarness;

/// Builds a CLI command using OpenAI's default model and a synthetic API key.
///
/// Pins the provider and model so startup does not depend on application
/// defaults, and isolates `HOME`/`VTCODE_CONFIG` into `isolated_home` so the
/// developer's own global `vtcode.toml` (a different `agent.provider` /
/// `agent.api_key_env`) cannot leak into the run.
fn base_command(harness: &TestHarness, isolated_home: &std::path::Path) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("vtcode"));
    let _configured_command = cmd
        .args(["--provider", "openai", "--model", DEFAULT_MODEL])
        .env("OPENAI_API_KEY", "test-key")
        .env("MERGE_GATEWAY_API_KEY", "test-key")
        .env("NO_COLOR", "1")
        .env("HOME", isolated_home)
        .env("VTCODE_CONFIG", isolated_home)
        .env_remove("VTCODE_CONFIG_PATH")
        .current_dir(harness.workspace());
    cmd
}

/// Isolated `HOME` shared by the CLI-harness failure tests.
///
/// Bound by each test so the directory outlives the spawned child process.
struct IsolatedHome(TempDir);

impl IsolatedHome {
    fn new() -> Self {
        Self(TempDir::new().expect("create isolated home"))
    }

    fn path(&self) -> &std::path::Path {
        self.0.path()
    }
}

/// Builds an isolated CLI command plus the guard that owns its temp home.
fn isolated_command(harness: &TestHarness) -> (IsolatedHome, Command) {
    let home = IsolatedHome::new();
    let cmd = base_command(harness, home.path());
    (home, cmd)
}

#[test]
fn print_mode_requires_prompt_or_stdin() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    let _marker = harness
        .write_file(".vtcode/.keep", "")
        .expect("failed to mark workspace initialized");
    let (_home, mut cmd) = isolated_command(&harness);
    let _argument = cmd.arg("--print");

    let _assertion = cmd.assert().failure().stderr(predicate::str::contains("No prompt provided"));
}

#[test]
fn config_override_failure_is_reported() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    let missing_config = harness.workspace().join("missing-config.toml");

    let (_home, mut cmd) = isolated_command(&harness);
    let _argument = cmd
        .arg("--workspace")
        .arg(harness.workspace())
        .arg("--config")
        .arg(&missing_config)
        .arg("--print")
        .arg("hello")
        .current_dir(harness.workspace());

    let _assertion = cmd.assert().failure().stderr(
        predicate::str::contains("failed to initialize VT Code startup context")
            .and(predicate::str::contains(missing_config.to_string_lossy())),
    );
}

#[test]
fn unknown_positional_token_fails_without_forwarding_prompt_to_llm() {
    let harness = TestHarness::new().expect("failed to init harness workspace");
    let _marker = harness
        .write_file(".vtcode/.keep", "")
        .expect("failed to mark workspace initialized");

    let (_home, mut cmd) = isolated_command(&harness);
    let _argument = cmd.arg("--").arg("hellp");

    let _assertion = cmd.assert().failure().stderr(
        predicate::str::contains("invalid value")
            .and(predicate::str::contains("is not a valid workspace path or subcommand"))
            .and(predicate::str::contains("try '--help'"))
            .and(predicate::str::contains("Sending prompt to").not()),
    );
}
