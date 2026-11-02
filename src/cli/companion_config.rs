//! Per-user setup for the local VT Code Companion marketplace plugin.

use crate::startup::StartupContext;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use vtcode_commons::VtCodePaths;
use vtcode_core::cli::args::{CompanionCommand, CompanionSetupArgs};

const CONFIG_FILE: &str = "companion.json";

pub(crate) fn handle_companion_command(startup: &StartupContext, command: CompanionCommand) -> Result<()> {
    match command {
        CompanionCommand::Setup(args) => setup(startup, args),
    }
}

fn setup(startup: &StartupContext, args: CompanionSetupArgs) -> Result<()> {
    let binary = args
        .binary
        .unwrap_or(std::env::current_exe().context("could not locate this VT Code executable")?);
    let workspace = startup.workspace.clone();
    let binary = validate_binary(&binary)?;
    let workspace = validate_workspace(&workspace)?;
    let paths = VtCodePaths::resolve().context("could not resolve VT Code configuration directory")?;
    let config = paths.config_dir().join(CONFIG_FILE);
    save_config(&config, &binary, &workspace)?;

    println!("VT Code Companion is configured.");
    println!("  Binary:    {}", binary.display());
    println!("  Workspace: {}", workspace.display());
    println!("  Settings:  {}", config.display());
    println!("Restart the plugin connection in Codex or ChatGPT Desktop to use it.");
    Ok(())
}

fn save_config(config: &Path, binary: &Path, workspace: &Path) -> Result<()> {
    let contents = serde_json::to_vec_pretty(&serde_json::json!({
        "binary": binary,
        "workspace": workspace,
    }))
    .context("could not encode Companion setup")?;
    let parent = config.parent().context("Companion config path has no parent")?;
    VtCodePaths::ensure_user_dir(parent)?;
    VtCodePaths::write_private_file_atomic(config, &contents)
        .with_context(|| format!("could not save Companion setup to {}", config.display()))
}

fn validate_binary(path: &Path) -> Result<PathBuf> {
    let path = vtcode_commons::paths::canonicalize(path)
        .with_context(|| format!("could not resolve VT Code binary {}", path.display()))?;
    if !path.is_file() {
        bail!("VT Code binary must be a regular file: {}", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if path.metadata()?.permissions().mode() & 0o111 == 0 {
            bail!("VT Code binary is not executable: {}", path.display());
        }
    }
    Ok(path)
}

fn validate_workspace(path: &Path) -> Result<PathBuf> {
    let path = vtcode_commons::paths::canonicalize(path)
        .with_context(|| format!("could not resolve workspace {}", path.display()))?;
    if !path.is_dir() {
        bail!("Companion workspace must be a directory: {}", path.display());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{save_config, validate_binary, validate_workspace};
    use clap::Parser;
    use std::fs;
    use vtcode_core::cli::args::{Cli, Commands, CompanionCommand};

    #[test]
    fn companion_setup_requires_an_executable_and_workspace_directory() {
        let directory = tempfile::tempdir().expect("temp directory");
        let binary = directory.path().join("vtcode");
        fs::write(&binary, b"binary").expect("binary file");
        let workspace = directory.path().join("workspace");
        fs::create_dir(&workspace).expect("workspace directory");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).expect("executable permission");
        }
        assert!(validate_binary(&binary).is_ok());
        assert_eq!(
            validate_workspace(&workspace).expect("valid workspace"),
            vtcode_commons::paths::canonicalize(&workspace).unwrap()
        );
        assert!(validate_workspace(&binary).is_err());

        let config = directory.path().join("config/companion.json");
        let canonical_binary = validate_binary(&binary).expect("canonical binary");
        let canonical_workspace = validate_workspace(&workspace).expect("canonical workspace");
        save_config(&config, &canonical_binary, &canonical_workspace).expect("save Companion setup");
        let saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&config).expect("read setup")).expect("valid setup JSON");
        assert_eq!(saved["binary"], canonical_binary.to_string_lossy().as_ref());
        assert_eq!(saved["workspace"], canonical_workspace.to_string_lossy().as_ref());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&config).expect("setup metadata").permissions().mode() & 0o777, 0o600);
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&binary, fs::Permissions::from_mode(0o600)).expect("non-executable permission");
            assert!(validate_binary(&binary).is_err());
        }
    }

    #[test]
    fn companion_setup_uses_global_workspace_and_selected_binary_arguments() {
        let directory = tempfile::tempdir().expect("temp directory");
        let cli = Cli::try_parse_from([
            "vtcode",
            "--workspace",
            directory.path().to_str().expect("UTF-8 temp path"),
            "companion",
            "setup",
            "--binary",
            "/tmp/vtcode",
        ])
        .expect("parse setup arguments");
        assert_eq!(cli.workspace.as_deref(), Some(directory.path()));
        assert!(matches!(cli.command, Some(Commands::Companion { command: CompanionCommand::Setup(_) })));
    }
}
