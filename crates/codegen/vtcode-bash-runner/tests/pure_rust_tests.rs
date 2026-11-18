//! Regression tests for structured pure-Rust option flags.
//!
//! The executor must read recursion/hidden behavior from the invocation
//! instead of inferring flags from the command text, where path names can
//! contain flag-like substrings.

#![cfg(feature = "pure-rust")]

use std::fs;

use anyhow::Result;
use assert_fs::TempDir;
use vtcode_bash_runner::{AllowAllPolicy, BashRunner, PureRustCommandExecutor};

fn pure_rust_runner(dir: &TempDir) -> Result<BashRunner<PureRustCommandExecutor, AllowAllPolicy>> {
    BashRunner::new(dir.path().to_path_buf(), PureRustCommandExecutor, AllowAllPolicy)
}

#[test]
fn nonrecursive_rm_ignores_dash_r_inside_path_names() -> Result<()> {
    let dir = TempDir::new()?;
    let target = dir.path().join("my-r-dir");
    fs::create_dir_all(&target)?;
    fs::write(target.join("keep.txt"), "keep")?;
    let runner = pure_rust_runner(&dir)?;
    assert!(runner.rm("my-r-dir", false, false).is_err());
    assert_eq!(fs::read_to_string(target.join("keep.txt"))?, "keep");
    Ok(())
}

#[test]
fn recursive_rm_still_removes_directories() -> Result<()> {
    let dir = TempDir::new()?;
    let target = dir.path().join("plain-dir");
    fs::create_dir_all(&target)?;
    fs::write(target.join("gone.txt"), "gone")?;
    let runner = pure_rust_runner(&dir)?;
    runner.rm("plain-dir", true, false)?;
    assert!(!target.exists());
    Ok(())
}

#[test]
fn nonrecursive_copy_ignores_dash_r_inside_path_names() -> Result<()> {
    let dir = TempDir::new()?;
    fs::create_dir_all(dir.path().join("source"))?;
    fs::write(dir.path().join("source").join("file.txt"), "data")?;
    let runner = pure_rust_runner(&dir)?;
    assert!(runner.cp("source", "dest-r", false).is_err());
    assert!(!dir.path().join("dest-r").join("file.txt").exists());
    Ok(())
}

#[test]
fn ls_hidden_flag_ignores_dash_a_inside_path_names() -> Result<()> {
    let dir = TempDir::new()?;
    let tricky = dir.path().join("my-a-dir");
    fs::create_dir_all(&tricky)?;
    fs::write(tricky.join(".hidden"), "hidden")?;
    fs::write(tricky.join("visible.txt"), "visible")?;
    let plain = dir.path().join("plain");
    fs::create_dir_all(&plain)?;
    fs::write(plain.join(".hidden"), "hidden")?;
    let runner = pure_rust_runner(&dir)?;
    let without_hidden = runner.ls(Some("my-a-dir"), false)?;
    assert!(!without_hidden.contains(".hidden"), "{without_hidden}");
    assert!(without_hidden.contains("visible.txt"), "{without_hidden}");
    let with_hidden = runner.ls(Some("my-a-dir"), true)?;
    assert!(with_hidden.contains(".hidden"), "{with_hidden}");
    // A genuine Unix `-la` request must also work on ordinary names.
    let plain_shown = runner.ls(Some("plain"), true)?;
    assert!(plain_shown.contains(".hidden"), "{plain_shown}");
    Ok(())
}
