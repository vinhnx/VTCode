use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use vtcode_core::cli::args::Cli;
use vtcode_core::config::loader::{ConfigBuilder, VTCodeConfig};
use vtcode_core::utils::validation::validate_path_exists;

use super::first_run::maybe_run_first_run_setup;
use super::validation::{
    parse_cli_config_entries, resolve_config_path, resolve_workspace_path,
    validate_additional_directories,
};

pub(super) struct LoadedStartupConfig {
    pub(super) workspace: PathBuf,
    pub(super) config: VTCodeConfig,
    pub(super) first_run_occurred: bool,
    pub(super) full_auto_requested: bool,
    pub(super) automation_prompt: Option<String>,
}

pub(super) async fn load_startup_config(args: &Cli) -> Result<LoadedStartupConfig> {
    let workspace_override = args
        .workspace_path
        .clone()
        .or_else(|| args.workspace.clone());

    let workspace = resolve_workspace_path(workspace_override)
        .context("Failed to resolve workspace directory")?;
    if args.workspace_path.is_some() {
        validate_path_exists(&workspace, "Workspace")?;
    }

    validate_additional_directories(&args.additional_dirs)?;
    let (cli_config_path_override, inline_config_overrides) =
        parse_cli_config_entries(&args.config);
    let env_config_path_override = std::env::var("VTCODE_CONFIG_PATH").ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    });
    let config_path_override = cli_config_path_override.or(env_config_path_override);

    let mut builder = ConfigBuilder::new().workspace(workspace.clone());
    if let Some(path_override) = config_path_override {
        let resolved_path = resolve_config_path(&workspace, &path_override);
        builder = builder.config_file(resolved_path);
    }

    if !inline_config_overrides.is_empty() {
        builder = builder.cli_overrides(&inline_config_overrides);
    }

    if let Some(ref model) = args.model {
        builder = builder.cli_override(
            "agent.default_model".to_owned(),
            toml::Value::String(model.clone()),
        );
    }
    if let Some(ref provider) = args.provider {
        builder = builder.cli_override(
            "agent.provider".to_owned(),
            toml::Value::String(provider.clone()),
        );
    }

    let manager = builder.build().context("Failed to load configuration")?;
    let mut config = manager.config().clone();

    let (full_auto_requested, automation_prompt) = match args.full_auto.clone() {
        Some(value) if value.trim().is_empty() => (true, None),
        Some(value) => (true, Some(value)),
        None => (false, None),
    };

    let first_run_occurred = maybe_run_first_run_setup(args, &workspace, &mut config).await?;

    if automation_prompt.is_some() && args.command.is_some() {
        bail!(
            "--auto/--full-auto with a prompt cannot be combined with other commands. Provide only the prompt."
        );
    }

    Ok(LoadedStartupConfig {
        workspace,
        config,
        first_run_occurred,
        full_auto_requested,
        automation_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tempfile::TempDir;
    use vtcode_core::cli::args::Cli;

    #[tokio::test]
    async fn cli_config_path_override_loads_requested_file() {
        let temp_dir = TempDir::new().expect("temp dir");
        let workspace = temp_dir.path().join("workspace");
        std::fs::create_dir(&workspace).expect("workspace dir");
        std::fs::create_dir(workspace.join(".vtcode")).expect("workspace dot dir");

        let config_path = temp_dir.path().join("custom-config.toml");
        std::fs::write(
            &config_path,
            r#"
[debug]
enable_tracing = true
"#,
        )
        .expect("custom config");

        let args = Cli::parse_from([
            "vtcode",
            "--workspace",
            workspace.to_str().expect("workspace path"),
            "--config",
            config_path.to_str().expect("config path"),
        ]);

        let loaded = load_startup_config(&args)
            .await
            .expect("startup config should load");

        assert!(loaded.config.debug.enable_tracing);
    }
}
