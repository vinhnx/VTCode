use anyhow::Result;
use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn config_command_writes_configuration_file() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let output_path = temp_dir.path().join("vtcode-config.toml");

    let mut cmd = Command::cargo_bin("vtcode")?;
    cmd.current_dir(temp_dir.path());
    cmd.arg("config");
    cmd.arg("--output");
    cmd.arg(&output_path);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Configuration written"));

    let contents = fs::read_to_string(&output_path)?;
    assert!(
        contents.contains("[agent]"),
        "generated config should include the agent section"
    );

    Ok(())
}

#[test]
fn positional_workspace_path_must_exist() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let missing_workspace = temp_dir.path().join("missing-workspace");
    let output_path = temp_dir.path().join("should-not-exist.toml");

    let mut cmd = Command::cargo_bin("vtcode")?;
    cmd.current_dir(temp_dir.path());
    cmd.arg(&missing_workspace);
    cmd.arg("config");
    cmd.arg("--output");
    cmd.arg(&output_path);

    let assert = cmd.assert().failure();
    assert.stderr(predicate::str::contains("does not exist"));

    assert!(
        !output_path.exists(),
        "config command should not create output when workspace is invalid"
    );

    Ok(())
}
